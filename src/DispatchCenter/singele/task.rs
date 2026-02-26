use std::{
    future::Future,
    pin::Pin,
    rc::{Rc,Weak},
    cell::RefCell,
    task::Waker,
};
use log::{debug};
use super::join_handle::JoinHandle;

thread_local!{
    static TASKID: RefCell<i64> = RefCell::new(0);
}

// 任务结构体,用于存储异步任务的信息
pub struct Task{
    pub id: i64,
    future:Rc<RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>>,
    pub waker: Rc<RefCell<Option<Waker>>>,
}

impl Task{
    pub fn new(future: Pin<Box<dyn Future<Output = ()>>>) -> Self{
        debug!("开始创建任务");
        Self{
            id: TASKID.with(|id| {
                let mut id = id.borrow_mut();
                *id += 1;
                debug!("id创建完成");
                *id
            }),
            future: Rc::new(RefCell::new(Some(future))),
            waker: Rc::new(RefCell::new(None)),
        }
    }


    pub fn future(&self) -> Rc<RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>>{
        Rc::clone(&self.future)
    }

    pub fn clear_future(&mut self){
        *self.future.borrow_mut() = None;
        *self.waker.borrow_mut() = None;
    }
}


pub struct Output<T>{
    pub value:Rc<RefCell<Option<T>>>,
    pub wakers:RefCell<Vec<Waker>>,
}

impl<T> Output<T>{
    pub fn new() -> Self{
        Self{
            value:Rc::new(RefCell::new(None)),
            wakers:RefCell::new(Vec::new()),
        }
    }
}