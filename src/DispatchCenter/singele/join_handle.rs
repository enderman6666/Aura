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
<<<<<<< HEAD
=======
    waker: Option<Waker>,
>>>>>>> e60328c5e3519f88c05570a2decc4d91ef322717
    output: Rc<RefCell<Output<T>>>,
}

impl<T> JoinHandle<T>{
    pub fn new(output: Rc<RefCell<Output<T>>>) -> Self{
<<<<<<< HEAD
        Self{       
=======
        Self{
            waker: None,            
>>>>>>> e60328c5e3519f88c05570a2decc4d91ef322717
            output,
        }
    }
}


impl<T> Future for JoinHandle<T> {
    type Output = T;
<<<<<<< HEAD
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(value) = self.output.borrow_mut().value.borrow_mut().take() {
            Poll::Ready(value)
        } else {
=======
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 首先检查任务是否已经完成
        if let Some(value) = self.output.borrow_mut().value.borrow_mut().take() {
            // 任务已经完成，返回结果
            Poll::Ready(value)
        } else {
            // 将waker添加到Output的wakers列表中
>>>>>>> e60328c5e3519f88c05570a2decc4d91ef322717
            self.output.borrow_mut().wakers.borrow_mut().push(cx.waker().clone());
            Poll::Pending
        }
    }

}
