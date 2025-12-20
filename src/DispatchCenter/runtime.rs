use std::{sync::{Arc, Mutex, Weak}, 
    task::{Waker, Context,Wake,Poll}, 
    future::Future,
    thread::{self, Thread},
    time::Duration,
    marker::Unpin,
    pin::Pin  
};
use super::centerpoll::CenterPoll;



type future<T>=Arc<Mutex<AuraFuture<T>>>;
//运行时线程模式
pub enum ThreadingModel<T: 'static> {
    SingleThread,
    MultiThread(Arc<Mutex<CenterPoll<T>>>),
}


//运行时
pub struct Aura<T: 'static>{
    pub waiting_future: Vec<future<T>>,
    pub ready_future: Vec<future<T>>,
    threading_model: ThreadingModel<T>,
    waker: Option<AuraWaker<T>>,
    start: bool,
}

impl<T: 'static> Aura<T>{
    pub fn new(threading_model: ThreadingModel<T>) -> Arc<Mutex<Self>> {
        let aura = Arc::new(Mutex::new(Self {
            waiting_future: Vec::new(),
            ready_future: Vec::new(),
            threading_model,
            waker: None,
            start: false,
        }));
        // 创建弱引用的唤醒器，使用新的结构体语法
        let waker = AuraWaker::new(Arc::downgrade(&aura));
        aura.lock().unwrap().waker = Some(waker);
        aura
    }

    //运行时主循环
    pub fn run(mut self) {
        if self.start {
            return;
        }
        self.start = true;
        if let Some(waker) = self.waker {
            let waker = Waker::from(Arc::new(waker));
            let mut cx = Context::from_waker(&waker);
            match self.threading_model {
                ThreadingModel::SingleThread => {
                    // 单线程模式 - 确保future不会在线程间传输
                    loop{
                        if !self.start&&self.waiting_future.is_empty()&&self.ready_future.is_empty() {
                            break;
                        }
                        if self.waiting_future.is_empty()&&self.ready_future.is_empty() {
                            thread::sleep(Duration::from_millis(100));
                            continue;
                        }else{
                            if !self.ready_future.is_empty() {
                                let future = self.ready_future.clone().pop().unwrap();
                                let l_future = future.lock().unwrap();
                                let mut inner_future = l_future.future.lock().unwrap();
                                match inner_future.as_mut().poll(&mut cx) {
                                    Poll::Pending => {
                                        self.waiting_future.push(future.clone());
                                        continue;
                                    },
                                    Poll::Ready(_) => {
                                        self.ready_future.push(future.clone());
                                        continue;
                                    },
                                }
                            }else{
                                let future = self.waiting_future.clone().pop().unwrap();
                                let l_future = future.lock().unwrap();
                                let mut inner_future = l_future.future.lock().unwrap();
                                match inner_future.as_mut().poll(&mut cx) {
                                    Poll::Pending => {
                                        self.waiting_future.push(future.clone());
                                        continue;
                                    },
                                    Poll::Ready(_) => {
                                        self.ready_future.push(future.clone());
                                        continue;
                                    },
                                }
                            }
                        }
                    }
                },
                ThreadingModel::MultiThread(center_poll) => {
                    let center_poll = center_poll;
                    loop{
                        if !self.start&&self.waiting_future.is_empty()&&self.ready_future.is_empty() {
                            break;
                        }
                        if center_poll.lock().unwrap().ready_future.lock().unwrap().is_empty() {
                            if center_poll.lock().unwrap().singel_runtime.lock().unwrap().is_empty() {
                                thread::sleep(Duration::from_millis(100));
                                continue;
                            }else{
                                let mut runlen = center_poll.lock().unwrap().singel_runtime.lock().unwrap().len();
                                for runtime in center_poll.lock().unwrap().singel_runtime.lock().unwrap().iter() {
                                    let mut runtime = runtime.lock().unwrap().waiting_future.clone();
                                    if !runtime.is_empty() {
                                        let getfuture = runtime.pop().unwrap();
                                        self.waiting_future.push(getfuture);
                                        break;
                                    }else{
                                        runlen -= 1;//没有future了，减少运行时数量
                                        continue;
                                    }
                                }
                                if runlen == 0 {//所有运行时都没有future了
                                    thread::sleep(Duration::from_millis(100));
                                     continue;
                                }
                                let future = self.waiting_future.pop().unwrap();
                                let mut future_guard = future.lock().unwrap();
                                let mut future_pin = Pin::new(&mut *future_guard);
                                match future_pin.as_mut().poll(&mut cx) {
                                    Poll::Pending => {
                                        self.waiting_future.push(future.clone());
                                        continue;
                                    },
                                    Poll::Ready(_) => {
                                        self.ready_future.push(future.clone());
                                        continue;
                                    },
                                }
                            }
                        }else{
                            let future = center_poll.lock().unwrap().ready_future.lock().unwrap().pop().unwrap();
                            let mut future_guard = future.lock().unwrap();
                            let mut future_pin = Pin::new(&mut *future_guard);
                            match future_pin.as_mut().poll(&mut cx) {
                                Poll::Pending => {
                                    self.waiting_future.push(future.clone());
                                    continue;
                                },
                                Poll::Ready(_) => {
                                    self.ready_future.push(future.clone());
                                    continue;
                                },
                            }

                        }
                    }
                }
            }
        }
    }

    pub fn sqawn(&mut self, future: future<T>) {
        match &self.threading_model {
            ThreadingModel::SingleThread => {
                self.waiting_future.push(future);
            },
            ThreadingModel::MultiThread(center_poll) => {
                center_poll.lock().unwrap().add_allfuture(future);
            }
        }
    }

    pub fn sqawn_async(&mut self, futures: Vec<future<T>>) {
        futures.iter().for_each(|future| {
            self.waiting_future.push(future.clone());
        });
    }

    pub fn stop(&mut self) {
        self.start = false;
        self.waiting_future.clear();
    }
}

//唤醒器 - 针对单线程和多线程使用不同的实现
struct AuraWaker<T: 'static> {
    future: Option<future<T>>,
    runtime: Weak<Mutex<Aura<T>>>,
}

impl<T: 'static> AuraWaker<T> {
    pub fn new(runtime: Weak<Mutex<Aura<T>>>) -> Self {
        Self {
            future: None,
            runtime,
        }
    }
}

impl<T: 'static> Wake for AuraWaker<T> {
    fn wake(self: Arc<Self>) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.lock().unwrap().ready_future.push(self.future.clone().unwrap());  
        }
    }
}

//伪IO任务future
pub struct AuraFuture<T: 'static> {
    future: Arc<Mutex<Pin<Box<dyn Future<Output = T> + Send + Unpin>>>>,
    thread_id: usize,
    aurawaker: Arc<Mutex<Option<AuraWaker<T>>>>,
}

//伪IO任务future类型

impl<T: 'static> AuraFuture<T> {
    fn new(future: Box<dyn Future<Output = T> + Send + Unpin>, thread_id: usize, aurawaker: Arc<Mutex<AuraWaker<T>>>) -> Self {
        // 创建aurawaker的克隆并设置到字段中
        let waker_clone = aurawaker.lock().unwrap().clone();
        Self {
            future: Arc::new(Mutex::new(Box::pin(future))),
            thread_id,
            aurawaker: Arc::new(Mutex::new(Some(waker_clone))),
        }
    }
}

impl <T: 'static>Clone for Aura<T> {
    fn clone(&self) -> Self {
        Self {
            waiting_future: self.waiting_future.clone(),
            ready_future: self.ready_future.clone(),
            threading_model: self.threading_model.clone(),
            waker: self.waker.clone(),
            start: self.start
        }
    }
}

impl <T: 'static>Clone for ThreadingModel<T> {
    fn clone(&self) -> Self {
        match self {
            ThreadingModel::SingleThread => ThreadingModel::SingleThread,
            ThreadingModel::MultiThread(center_poll) => ThreadingModel::MultiThread(center_poll.clone()),
        }
    }
}

impl<T: 'static> Clone for AuraFuture<T> {
    fn clone(&self) -> Self {
        Self {
            future: self.future.clone(),
            thread_id: self.thread_id,
            aurawaker: self.aurawaker.clone(),
        }
    }
}

// 为 AuraFuture<T> 实现 Future trait
impl<T: 'static> Future for AuraFuture<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 获取内部 future 的可变引用并调用其 poll 方法
        let mut inner_future = self.future.lock().unwrap();
        inner_future.as_mut().poll(cx)
    }
}


impl <T: 'static>Clone for AuraWaker<T> {
    fn clone(&self) -> Self {
        Self {
            future: None,
            runtime: self.runtime.clone(),
        }
    }
}
