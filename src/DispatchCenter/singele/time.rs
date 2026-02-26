use std::{
    time::{Duration, Instant}, 
    task::{Context, Poll, Waker},
    future::Future,
    pin::{Pin},
    rc::{Rc, Weak},
    cell::RefCell,
};
use crate::GeneralComponents::time::time_wheel::TimeWheel;
use log::debug;


pub enum Either<L, R>{
    Left(L),
    Right(R),
}

// 两个future同时执行,返回先完成的结果
pub struct Race<L, R>{
    left: Option<L>,
    right: Option<R>,
    waker: Option<Waker>,
}

impl<L,R> Race<L, R>
where
    L: Future,
    R: Future,
{
    pub fn new(left: L, right: R) -> Self{
        Self{
            left: Some(left),
            right: Some(right),
            waker: None,
        }
    }
}

impl<L,R> Future for Race<L, R>
where
    L: Future,
    R: Future,
{
    type Output = Either<L::Output, R::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        let race = unsafe { self.get_unchecked_mut() };
        let left = race.left.as_mut().unwrap();
        let right = race.right.as_mut().unwrap();
        let left = unsafe { Pin::new_unchecked(left) };
        let right = unsafe { Pin::new_unchecked(right) };
        match left.poll(cx){
            Poll::Ready(output) => {
                race.left.take();
                Poll::Ready(Either::Left(output))
            },
            Poll::Pending => match right.poll(cx){
                Poll::Ready(output) => {
                    race.right.take();
                    Poll::Ready(Either::Right(output))
                },
                Poll::Pending => {
                    race.waker = Some(cx.waker().clone());
                    Poll::Pending
                },
            },
        }
    }
}

// 等待指定时间
pub struct Sleep{
    waker: Option<Waker>,
    duration: Duration,
    start_time: Instant,
    time_wheel: Rc<RefCell<TimeWheel>>,
}

impl Sleep{
    pub fn new(duration: Duration, time_wheel: Rc<RefCell<TimeWheel>>) -> Self{
        let sleep=Sleep{
            waker: None,
            duration,
            start_time: Instant::now(),
            time_wheel: time_wheel.clone(),
        };
        debug!("创建新任务,等待时间: {:?}", sleep.duration);
        sleep
    }
}

impl Future for Sleep{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        if Instant::now() >= self.start_time + self.duration{
            Poll::Ready(())
        } else {
            self.waker = Some(cx.waker().clone());
            let _=self.time_wheel.borrow_mut().add_task(self.start_time , self.duration, Some(cx.waker().clone()));
            Poll::Pending
        }
    }
}


#[cfg(test)]
mod tests{
    use std::time::{Duration, Instant};
    use crate::DispatchCenter::singele::singele_runtime::SingeleRuntime;
    use crate::DispatchCenter::singele::time::Either;
    use log::{debug,LevelFilter};
        use env_logger;

        fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level) // 主过滤级别
            .filter_module("Aura", LevelFilter::Trace) // 你的crate用最详细级别
            .filter_module("mio", LevelFilter::Warn) // 第三方库只显示警告
            .is_test(true)
            .format_timestamp_nanos() // 高精度时间戳
            .try_init()
            .ok(); // 忽略初始化失败，避免在多次调用时panic
        }
    #[test]
    fn test_sleep(){
        setup_logger(LevelFilter::Trace);
        debug!("测试开始");
        SingeleRuntime::run(
            async{
                let sleep = SingeleRuntime::sleep(Duration::from_micros(100));
                let start_time = Instant::now();
                debug!("开始等待");
                sleep.await;
                debug!("等待完成");
                let end_time = Instant::now();
                assert!(end_time - start_time >= Duration::from_micros(100));
            }
        );
    }

    #[test]
    fn test_race(){
        setup_logger(LevelFilter::Trace);
        debug!("测试开始");
        SingeleRuntime::run(
            async{
                let sleep_1 = async{
                    SingeleRuntime::sleep(Duration::from_millis(100));
                    "任务1".to_string()
                };
                let sleep_2 = async{
                    SingeleRuntime::sleep(Duration::from_millis(200));
                    "任务2".to_string()
                };
                let race = SingeleRuntime::race(sleep_1, sleep_2);
                match race.await{
                    Either::Left(output) => {
                        assert_eq!(output, "任务1");
                    },
                    _=>{
                        panic!("任务1应该先完成");
                    }
                }
            }
        );
    }
}