use std::{
    sync::{Arc,Mutex},
    future::Future,
    task::{Context,Poll,Waker},
    pin::Pin,
    rc::Rc,
    cell::RefCell,
};

pub struct JoinHandle<T>{    
    output: Arc<Mutex<Output<T>>>,
}

impl<T> JoinHandle<T>{
    pub fn new(output: Arc<Mutex<Output<T>>>) -> Self{
        Self{
            output,
        }
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(value) = self.output.lock().unwrap().data.replace(None) {
            Poll::Ready(value)
        } else {
            self.output.lock().unwrap().wakers.push(cx.waker().clone());
            Poll::Pending
        }
    }

}



pub struct Output<T>{
    pub data: RefCell<Option<T>>,
    wakers: Vec<Waker>,
}

impl<T> Output<T>{
    pub fn new()->Self{
        Self{
            data: RefCell::new(None),
            wakers: Vec::new(),
        }
    }

    pub fn notify(&mut self){
        let wakers = self.wakers.drain(..);
        for waker in wakers{
            waker.wake();
        }
    }
}
