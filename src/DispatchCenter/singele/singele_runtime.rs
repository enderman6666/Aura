use std::{
    cell::RefCell, 
    collections::{HashMap, VecDeque}, 
    future::{self, Future}, 
    pin::Pin, rc::{Rc,Weak}, 
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    time::Duration as StdDuration,
    thread::sleep,
    net::SocketAddr,
    io::{self,ErrorKind},
};
use chrono::Duration;
use log::{info,debug,trace,error,warn};
use super::{
    task::{Task,Output},
    join_handle::JoinHandle,
    time::*,
};
use mio::net::TcpStream;
use crate::{DispatchCenter::singele::singele_runtime, GeneralComponents::time::time_wheel::TimeWheel};    
use super::net::{tcp::{TcpServer,TcpConnet,TcpConnectFuture},udp::UdpHandeler,evenloop::EventLoop,stream};
use super::file::{window::evenloop::WinFileCenter,FileCenter,window::winfile::WinFile};

thread_local!{
    static SINGLE_RUNTIME: RefCell<Option<Rc<SingeleRuntime>>> = RefCell::new(None);
}

// 单线程运行时,用于管理异步任务
pub struct SingeleRuntime{
    prepare_tasks:Rc<RefCell<Vec<Pin<Box<dyn Future<Output = ()>>>>>>,// 准备任务id队列
    start_tasks:Rc<RefCell<Vec<i64>>>,// 启动任务id队列
    waitting_futures:Rc<RefCell<VecDeque<i64>>>,// 等待任务id队列
    ready_futures:Rc<RefCell<VecDeque<i64>>>,// 就绪任务id队列
    clear_tasks:RefCell<Vec<i64>>,// 清除任务id队列
    tasks:Rc<RefCell<HashMap<i64, Rc<RefCell<Task>>>>>,// 任务id到任务的映射
    handle_list:(Rc<RefCell<TimeWheel>>,Rc<RefCell<EventLoop>>,Rc<RefCell<dyn FileCenter>>),// 监听器元组
}
impl SingeleRuntime{
    fn new() -> Self{
        #[cfg(target_os = "windows")]
        {let file_center = Rc::new(RefCell::new(WinFileCenter::new()));
        Self{
            prepare_tasks: Rc::new(RefCell::new(Vec::new())),// 准备生成任务队列
            start_tasks: Rc::new(RefCell::new(Vec::new())),// 启动任务id队列
            waitting_futures: Rc::new(RefCell::new(VecDeque::new())),// 等待任务id队列
            ready_futures: Rc::new(RefCell::new(VecDeque::new())),// 就绪任务id队列
            clear_tasks: RefCell::new(Vec::new()),// 清除任务id队列
            tasks: Rc::new(RefCell::new(HashMap::new())),// 任务id到任务的映射
            handle_list: (
                Rc::new(RefCell::new(TimeWheel::new(StdDuration::from_micros(100)))),
                Rc::new(RefCell::new(EventLoop::new())),
                file_center,
            ),// 监听器元组
        }}
    }

    // 获取当前线程的运行时实例
    pub fn get_runtime() -> Rc<SingeleRuntime>{
        SINGLE_RUNTIME.with(|runtime|{
            let mut runtime_b = runtime.borrow_mut();
            if runtime_b.is_none(){
                runtime_b.replace(Rc::new(SingeleRuntime::new()));
            }
        });
        let runtime = SINGLE_RUNTIME.with(|runtime|{
            runtime.borrow_mut().as_ref().unwrap().clone()
        });
        runtime
    }

    // 运行异步任务，可直接使用
    pub fn run(future: impl Future<Output = ()>+ 'static){
        debug!("开始初始化SingeleRuntime");
        let runtime = Self::get_runtime();
        debug!("SingeleRuntime初始化完成");
        debug!("添加初始任务到prepare_tasks队列");
        runtime.prepare_tasks.borrow_mut().push(Box::pin(future));
        for task in runtime.prepare_tasks.borrow_mut().drain(..){
            let task_id = runtime.task(task);
            runtime.start_tasks.borrow_mut().push(task_id);
        }
        debug!("任务包装完成，开始循环处理任务");
        let mut cont_times=0;
        loop{
            cont_times+=1;
            let tasks_count = runtime.tasks.borrow().len();
            let tasks_empty = tasks_count == 0;
            if tasks_empty{
                debug!("tasks映射为空，退出循环");
                break;
            }
            // 处理新添加的任务（来自spawn方法）
            if !runtime.prepare_tasks.borrow().is_empty(){
                debug!("处理prepare_tasks队列中的任务");
                for task in runtime.prepare_tasks.borrow_mut().drain(..){
                    let task_id = runtime.task(task);
                    runtime.start_tasks.borrow_mut().push(task_id);
                    debug!("创建新任务 {} 并添加到start_tasks队列", task_id);
                }
            }
            let start_tasks_empty = runtime.start_tasks.borrow().is_empty();
            if !start_tasks_empty{
                //初始化任务waker并poll任务
                debug!("处理start_tasks队列中的任务");
                let _ =runtime.start_tasks.borrow_mut().drain(..).map(
                    |task_id|{
                        let task_id_val = task_id;
                        log::debug!("处理任务: {}", task_id_val);
                        let waker = SingelWaker::new(Rc::downgrade(&runtime), task_id_val);
                        let waker = waker.create_raw_waker();
                        let mut cx = Context::from_waker(&waker);
                        let taskmap = runtime.tasks.borrow();
                        let task = taskmap.get(&task_id_val).unwrap().borrow();
                        debug!("准备poll任务: {}", task_id_val);
                        match task.future().borrow_mut().as_mut().unwrap().as_mut().poll(&mut cx){
                            Poll::Ready(_) => {
                                debug!("任务 {} 执行完成", task_id_val);
                                runtime.clear_tasks.borrow_mut().push(task_id_val);
                                debug!("将任务 {} 添加到clear_tasks队列", task_id_val);
                            }
                            Poll::Pending => {
                                debug!("任务 {} 未完成", task_id_val);
                                // 存储任务waker
                                *task.waker.borrow_mut() = Some(waker);
                                // 将任务添加到等待队列
                                runtime.waitting_futures.borrow_mut().push_back(task_id_val);
                                debug!("存储任务 {} 的waker并添加到等待队列", task_id_val);
                            }
                        }
                        debug!("完成处理任务: {}", task_id_val);
                    }
                ).collect::<Vec<_>>();
                debug!("start_tasks队列已通过drain清空");
                debug!("继续执行后续代码");
            }
            debug!("检查就绪任务队列");
            // 处理就绪任务
            let ready_tasks: Vec<i64> = runtime.ready_futures.borrow_mut().drain(..).collect();
            for task_id in ready_tasks {
                debug!("处理就绪任务: {}", task_id);
                let taskmap = runtime.tasks.borrow();
                let task = taskmap.get(&task_id).unwrap().borrow_mut();
                let waker = task.waker.borrow_mut().take();
                let waker = if let Some(waker) = waker.as_ref(){
                    waker
                }else{
                    continue;
                };
                match task.future().borrow_mut().as_mut().unwrap().as_mut().poll(&mut Context::from_waker(&waker)){
                    Poll::Ready(_) => {
                        runtime.clear_tasks.borrow_mut().push(task_id);
                    }
                    Poll::Pending => {
                        *task.waker.borrow_mut() = Some(waker.clone());
                        // 将任务添加回等待队列
                        runtime.waitting_futures.borrow_mut().push_back(task_id);
                    }
                }
            }
            let mut timewheel = runtime.handle_list.0.borrow_mut();
            if !timewheel.is_empty(){
                timewheel.poll();
            }
            let mut single_center = runtime.handle_list.1.borrow_mut();
            let _ = single_center.poll();
            let mut file_center = runtime.handle_list.2.borrow_mut();
            file_center.poll();
            debug!("等待列队任务数量: {}", runtime.waitting_futures.borrow().len());
            debug!("就绪任务队列任务数量: {}", runtime.ready_futures.borrow().len());
            // 清除就绪任务
            let clear_tasks_count = runtime.clear_tasks.borrow().len();
            if clear_tasks_count > 0{
                debug!("开始清理任务，clear_tasks队列中有 {} 个任务", clear_tasks_count);
                // 处理clear_tasks队列中的所有任务
                while let Some(task_id) = runtime.clear_tasks.borrow_mut().pop(){
                    debug!("移除任务: {}", task_id);
                    runtime.remove_task(task_id);
                }
                debug!("clear_tasks队列已清空,共计循环{}次",cont_times);
            }
            if runtime.ready_futures.borrow().is_empty()&&runtime.start_tasks.borrow().is_empty(){
                sleep(StdDuration::from_micros(100));
                debug!("等待队列已空,共计循环{}次",cont_times);
                
            }
        }
    }

    // 创建新任务,并添加到任务映射中
    fn task(&self, future: impl Future<Output = ()>+ 'static)->i64{
        debug!("创建新任务");
        let task = Rc::new(RefCell::new(Task::new(Box::pin(future))));
        debug!("新任务 {} 创建完成", task.borrow().id);
        let task_id = task.borrow().id;
        self.tasks.borrow_mut().entry(task_id).or_insert(Rc::clone(&task));
        debug!("新任务 {} 已插入至runtime.tasks", task_id);
        task_id
    }

    // 创建新任务并添加到等待队列
    pub fn spawn<T:'static>(future: impl Future<Output = T>+ 'static)->JoinHandle<T>{
        let runtime = Self::get_runtime();
        let output = Rc::new(RefCell::new(Output::new()));
        let output_clone = Rc::clone(&output);
        let result=async move{
            let value = future.await;
            *output_clone.borrow_mut().value.borrow_mut() = Some(value);
            // 任务完成，唤醒Output的waker
            for waker in output_clone.borrow_mut().wakers.borrow_mut().drain(..) {
                waker.wake();
            }
        };
        runtime.prepare_tasks.borrow_mut().push(Box::pin(result));
        JoinHandle::new(output)
    }

    // 批量运行所有任务
    pub fn run_all(futures: Vec<Pin<Box<dyn Future<Output = ()>+ 'static>>>){
        let runtime = Self::get_runtime();
        for future in futures{
            runtime.prepare_tasks.borrow_mut().push(future);
        }
        SingeleRuntime::run(
            async{}
        );
    }

    // 等待指定时间
    pub fn sleep(duration: StdDuration) -> Sleep{
        let runtime = Self::get_runtime();
        let timewheel = Rc::clone(&runtime.handle_list.0);
        Sleep::new(duration, timewheel)
    }

    // 两个future同时执行,返回先完成的结果
    pub fn race<L,R>(left: L, right: R) -> Race<L, R>
    where
        L: Future,
        R: Future,
    {
        Race::new(left, right)
    }

    fn remove_task(&self,task_id:i64){
        self.tasks.borrow_mut().remove(&task_id);
        self.start_tasks.borrow_mut().retain(|id| *id != task_id);
        self.ready_futures.borrow_mut().retain(|id| *id != task_id);
        self.waitting_futures.borrow_mut().retain(|id| *id != task_id);
    }

    // 创建一个TCP监听器
    pub fn TcpListener(ip: SocketAddr) -> Result<TcpServer, io::Error>{
        let eventloop = Rc::clone(&SingeleRuntime::get_runtime().handle_list.1);
        TcpServer::new(ip, eventloop)
    }

    // 创建一个UDP套接字
    pub fn UdpSocket(ip: SocketAddr) -> Result<UdpHandeler, io::Error>{
        let eventloop = Rc::clone(&SingeleRuntime::get_runtime().handle_list.1);
        UdpHandeler::new(ip, eventloop)
    }

    // 创建一个主动连接的TCP连接
    pub async fn TcpStream(addr:SocketAddr) -> Result<TcpConnet,std::io::Error>{
        let eventloop = Rc::clone(&SingeleRuntime::get_runtime().handle_list.1);
        TcpConnet::build(addr,eventloop).await
    }

    // 从Tcp监听器返回一个TCP连接创建
    pub fn TcpConnet(socket: Rc<RefCell<TcpStream>>) -> TcpConnet{
        let eventloop = Rc::clone(&SingeleRuntime::get_runtime().handle_list.1);
        TcpConnet::new(socket, eventloop)
    }

    pub fn file(path:&str)->windows::core::Result<WinFile>{
        let singele_runtime=SingeleRuntime::get_runtime();
        let file_center = Rc::clone(&singele_runtime.handle_list.2);
        WinFile::new(file_center,path)
    }

}

// 自定义Waker，用于单线程环境下的任务唤醒
pub struct SingelWaker {
    runtime: Weak<SingeleRuntime>,
    task_id:i64,
}

impl SingelWaker{
    fn new(runtime: Weak<SingeleRuntime>,task_id:i64) -> Self{
        Self{
            runtime,
            task_id,
        }
    }

    pub fn create_raw_waker(&self) -> Waker{
        // 复制当前waker实例到堆上的Box中
        let waker_box = Rc::new(RefCell::new(Self{
            runtime: self.runtime.clone(),
            task_id: self.task_id,
        }));
        let raw_waker = RawWaker::new(Rc::into_raw(waker_box) as *mut _, &COUNTER_VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}

// 定义一个辅助的非泛型函数，用于处理类型擦除
unsafe fn wake_internal(waker: &SingelWaker) {
    if let Some(runtime) = waker.runtime.upgrade() {
        let task_id = waker.task_id;
        // 同时获取两个可变借用，避免重入问题
        let mut ready_futures = runtime.ready_futures.borrow_mut();
        let mut waitting_futures = runtime.waitting_futures.borrow_mut();
        
        if waitting_futures.iter().any(|id| *id == task_id){
            ready_futures.push_back(task_id);
            waitting_futures.retain(|id| *id != task_id);
        }else{
            if !ready_futures.iter().any(|id| *id == task_id){
            log::error!("任务{}未在等待队列和就绪列队中，无法唤醒",waker.task_id);
            }
        }
    }
}

unsafe fn drop_waker_internal(ptr: *const ()) {
    unsafe {
        // 将原始指针转换回Rc<RefCell<SingelWaker>>，然后自动释放
        let _ = Rc::from_raw(ptr as *mut RefCell<SingelWaker>);
    }
}

static COUNTER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    // clone函数：增加Rc引用计数
    |ptr| unsafe { 
        let waker_rc = Rc::from_raw(ptr as *mut RefCell<SingelWaker>);
        let clone_rc = Rc::clone(&waker_rc);
        // 将原始Rc转换回指针（因为from_raw会获取所有权）
        let _ = Rc::into_raw(waker_rc);
        RawWaker::new(Rc::into_raw(clone_rc) as *mut _, &COUNTER_VTABLE) 
    },
    // wake函数：获取Rc所有权并唤醒任务
    |ptr| unsafe {
        let waker_rc = Rc::from_raw(ptr as *mut RefCell<SingelWaker>);
        let waker_ref = &*waker_rc.borrow();
        wake_internal(waker_ref);
        // Rc会在这里自动释放（如果是最后一个引用）
    },
    // wake_by_ref函数：不获取所有权，仅唤醒任务
    |ptr| unsafe {
        let waker_ref_cell = &*(ptr as *const RefCell<SingelWaker>);
        let waker_ref = &*waker_ref_cell.borrow();
        wake_internal(waker_ref);
    },
    // drop函数：释放Rc引用
    |ptr| unsafe {
        drop_waker_internal(ptr);
    },
);

#[cfg(test)]
mod tests{
    use super::SingeleRuntime;
    mod test_singele_runtime{
        use super::*;
        use std::{
            pin::Pin,
            sync::Once,
            time::Duration,
        };
        use log::{debug,LevelFilter};
        use env_logger;

        fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level) // 主过滤级别
            .filter_module("aura", LevelFilter::Trace) // 你的crate用最详细级别
            .filter_module("mio", LevelFilter::Warn) // 第三方库只显示警告
            .is_test(true)
            .format_timestamp_nanos() // 高精度时间戳
            .try_init()
            .ok(); // 忽略初始化失败，避免在多次调用时panic
        }

        #[test]
        fn test_singele_runtime(){
            setup_logger(LevelFilter::Trace);
            debug!("测试开始");
            SingeleRuntime::run(
                async {
                    debug!("异步代码开始执行");
                    let a = 1;
                    let b = 2;
                    debug!("a = {}, b = {}, a+b = {}", a, b, a+b);
                    let result = a + b;
                    assert_eq!(result, 3, "断言失败: {} != 3", result);
                    debug!("异步代码执行完成");
                }
            );
            debug!("测试结束");
        }
        #[test]
        fn test_singele_runtime_spawn(){
            setup_logger(LevelFilter::Trace);
            SingeleRuntime::run(
                async {
                    let a = 1;
                    let b = SingeleRuntime::spawn(async {
                        2
                    }).await;
                    assert_eq!(a+b,3,"spawn方法返回的结果不正确");
                    debug!("spawn测试成功");
                }
            )
        }
        #[test]
        fn test_singele_runtime_runall(){
            setup_logger(LevelFilter::Trace);
            let futures:Vec<Pin<Box<dyn Future<Output=()>>>>=vec![
                Box::pin(async{
                    let a = 1;
                    assert_eq!(2*a,2,"2!=2");
                }),
                Box::pin(async{
                    let a =2;
                    assert_eq!(2*a,4,"4!=4");
                }),
                Box::pin(async{
                    let a =3;
                    assert_eq!(3*a,9,"9!=9");
                }),
                Box::pin(async{
                    let a =4;
                    assert_eq!(4*a,16,"16!=16");
                }),
                Box::pin(async{
                    let a =5;
                    assert_eq!(5*a,25,"25!=25");
                }),
            ];
            SingeleRuntime::run_all(futures);
        }

        #[test]
        fn test_singele_runtime_tcp_listener(){
            setup_logger(LevelFilter::Trace);
            let ip = "127.0.0.1:8080".parse().unwrap();
            let mut listener = SingeleRuntime::TcpListener(ip).unwrap();
            SingeleRuntime::run(async move{
                use futures::StreamExt;
                while let Some(stream) = listener.listen().next().await{
                    debug!("监听到一个连接，内容为{:?}",stream);
                    break; // 只处理一个连接就退出测试
                }
            });
        }

        #[test]
        fn test_singele_runtime_tcp_stream(){
            setup_logger(LevelFilter::Trace);
            let string = "hello world".to_string();
            let string_bytes = string.as_bytes().to_vec();
            let ip = "112.126.94.134:6666".parse().unwrap();
            SingeleRuntime::run(async move{
                use futures::StreamExt;
                let stream = if let Ok(stream) = SingeleRuntime::TcpStream(ip).await{
                    stream
                } else {
                    panic!("连接失败");
                };
                let mut write_stream = stream.write(string_bytes);
                while let Some(result) = write_stream.next().await {
                    match result {
                        Ok(n) => debug!("写入成功，写入了 {} 字节", n),
                        Err(e) => panic!("写入失败: {:?}", e),
                    }
                }
                // 写入完成
                debug!("所有数据写入完成");
            });
        }

        #[test]
        fn test_singele_runtime_udp_send(){
            setup_logger(LevelFilter::Trace);
            let ip = "0.0.0.0:8080".parse().unwrap();
            let sip = "112.126.94.134:6666".parse().unwrap();
            let string = "hello world".to_string();
            let string_bytes = string.as_bytes().to_vec();
            SingeleRuntime::run(async move{
                use futures::StreamExt;
                let udp = SingeleRuntime::UdpSocket(ip).unwrap();
                let mut write_stream = udp.write_to(sip,string_bytes);
                while let Some(result) = write_stream.next().await {
                    match result {
                        Ok(n) => debug!("写入成功，写入了 {} 字节", n),
                        Err(e) => panic!("写入失败: {:?}", e),
                    }
                }
                // 写入完成
                debug!("所有数据写入完成");
            });
        }

        #[test]
        fn test_singele_runtime_udp_recv(){
            setup_logger(LevelFilter::Trace);
            let ip = "0.0.0.0:8080".parse().unwrap();
            let udp = SingeleRuntime::UdpSocket(ip).unwrap();
            SingeleRuntime::run(async move{
                use futures::StreamExt;
                while let Some(buf) = udp.read().next().await{
                    debug!("监听到一个连接，内容为{:?}",buf);
                    break; // 只处理一个连接就退出测试
                }
            });
        }
    }
}