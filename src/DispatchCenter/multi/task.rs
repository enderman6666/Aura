use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicPtr, Ordering },
    task::Waker,
    cell::RefCell,
};


pub struct Task{
    id: u64,
    future: RefCell<Option<Pin<Box<dyn Future<Output = ()> + Send>>>>,
    waker: AtomicPtr<Option<Waker>>,
}

impl Task{
    pub fn new(id: u64, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> Self{
        Self{
            id,
            future: RefCell::new(Some(future)),
            waker: AtomicPtr::new(Box::into_raw(Box::new(None))),
        }
    }

    pub fn id(&self) -> u64{
        self.id
    }

    pub fn waker(&self) -> *mut Option<Waker>{
        self.waker.load(Ordering::Acquire)
    }

    pub fn future(&self) -> &RefCell<Option<Pin<Box<dyn Future<Output = ()> + Send>>>>{
        &self.future
    }

    pub fn clear_future(&mut self)->Result<(), String>{
        if self.future.get_mut().is_none(){
            return Err("future is None".to_string());
        }
        *self.future.get_mut() = None;
        self.waker.store(Box::into_raw(Box::new(None)), Ordering::Release);
        Ok(())
    }

    // 设置任务的唤醒器
    pub fn set_waker(&self, waker: Waker){
        self.waker.store(Box::into_raw(Box::new(Some(waker))), Ordering::Release);
    }
}

impl Drop for Task{
    fn drop(&mut self){
        unsafe{
            drop(Box::from_raw(self.waker.load(Ordering::Acquire)));
        }
    }
}
