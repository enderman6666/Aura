use std::{
    cell::{Cell, RefCell, UnsafeCell}, 
    collections::VecDeque, 
    future::Future, 
    pin::Pin, 
    rc::{Rc, Weak}, 
    task::{Context, Poll, Waker},
    ops::{Deref, DerefMut},
};


// 异步互斥锁,用于对共享资源的访问控制，一旦锁住，其他任务必须等待，直到锁被释放
pub struct AsMutex<T>{
    waiters: RefCell<VecDeque<Waker>>,
    data: UnsafeCell<T>,
    locked: Cell<bool>,
}

impl<T> AsMutex<T>{
    pub fn new(data: T) -> Self{
        Self{
            waiters: RefCell::new(VecDeque::new()),
            data: UnsafeCell::new(data),
            locked: Cell::new(false),
        }
    }

    pub async fn lock(&self) -> AsMutexGuard<'_, T>{
        let lock=Locked::new(self).await;
        AsMutexGuard::new(self)
    }

}

pub struct AsMutexGuard<'a, T>{
    mutex:&'a AsMutex<T>,
}

impl<'a, T> AsMutexGuard<'a, T>{
    pub fn new(mutex:&'a AsMutex<T>) -> Self{
        Self{
            mutex,
        }
    }
}

impl<'a, T> Drop for AsMutexGuard<'a, T>{
    fn drop(&mut self) {
        self.mutex.locked.set(false);
        
    }
}

impl<'a, T> Deref for AsMutexGuard<'a, T>{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe{&*self.mutex.data.get()}
    }
}

impl<'a, T> DerefMut for AsMutexGuard<'a, T>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe{&mut *self.mutex.data.get()}
    }
}

impl<'a,T>Deref for Locked<'a, T>{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe{&*self.mutex.data.get()}
    }
}

pub struct Locked<'a, T>{
    mutex:&'a AsMutex<T>,
}

impl<'a, T> Locked<'a, T>{
    pub fn new( mutex:&'a AsMutex<T>) -> Self{
        Self{
            mutex,
        }
    }
}

impl<'a, T> Future for Locked<'a, T>{
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.mutex.locked.get() {
            self.mutex.waiters.borrow_mut().push_back(cx.waker().clone());
            Poll::Pending
        } else {
            self.mutex.locked.set(true);
            Poll::Ready(())
        }
    }
}

#[derive(Clone,Copy)]
enum RWLockMode{
    Read,
    Write,
    Unlock,
}

//异步读写锁,用于对共享资源的访问控制，一次只能有一个写锁，多个读锁可以同时存在
//读锁会在返回的future离开作用域被释放，写锁会在写守卫离开作用域时被释放
struct AsRWlock<T>{
    data: UnsafeCell<T>,
    locked: Cell<RWLockMode>,
    read_count: Cell<usize>,//只计算以await为边界的读锁
    waiters: RefCell<VecDeque<Waker>>,
}

impl<T> AsRWlock<T>{
    pub fn new(data: T) -> Self{
        Self{
            data: UnsafeCell::new(data),
            locked: Cell::new(RWLockMode::Unlock),
            read_count: Cell::new(0),
            waiters: RefCell::new(VecDeque::new()),
        }
    }

    pub async fn read(&self) -> Result<&T, String>{
        let lock=LockedRead::new(self).await;
        if let RWLockMode::Unlock = self.locked.get(){
            Ok(unsafe{&*self.data.get()})
        } else {
            Err("读写锁异常".to_string())
        }   
    }
}

struct LockedRead<'a, T>{
    rwlock:&'a AsRWlock<T>,
}

impl<'a, T> LockedRead<'a, T>{
    pub fn new(rwlock:&'a AsRWlock<T>) -> Self{
        Self{
            rwlock,
        }
    }
}

impl<'a, T> Future for LockedRead<'a, T>{
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let rwlock=self.rwlock.locked.get();
        match rwlock{
            RWLockMode::Unlock =>{
                self.rwlock.locked.set(RWLockMode::Read);
                self.rwlock.read_count.set(1);
                Poll::Ready(())
            }
            RWLockMode::Read =>{
                self.rwlock.read_count.set(self.rwlock.read_count.get()+1);
                Poll::Ready(())
            }
            RWLockMode::Write =>{
                self.rwlock.waiters.borrow_mut().push_back(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<'a, T> Drop for LockedRead<'a, T>{
    fn drop(&mut self) {
        self.rwlock.read_count.set(self.rwlock.read_count.get()-1);
        if self.rwlock.read_count.get() == 0{
            self.rwlock.locked.set(RWLockMode::Unlock);
            for waker in self.rwlock.waiters.borrow_mut().drain(..){
                waker.wake();
            }
        }
    }
}

struct LockWrite<'a,T>{
    rwlock:&'a AsRWlock<T>,
}

impl<'a, T> LockWrite<'a, T>{
    pub fn new(rwlock:&'a AsRWlock<T>) -> Self{
        Self{
            rwlock,
        }
    }
}

impl<'a, T> Future for LockWrite<'a, T>{
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let rwlock=self.rwlock.locked.get();
        match rwlock{
            RWLockMode::Unlock =>{
                self.rwlock.locked.set(RWLockMode::Write);
                Poll::Ready(())
            }
            RWLockMode::Read =>{
                self.rwlock.waiters.borrow_mut().push_back(cx.waker().clone());
                Poll::Pending
            }
            RWLockMode::Write =>{
                self.rwlock.waiters.borrow_mut().push_back(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

struct WLockedGuard<'a, T>{
    rwlock:&'a AsRWlock<T>,
}

impl<'a, T> WLockedGuard<'a, T>{
    pub fn new(rwlock:&'a AsRWlock<T>) -> Self{
        Self{
            rwlock,
        }
    }

    pub fn borrow_mut(&self) -> &mut T{
        unsafe{&mut *self.rwlock.data.get()}
    }
}

impl<'a, T> Drop for WLockedGuard<'a, T>{
    fn drop(&mut self) {
        self.rwlock.locked.set(RWLockMode::Unlock);
        if let Some( waker) = self.rwlock.waiters.borrow_mut().pop_front() {
            waker.wake();
        }
    }
}



#[cfg(test)]
mod tests{
    use super::*;
    use crate::DispatchCenter::singele::{
        singele_runtime::SingeleRuntime,
        time::Sleep
    };
    use std::time::Duration;
    use log::{debug,LevelFilter};
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
    fn test_as_mutex(){
        SingeleRuntime::run(
            async{
                let mutex = Rc::new(AsMutex::new(1));
                let mutex_clone = Rc::clone(&mutex);
                SingeleRuntime::spawn(async move{
                    let mut locked = mutex_clone.lock().await;
                    assert_eq!(*locked, 1);
                    *locked += 1;
                    assert_eq!(*locked, 5);
                }).await;
                SingeleRuntime::spawn(async move{
                    let guard = mutex.lock().await;
                    let l =guard;
                    assert_eq!(*l, 2);
                }).await;
            }
        );
    }

    #[test]
    fn test_as_mutexs(){
        setup_logger(LevelFilter::Trace);
        let mutex =Rc::new(AsMutex::new("你好，世界！".to_string()));
        let mutex_clone1 = Rc::clone(&mutex);
        let mutex_clone2 = Rc::clone(&mutex);
        let mutex_clone3 = Rc::clone(&mutex);
        let future1 =async move{
            let mut locked = mutex_clone1.lock().await;
            *locked = format!("{}，我是协程1", *locked);
            Sleep::new(Duration::from_micros(1000)).await;
            debug!("{}", *locked);
        };
        let future2 =async move{
            let mut locked = mutex_clone2.lock().await;
            Sleep::new(Duration::from_micros(500)).await;
            *locked = format!("{}，我是协程2", *locked);
            debug!("{}", *locked);
        };
        let future3 =async move{
            let mut locked = mutex_clone3.lock().await;
            *locked = format!("{}，我是协程3", *locked);
            Sleep::new(Duration::from_micros(250)).await;
            debug!("{}", *locked);
        };
        let futures: Vec<Pin<Box<dyn Future<Output = ()>>>> = vec![Box::pin(future1), Box::pin(future2), Box::pin(future3)];
        SingeleRuntime::run_all(futures);
    }
}
