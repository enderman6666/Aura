use std::time::{Duration, Instant};
use std::{
    task::Waker,
    pin::Pin,
    task::{Context,Poll},
    future::Future,
};
use crate::DispatchCenter::multi::runtime::Runtime;
use log::{debug,error};


// 异步睡眠指定时间
// 用于异步任务的延迟执行
pub struct Sleep{
    waker: Option<Waker>,
    duration: Duration,
    start_time: Instant,
}

impl Sleep{
    pub fn new(duration: Duration) -> Self{
        let sleep=Sleep{
            waker: None,
            duration,
            start_time: Instant::now(),
        };
        sleep
    }
}

impl Future for Sleep{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        if Instant::now() >= self.start_time + self.duration{
            Poll::Ready(())
        } else {
            let time_wheel = Runtime::get_time_wheel();
            match time_wheel.borrow_mut().add_task(self.start_time , self.duration, Some(cx.waker().clone())){
                Ok(_) => {
                    self.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
                Err(e) => {
                    error!("添加任务到时间轮失败: {:?}", e);
                    Poll::Pending
                }
            }
        }
    }
}



