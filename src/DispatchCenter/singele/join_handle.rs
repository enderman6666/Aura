use std::{
    future::Future,
    task::{Context,Poll,Waker},
    pin::Pin,
    rc::{Rc,Weak},
    cell::RefCell,
};
use super::task::{Output};

// 加入句柄,在主future中监听其它任务的输出
pub struct JoinHandle<T> {
    output: Rc<RefCell<Output<T>>>,
}

impl<T> JoinHandle<T>{
    pub fn new(output: Rc<RefCell<Output<T>>>) -> Self{
        Self{       
            output,
        }
    }
}


impl<T> Future for JoinHandle<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(value) = self.output.borrow_mut().value.borrow_mut().take() {
            Poll::Ready(value)
        } else {
            self.output.borrow_mut().wakers.borrow_mut().push(cx.waker().clone());
            Poll::Pending
        }
    }

}
