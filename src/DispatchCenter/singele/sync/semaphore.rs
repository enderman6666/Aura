use std::{
    future::Future,
    task::{
        Waker,Poll,Context
    },
    cell::RefCell,
    rc::Rc,
    pin::Pin,
};

use log::debug;


// 信号量,用于控制并发数
pub struct AsyncSemaphore{
    maxcount:usize,
    usedcount:Rc<RefCell<usize>>,
    waiters:Rc<RefCell<Vec<Waker>>>,
}

impl AsyncSemaphore{
    fn new(maxcount:usize)->Self{
        Self{
            maxcount,
            usedcount:Rc::new(RefCell::new(0)),
            waiters:Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn acquire(&self)->impl Future<Output=SemaphoreGuard>{
        SemaphoreAcquire::new(self.usedcount.clone(), self.waiters.clone(), self.maxcount)
    }
}

pub struct SemaphoreGuard{
    usedcount:Rc<RefCell<usize>>,
    waiters:Rc<RefCell<Vec<Waker>>>,
}

impl Drop for SemaphoreGuard{
    fn drop(&mut self) {
        let mut usedcount = self.usedcount.borrow_mut();
        *usedcount -= 1;
        debug!("SemaphoreGuard drop: 释放信号量，usedcount={}", *usedcount);
        
        let mut waiters = self.waiters.borrow_mut();
        if !waiters.is_empty() {
            let waker = waiters.remove(0);
            drop(waiters);
            waker.wake();
            debug!("SemaphoreGuard drop: 唤醒等待的任务");
        }
    }
}

struct SemaphoreAcquire{
    usedcount:Rc<RefCell<usize>>,
    waiters:Rc<RefCell<Vec<Waker>>>,
    maxcount:usize,
}

impl SemaphoreAcquire{
    fn new(usedcount:Rc<RefCell<usize>>, waiters:Rc<RefCell<Vec<Waker>>>, maxcount:usize)->Self{
        Self{
            usedcount,
            waiters,
            maxcount,
        }
    }
}

impl Future for SemaphoreAcquire{
    type Output = SemaphoreGuard;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut usedcount = self.usedcount.borrow_mut();
        debug!("SemaphoreAcquire poll: usedcount={}, maxcount={}", *usedcount, self.maxcount);
        if *usedcount < self.maxcount{
            *usedcount += 1;
            debug!("SemaphoreAcquire poll: 获取信号量成功，usedcount={}", *usedcount);
            Poll::Ready(SemaphoreGuard{
                usedcount: self.usedcount.clone(),
                waiters: self.waiters.clone(),
            })
        }else{
            debug!("SemaphoreAcquire poll: 信号量已满，加入等待队列");
            self.waiters.borrow_mut().push(cx.waker().clone());
            Poll::Pending
        }
    }
}



#[cfg(test)]
mod tests{
    use super::*;
    use crate::DispatchCenter::singele::singele_runtime::SingeleRuntime;
    use log::{debug,info,LevelFilter};
    use env_logger;
    use std::{rc::Rc,cell::RefCell,time::Duration};
    fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("aura::DispatchCenter::singele::sync", LevelFilter::Trace) 
            .filter_module("mio", LevelFilter::Warn)
            .filter_module("aura::DispatchCenter::singele::singele_runtime", LevelFilter::Off)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
    
    #[test]
    fn test_semaphore(){
        setup_logger(LevelFilter::Info);
        let semaphore = Rc::new(RefCell::new(AsyncSemaphore::new(2)));
        let semaphore_clone1 = Rc::clone(&semaphore);
        let semaphore_clone2 = Rc::clone(&semaphore);
        let semaphore_clone3 = Rc::clone(&semaphore);
        let semaphore_clone4 = Rc::clone(&semaphore);
        let future1=async move {
            info!("锁1等待信号量");
            let _guard = semaphore_clone1.borrow().acquire().await;
            info!("锁1获取到信号量");
            SingeleRuntime::sleep(Duration::from_millis(200)).await;
            info!("锁1执行完毕");
        };
        let future2=async move {
            info!("锁2等待信号量");
            let _guard = semaphore_clone2.borrow().acquire().await;
            info!("锁2获取到信号量");
            SingeleRuntime::sleep(Duration::from_millis(100)).await;
            info!("锁2执行完毕");
        };
        let future3=async move {
            info!("锁3等待信号量");
            let _guard = semaphore_clone3.borrow().acquire().await;
            info!("锁3获取到信号量");
            info!("锁3执行完毕");
        };
        let future4=async move {
            info!("锁4等待信号量");
            let _guard = semaphore_clone4.borrow().acquire().await; 
            info!("锁4获取到信号量");
            SingeleRuntime::sleep(Duration::from_millis(300)).await;  
            info!("锁4执行完毕");
        };
        let list:Vec<Pin<Box<dyn Future<Output=()>>>> = vec![Box::pin(future1),Box::pin(future2),Box::pin(future3),Box::pin(future4)];
        SingeleRuntime::run_all(list);
    }
}