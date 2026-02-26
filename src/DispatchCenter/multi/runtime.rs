use std::{
    thread::{sleep,self},
    sync::{Arc, Condvar},
    collections::{HashMap,VecDeque},
    future::Future,
    pin::Pin,
    cell::{UnsafeCell, RefMut,RefCell},
    rc::{Rc, Weak},
    task::{Context, Poll, Waker,Wake},
    time:: Duration,
};
use log::{info, error};
use crate::DispatchCenter::multi::{task::Task, scheduler::*,net::evenloop::EventLoop};
use crate::GeneralComponents::{id_bulider,static_exchange,time::time_wheel};


thread_local!{
    static CURRENT_RUNTIME: RefCell<Option<Rc<Runtime>>> = RefCell::new(None);
}



#[derive(Clone, Copy, Debug)]
pub enum Status{
    Running,
    Idle,
    Closing,
    Closed,
}

// 运行时结构体
// 用于线程级别的任务管理
pub struct Runtime{
    id: u64,
    prepare_tasks: RefCell<VecDeque<Pin<Box<dyn Future<Output = ()> + Send>>>>,
    watting_tasks: RefCell<VecDeque<u64>>,
    ready_tasks: RefCell<VecDeque<u64>>,
    tasks: RefCell<HashMap<u64, Task>>,
    id_bulider: RefCell<id_bulider::IdBulider>,
    run: RefCell<Status>,
    finish_tasks: RefCell<Option<u64>>,
    time_wheel: Rc<RefCell<time_wheel::TimeWheel>>,
}

impl Runtime{
    pub fn new(id: u64)->Rc<Self>{
        CURRENT_RUNTIME.with(|cell|{
            let mut runtime=cell.borrow_mut();
            if runtime.is_none(){
                runtime.replace(Rc::new(Self{
                    id,
                    prepare_tasks: RefCell::new(VecDeque::new()),
                    watting_tasks: RefCell::new(VecDeque::new()),
                    ready_tasks: RefCell::new(VecDeque::new()),
                    tasks: RefCell::new(HashMap::new()),
                    id_bulider: RefCell::new(id_bulider::IdBulider::new()),
                    run: RefCell::new(Status::Running),
                    finish_tasks: RefCell::new(None),
                    time_wheel: Rc::new(RefCell::new(time_wheel::TimeWheel::new(Duration::from_micros(100)))),
                }));
            }
        });
        #[cfg(not(miri))]
        {
            EventLoop::new();
        }
        Rc::clone(&CURRENT_RUNTIME.with(|cell| cell.borrow().as_ref().unwrap().clone()))
    }

    pub fn add_future(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>){
        self.prepare_tasks.borrow_mut().push_back(future);
    }

    // 获取运行时状态
    pub fn is_running(&self) -> Status{
       *self.run.borrow()
    }

    // 获取运行时任务数量
    pub fn get_task_len(&self) -> usize{
        self.tasks.borrow().len()
    }

    // 获取当前线程运行时
    pub fn get_runtime()->Rc<Self>{
        CURRENT_RUNTIME.with(|cell| cell.borrow().as_ref().unwrap().clone())
    }

    pub fn get_id(&self) -> u64{
        self.id
    }

    // 设置时间轮步长时间
    pub fn set_slot_duration(&mut self,slot_duration: Duration){
        self.time_wheel.borrow_mut().set_slot_duration(slot_duration);
    }

    // 获取时间轮
    pub fn get_time_wheel() -> Rc<RefCell<time_wheel::TimeWheel>>{
        Runtime::get_runtime().time_wheel.clone()
    }

    pub fn change_status(&self, status: Status){
        *self.run.borrow_mut() = status;
        let exchange_list = static_exchange::EXCHANGE_LIST.get().unwrap();
        let mut status_lock = exchange_list[self.id as usize].1.write().unwrap();
        *status_lock = Some(status);

    }

    // 关闭运行时
    fn close(& self){
        self.tasks.borrow_mut().clear();
        self.watting_tasks.borrow_mut().clear();
        self.ready_tasks.borrow_mut().clear();
        self.prepare_tasks.borrow_mut().clear();
        *self.run.borrow_mut() = Status::Closed;
           match &static_exchange::get_list(self.id as usize).unwrap().1.push(BackMsg::Closed){
                Ok(_) => {},
                Err(e) => error!("运行时 {} 信息交换列表推送错误: {:?}", self.id, e),
           }
    }

    // 运行时轮询
    pub fn run(& self){
        if self.ready_tasks.borrow().is_empty()&&self.prepare_tasks.borrow().is_empty(){
            let msgs:Result<Option<Vec<TaskMsg>>, String> = match self.is_running() {
                Status::Running => {
                        static_exchange::get_list(self.id as usize).unwrap().0.pop_all()
                },
                Status::Idle => {
                        static_exchange::get_list(self.id as usize).unwrap().0.pop_all()
                }
                Status::Closing => {
                    Err("运行正在关闭，无法接收任务".to_string())
                }
                Status::Closed => {
                    Err("运行时已关闭，无法接收任务".to_string())
                }
            };
            if let Status::Closing = self.is_running() && !self.prepare_tasks.borrow_mut().is_empty(){
                self.prepare_tasks.borrow_mut().clear();
            }
            match msgs{
                Ok(Some(msgs)) => { 
                    for msg in msgs{
                        match msg{
                            TaskMsg::AddFuture(futures) => {
                                if let Status::Running = self.is_running(){
                                }else{
                                    self.change_status(Status::Running);
                                }
                                for future in futures{
                                    self.add_future(future);
                                }
                            },
                            TaskMsg::PushWakeId(id) => {
                                if let Status::Running = self.is_running(){
                                }else{
                                    self.change_status(Status::Running);
                                }
                                if self.tasks.borrow().contains_key(&id) && !self.ready_tasks.borrow().contains(&id){
                                    self.ready_tasks.borrow_mut().push_back(id);
                                }
                            },
                            TaskMsg::StealPrepare(id, count) => {
                                if let Status::Running = self.is_running(){
                                    if self.prepare_tasks.borrow_mut().len() >= count as usize{
                                        let exchange = static_exchange::get_list(id as usize).unwrap();
                                        unsafe{
                                            let len=self.prepare_tasks.borrow_mut().len();
                                            match (*exchange).0.push(TaskMsg::AddFuture(self.prepare_tasks.borrow_mut().drain(len-count as usize..).collect())){
                                                Ok(_) => {},
                                                Err(e) => error!("运行时 {} 信息交换列表推送错误: {:?}", id, e),
                                            }
                                        }
                                    }
                                }
                             }
                            TaskMsg::Close => {
                                self.change_status(Status::Closing);
                                self.close();
                            },
                        };
                    }
                },
                Ok(None) => {
                    info!("运行时 {} 任务交换列表为空", self.id);
                },
                Err(msg) => {
                    error!("运行时 {} {}", self.id, msg);
                }
            };
        }
        match self.is_running(){
            Status::Running => {
                if self.prepare_tasks.borrow_mut().is_empty() && self.ready_tasks.borrow().is_empty(){
                    thread::sleep(Duration::from_millis(1));
                }
                if !self.prepare_tasks.borrow_mut().is_empty(){
                    let future = self.prepare_tasks.borrow_mut().pop_front().unwrap();
                    let id = self.id_bulider.borrow_mut().get_id();
                    let task = Task::new(id, future);
                    self.tasks.borrow_mut().insert(id, task);
                    self.ready_tasks.borrow_mut().push_back(id);
                }
                if !self.ready_tasks.borrow().is_empty(){
                    let mut drop_id=Vec::new();
                    let list =self.ready_tasks.borrow_mut().drain(..).collect::<Vec<_>>();
                    for id in list{
                        let mut tasks = self.tasks.borrow_mut();
                        let task = tasks.get_mut(&id).unwrap();
                        let future = task.future();
                        let muxwaker=MutexWaker::new(id, self.id);
                        let waker = Arc::new(muxwaker);
                        let waker = waker.clone();
                        let waker = Waker::from(waker);
                        let mut cx=Context::from_waker(&waker);
                        match future.borrow_mut().as_mut(){
                            Some(future) => {
                                match future.as_mut().poll(&mut cx){
                                    Poll::Ready(_) => {
                                        let count_l = *self.finish_tasks.borrow();
                                        match count_l{
                                            Some(count) => {
                                                self.finish_tasks.borrow_mut().replace(count+1);
                                            },
                                            None => *self.finish_tasks.borrow_mut()= Some(1),
                                        }
                                        drop_id.push(id);
                                    },
                                    Poll::Pending => {
                                        self.watting_tasks.borrow_mut().push_back(id);
                                        task.set_waker(waker.clone());
                                    },
                                }
                        },
                            None => {},
                        };
                    }
                    for id in drop_id{
                        self.tasks.borrow_mut().remove(&id);
                    }
                }
                let mut time_wheel = self.time_wheel.borrow_mut();
                if !time_wheel.is_empty(){
                    time_wheel.poll();
                }
                // 处理网络事件
                #[cfg(not(miri))]
                if !EventLoop::get().is_empty(){
                    let _ = EventLoop::get().poll();
                }
                TREAD_LOAD.get().unwrap().lock().unwrap().insert(self.id, self.get_task_len());
                }
            _ => {},
        }
        if self.prepare_tasks.borrow_mut().is_empty() && self.ready_tasks.borrow().is_empty() && self.tasks.borrow().is_empty(){
            if let Status::Running = self.is_running(){
                let finish_count = *self.finish_tasks.borrow();
                if let Some(count) = finish_count{
                    match (*static_exchange::get_list(self.id as usize).unwrap()).1.push(BackMsg::AllTaskFinished(count)){
                            Ok(_) => {},
                            Err(e) => {
                                error!("{}", e);
                            }
                        }
                        let load=TREAD_LOAD.get().unwrap().lock().unwrap();
                        let max_load=load.values().max().unwrap();
                        for (id, count) in load.iter(){
                            if *count == *max_load{
                                let exchange = static_exchange::get_list(*id as usize).unwrap();
                                    match (*exchange).0.push(TaskMsg::StealPrepare(self.id, (*count as u64)/2)){
                                        Ok(_) => {},
                                        Err(e) => error!("运行时 {} 信息交换列表推送错误: {:?}", *id, e),
                                }
                            }
                        }
                        let (state,condvar) = &SCHEDULER;
                        let b=!*state.lock().unwrap();
                        if b{
                            *state.lock().unwrap()=true;
                            condvar.notify_one();
                        }
                    self.change_status(Status::Idle);
                }
            }
        }
    }
}


 pub struct MutexWaker{
    taskid: u64,
    threadid: u64,
}

impl MutexWaker{
    pub fn new(taskid: u64,threadid: u64) -> Self{
        Self{
            taskid,
            threadid,
        }
    }
}

impl Wake for MutexWaker{
    fn wake(self: Arc<Self>){
        let mut runtime = Runtime::get_runtime();
        if self.threadid == runtime.id{
            runtime.ready_tasks.borrow_mut().push_back(self.taskid);
        }else{
            match (*static_exchange::get_list(self.threadid as usize).unwrap()).0.push(TaskMsg::PushWakeId(self.taskid)){
                    Ok(_) => {
                        let (state,condvar) = &SCHEDULER;
                        let b=!*state.lock().unwrap();
                        if b{
                            *state.lock().unwrap()=true;
                            condvar.notify_one();
                        }
                    },
                    Err(e) => {
                        error!("{}", e);
                    }
            };

        }
    }
}
