use std::{
    thread::{sleep,self},
    sync::{Arc, Mutex,atomic::{AtomicPtr, Ordering,AtomicBool},Condvar},
    collections::{HashMap,VecDeque},
    future::Future,
    pin::Pin,
    cell::{UnsafeCell, RefCell},
    time::Duration,
    rc::Rc,
};
use crate::DispatchCenter::multi::runtime::{Runtime,Status};
use crate::DispatchCenter::multi::join::{JoinHandle,Output};
use crate::DispatchCenter::multi::time::{Sleep};
use crate::DispatchCenter::multi::net::evenloop::EventLoop;
use crate::GeneralComponents::{id_bulider,static_exchange};
use log::{error, info};

use std::sync::OnceLock;

pub static SCHEDULER: (Mutex<bool>,Condvar) = (Mutex::new(false),Condvar::new());// 用于调度器信息回收唤醒状态
pub static TREAD_LOAD: OnceLock<Arc<Mutex<HashMap<u64, usize>>>> = OnceLock::new(); //线程负载
pub static CETER: OnceLock<AtomicPtr<Scheduler>> = OnceLock::new(); //调度器运行时运行时

#[derive(Debug, Clone)]
pub enum BackMsg{
    AllTaskFinished(u64),
    StealTask,
    Closed,
}

// 调度器结构体
// 用于全局任务管理
pub struct Scheduler{
    runtimes: HashMap<u64, RefCell<usize>>,// 任务数
    id_bulider: RefCell<id_bulider::IdBulider>,  // 任务id生成器
    thread_cont:usize,  // 线程数
    tasks:RefCell<u64>,
    backtasks:RefCell<u64>
}

// 调度器初始化
// 用于全局任务管理
impl Scheduler{
    pub fn new(thread_cont:usize){
        let mut scheduler = Self{
            runtimes: HashMap::new(),
            id_bulider: RefCell::new(id_bulider::IdBulider::new()),
            thread_cont,
            tasks:RefCell::new(0),
            backtasks:RefCell::new(0),
        };
        scheduler.build_runtime();// 初始化线程、全局负载表、运行时
        match CETER.set(AtomicPtr::new(Box::into_raw(Box::new(scheduler)))){
            Ok(_) => {},
            Err(e) => error!("调度器运行时运行时初始化错误: {:?}", e),
        }
    }

    pub fn insert_runtime(&mut self, id: u64){
        self.runtimes.insert(id, RefCell::new(0));
    }

    //获取调度器实例
    pub fn get()->  &'static mut Scheduler{
        unsafe{ &mut (*CETER.get().unwrap().load(Ordering::Relaxed) ) }
    }

    // 初始化线程、全局负载表、运行时
    fn build_runtime(&mut self){
        static_exchange::init(self.thread_cont);
        for _ in 0..self.thread_cont{
            let id = self.id_bulider.borrow_mut().get_id();
            self.insert_runtime(id);
            match TREAD_LOAD.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))).try_lock(){
                Ok(mut loads) => {
                    loads.insert(id, 0);
                },
                Err(e) => error!("全局负载表初始化错误: {:?}", e),
            }
            let threadwait=static_exchange::EXCHANGE_LIST.get().unwrap()[id as usize].2.clone();
            thread::spawn(move ||{
                let runtime = Runtime::new(id);
                loop{
                    runtime.run();
                    match runtime.is_running(){
                        Status::Closed => {
                            info!("运行时 {} 已关闭", id);
                            break;
                        },
                        Status::Idle => {
                            threadwait.wait();
                            info!("运行时 {} 已空闲", id);
                        },
                        _ => ()
                    }
                }
            });
            info!("运行时 {} 已启动", id);
        }
    }

    //批量下发future任务到线程，由调度线程自行分配任务
    pub fn add(mut future: Vec<Pin<Box<dyn Future<Output = ()> + Send>>>){
        let loads=TREAD_LOAD.get().unwrap().lock().unwrap();
        let future_len = future.len();
        *Scheduler::get().tasks.borrow_mut() += future_len as u64;
        let thread_size=loads.len();
        let mid_load = (loads.values().sum::<usize>()+future_len)/thread_size;
        let mut loadmap = HashMap::new();
        for (threadid, load) in loads.iter(){
            if *load <= mid_load{
                continue;
            }else{
                loadmap.insert(*threadid, *load);
            }
        }
        if loadmap.is_empty(){
            let add_load=if future_len < thread_size{
                1
            }else{
                future_len/thread_size
            };
            for (threadid, _) in loads.iter(){
                if future.is_empty() {
                    break;
                }
                let tasks=if add_load >= future.len(){
                    TaskMsg::AddFuture(future.drain(..).collect())
                }else{
                    TaskMsg::AddFuture(future.drain(..add_load).collect())
                };
                let exchange_list = static_exchange::get_list(*threadid as usize).unwrap();
                unsafe{
                    match (*exchange_list).0.push(tasks){
                        Ok(_) => {
                            let threadwait=static_exchange::EXCHANGE_LIST.get().unwrap()[*threadid as usize].2.clone();
                            threadwait.check(*threadid as usize);
                        },
                        Err(e) => error!("运行时 {} 任务交换列表推送错误: {:?}", threadid, e),
                    }
                }
            }
        }else{
            let mid_load = (loadmap.values().sum::<usize>()+future_len)/loadmap.len();
            for (threadid, load) in loadmap.iter(){
                if future.is_empty() {
                    break;
                }
                let add_load =mid_load - *load;
                let actual_load = if add_load < 1 {
                    1
                } else {
                    add_load
                };
                let tasks=if actual_load >= future.len(){
                    TaskMsg::AddFuture(future.drain(..).collect())
                }else{
                    TaskMsg::AddFuture(future.drain(..actual_load).collect())
                };
                let exchange_list = static_exchange::get_list(*threadid as usize).unwrap();
                unsafe{
                    match (*exchange_list).0.push(tasks){
                        Ok(_) => {
                            let threadwait=static_exchange::EXCHANGE_LIST.get().unwrap()[*threadid as usize].2.clone();
                            threadwait.check(*threadid as usize);
                        },
                        Err(e) => error!("运行时 {} 任务交换列表推送错误: {:?}", threadid, e),
                    }
                }
            }
        }
    }

    // 包装一个future任务到一个线程中，返回一个JoinHandle
    pub fn spawn<T: Send+ 'static>( future: impl Future<Output = T>+ Send+ 'static)->JoinHandle<T>{
        let output = Arc::new(Mutex::new(Output::new()));
        let r_output = output.clone();
        let join_handle = JoinHandle::new(output);
        let result=async move {
            let o = future.await;
            *r_output.lock().unwrap().data.borrow_mut() = Some(o);
            r_output.lock().unwrap().notify();
        };
        let l:Vec<Pin<Box<dyn Future<Output = ()> + Send+ 'static>>> =vec![Box::pin(result)];
        Scheduler::add(l);
        join_handle
    }

    // 线程睡眠指定时间
    pub fn sleep(duration: Duration)->Sleep{
        Sleep::new(duration)
    }

    
    // 关闭所有运行时
    pub fn close(){
        let map=static_exchange::loop_get_map();
        for (id,list) in map.iter().enumerate(){
            let list_prt = &list.0;
            match list_prt.0.push(TaskMsg::Close){
                    Ok(_) => info!("已向运行时 {} 发送关闭信号", id),
                    Err(e) => error!("运行时 {} 任务交换列表推送错误: {:?}", id, e),
            }
        }
    }

    // 获取所有的运行时返回消息
    pub fn get_back_msg(&mut self) ->HashMap<u64, Vec<BackMsg>>{
        let exchange_list = static_exchange::EXCHANGE_LIST.get().unwrap();
        let mut msgmap = HashMap::new();
        for (index, list) in exchange_list.iter().enumerate(){
            let threadid = index;
            let list_prt = &list.0;
            match list_prt.1.pop_all(){
                   Ok(msg) => {
                        match msg{
                            Some(msgs) => {
                                msgmap.insert(threadid as u64, msgs);
                            },
                            None => {},
                        }
                   },
                   Err(e) => {
                       error!("运行时 {} 任务交换列表弹出错误: {:?}", threadid, e);
                   }
               }
        }
        msgmap
    }

    // 等待所有运行时完成任务，完成后自动关闭所有运行时
    pub fn join(){
        let  scheduler = Scheduler::get();
        loop{
            let msgmap=scheduler.get_back_msg();
            for (threadid, msgs) in msgmap.iter(){
                for msg in msgs{
                    match msg{
                        BackMsg::AllTaskFinished(count)=>{
                            *scheduler.backtasks.borrow_mut() += count;
                            info!("运行时 {} 所有任务完成, 已完成 {} 个任务", threadid, count);  
                        },
                        BackMsg::Closed=>{
                            scheduler.runtimes.remove(&threadid);
                            info!("运行时 {} 已关闭", threadid);
                        }
                        _=>{}
                    }
                }
            }
            *SCHEDULER.0.lock().unwrap()=false;
            if *scheduler.backtasks.borrow() == *scheduler.tasks.borrow(){
                info!("所有任务已完成, 共计完成 {} 个任务", *scheduler.backtasks.borrow());
                break;
            }
            let (state,condvar) = &SCHEDULER;
            let mut b=state.lock().unwrap();
            let result=condvar.wait_timeout(b, Duration::from_millis(10)).unwrap();
            b=result.0;
            if result.1.timed_out() {
                continue;
            }
        }
    }
}


impl Drop for Scheduler{
    fn drop(&mut self){
        Scheduler::close();
        if let Some(prt) =CETER.get(){
            unsafe{drop(Box::from_raw(prt.load(Ordering::Acquire)))};
        }
    }
}


pub enum TaskMsg{
    AddFuture(Vec<Pin<Box<dyn Future<Output = ()> + Send>>>),
    PushWakeId(u64),
    StealPrepare(u64, u64),// 偷取未初始化的任务,线程id，窃取数量
    Close,
}

impl std::fmt::Debug for TaskMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AddFuture(futures) => f.debug_struct("AddFuture").field("count", &futures.len()).finish(),
            Self::PushWakeId(id) => f.debug_tuple("PushWakeId").field(id).finish(),
            Self::StealPrepare(id, count) => f.debug_tuple("StealPrepare").field(id).field(count).finish(),
            Self::Close => write!(f, "Close"),
        }
    }
}


#[cfg(test)]
mod tests{
    use super::*;
    use log::{info,LevelFilter};
        fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("Aura::DispatchCenter::multi::scheduler", LevelFilter::Trace) 
            .filter_module("mio", LevelFilter::Warn)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
    #[test]
    fn test_runtime(){
        setup_logger(LevelFilter::Trace);
        Scheduler::new(4);
        let future1 = async{
            info!("future1");
        };
        let future2 = async{
            info!("future2");
        };
        let future3 = async{
            info!("future3");
        };
        let future4 = async{
            info!("future4");
        };
        Scheduler::add(vec![Box::pin(future1), Box::pin(future2), Box::pin(future3), Box::pin(future4)]);
        Scheduler::join();
    }

    #[test]
    fn test_spawn(){
        setup_logger(LevelFilter::Trace);
        Scheduler::new(2);
        let future = async{
            3+4
        };
        let future1=async{
            let n=Scheduler::spawn(future).await;
            assert!(n==7);
        };
        let future3=async{
            6+7
        };
        let future4=async{
            let n=Scheduler::spawn(future3).await;
            assert!(n==13);
        };
        let l:Vec<Pin<Box<dyn Future<Output = ()> + Send+ 'static>>> =vec![Box::pin(future1), Box::pin(future4)];
        Scheduler::add(l);
        Scheduler::join();
    }

    #[test]
    fn test_sleep(){
        use std::time::{Duration, Instant};
        setup_logger(LevelFilter::Trace);
        Scheduler::new(2);
        let future1 = async{
            let start_time = Instant::now();
            let sleep = Scheduler::sleep(Duration::from_millis(100));
            sleep.await;
            let end_time = Instant::now();
            assert!(end_time-start_time >= Duration::from_millis(100));
        };
        let future2 = async{
            let start_time = Instant::now();
            let sleep = Scheduler::sleep(Duration::from_millis(100));
            sleep.await;
            let end_time = Instant::now();
            assert!(end_time-start_time >= Duration::from_millis(100));
        };
        Scheduler::add(vec![Box::pin(future1), Box::pin(future2)]);
        Scheduler::join();
    }
}