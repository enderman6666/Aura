use std::{
    task::{Context, Poll, Waker},
    cell::UnsafeCell,
    future::Future,
    pin::Pin,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, AtomicU8,AtomicBool,Ordering},
};
use crate::GeneralComponents::lock_free::{prt::Atomic,atomic_queue::AtomicQueue};


// 异步互斥锁,用于在异步环境中保护共享资源
pub struct AsMutex<T>{
    waiters: AtomicQueue<Waker>,
    data: UnsafeCell<T>,
    locked: AtomicBool,//false表示未锁,true表示已锁
}

impl<T> AsMutex<T>{
    pub fn new(data: T) -> Self{
        Self{
            waiters: AtomicQueue::new(),
            data: UnsafeCell::new(data),
            locked: AtomicBool::new(false),//false表示未锁,true表示已锁
        }
    }

    // 采用异步的方式尝试获取锁
    pub fn lock(&self) -> Lock<'_, T>{
        Lock::new(self)
    }

    pub fn try_lock(&self) -> Result<AsMutexGuard<'_, T>,()> {
        match self.locked.compare_exchange(false, true, Ordering::Release, Ordering::Acquire){
            Ok(_) => Ok(AsMutexGuard{
                mutex: self,
            }),
            Err(_) => Err(()),
        }
    }
}

pub struct Lock<'a, T>{
    mutex: &'a AsMutex<T>,
    waker:Option<Waker>,
}

impl<'a, T> Lock<'a, T>{
    fn new(mutex: &'a AsMutex<T>) -> Self{
        Self{
            mutex,
            waker:None,
        }
    }
}

impl<'a, T> Future for Lock<'a, T>{
    type Output = Result<AsMutexGuard<'a, T>,()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        let state=self.mutex.locked.load(Ordering::Acquire);
        match state{
            false => {
                if self.mutex.locked.compare_exchange(false, true, Ordering::Release, Ordering::Acquire).is_ok(){
                    Poll::Ready(Ok(AsMutexGuard{
                        mutex: self.mutex,
                    }))
                }else{
                    self.waker = Some(cx.waker().clone());
                    match self.mutex.waiters.push(cx.waker().clone()){
                        Ok(_) => Poll::Pending,
                        Err(_) => Poll::Ready(Err(())),
                    }
                }
            }
            true => {
                self.waker = Some(cx.waker().clone());
                match self.mutex.waiters.push(cx.waker().clone()){
                    Ok(_) => Poll::Pending,
                    Err(_) => Poll::Ready(Err(())),
                }
            }
        }
    }
}

pub struct AsMutexGuard<'a, T>{
    mutex: &'a AsMutex<T>,
}

impl<'a, T> AsMutexGuard<'a, T>{
    fn new(mutex: &'a AsMutex<T>) -> Self{
        Self{
            mutex,
        }
    }
}

impl<'a, T> Deref for AsMutexGuard<'a, T>{
    type Target = T;
    fn deref(&self) -> &Self::Target{
        unsafe{&*self.mutex.data.get()}
    }
}

impl<'a, T> DerefMut for AsMutexGuard<'a, T>{
    fn deref_mut(&mut self) -> &mut Self::Target{
        unsafe{&mut *self.mutex.data.get()}
    }
}

impl<'a, T> Drop for AsMutexGuard<'a, T>{
    fn drop(&mut self){
        if let Some(waker)=self.mutex.waiters.pop(){
            waker.wake();
        }
        self.mutex.locked.store(false, Ordering::Release);
    }
}

unsafe impl<T> Sync for AsMutex<T>{}
unsafe impl<T> Send for AsMutex<T>{}

struct AsRwLock<T>{
   data: UnsafeCell<T>,
   locked: AtomicU8,//0表示未锁,1表示写锁,2表示读锁
   read_count: AtomicU32,
   read_list: AtomicQueue<Waker>,
   write_list: AtomicQueue<Waker>,
}

impl<T> AsRwLock<T>{
    pub fn new(data: T) -> Self{
        Self{
            data: UnsafeCell::new(data),
            locked: AtomicU8::new(0),
            read_count: AtomicU32::new(0),
            read_list: AtomicQueue::new(),
            write_list: AtomicQueue::new(),
        }
    }

    // 采用异步的方式尝试获取读锁
    pub fn read(&self) -> ReadLock<'_, T>{
        ReadLock::new(self)
    }

    // 采用异步的方式尝试获取写锁
    pub fn write(&self) -> WriteLock<'_, T>{
        WriteLock::new(self)
    }

    pub fn try_read(&self) -> Result<ReadLockGuard<'_, T>,()> {
        match self.locked.load(Ordering::Acquire){
            0 => {
                if self.locked.compare_exchange(0, 2, Ordering::Release, Ordering::Acquire).is_ok(){
                    self.read_count.fetch_add(1, Ordering::Acquire);
                    Ok(ReadLockGuard::new(self))
                }else{
                    Err(())
                }
            },
            1 => Err(()),
            2 => {
                self.read_count.fetch_add(1, Ordering::Acquire);
                Ok(ReadLockGuard::new(self))
            },
            _ => Err(()),
        }
    }

    pub fn try_write(&self) -> Result<WriteLockGuard<'_, T>,()> {
        match self.locked.compare_exchange(0, 1, Ordering::Release, Ordering::Acquire){
            Ok(_) => Ok(WriteLockGuard::new(self)),
            Err(_) => Err(()),
        }
    }
}

unsafe impl<T> Sync for AsRwLock<T>{}
unsafe impl<T> Send for AsRwLock<T>{}

struct ReadLock<'a, T>{
    rwlock: &'a AsRwLock<T>,
    waker:Option<Waker>,
}

impl<'a, T> ReadLock<'a, T>{
    fn new(rwlock: &'a AsRwLock<T>) -> Self{
        Self{
            rwlock,
            waker:None,
        }
    }
}

impl <'a,T> Future for ReadLock<'a,T> {
    type Output = Result<ReadLockGuard<'a, T>,()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        let state=self.rwlock.locked.load(Ordering::Acquire);
        match state{
            0 => {
                if self.rwlock.locked.compare_exchange(0, 2, Ordering::Release, Ordering::Acquire).is_ok(){
                    self.rwlock.read_count.fetch_add(1, Ordering::Relaxed);
                    Poll::Ready(Ok(ReadLockGuard::new(self.rwlock)))
                }else{
                    self.waker = Some(cx.waker().clone());
                    match self.rwlock.read_list.push(cx.waker().clone()){
                        Ok(_) => Poll::Pending,
                        Err(_) => Poll::Ready(Err(())),
                    }
                }
            }
            2 => {
                self.rwlock.read_count.fetch_add(1, Ordering::Relaxed);
                Poll::Ready(Ok(ReadLockGuard::new(self.rwlock)))
            }
            1 => {
                self.waker = Some(cx.waker().clone());
                match self.rwlock.write_list.push(cx.waker().clone()){
                    Ok(_) => Poll::Pending,
                    Err(_) => Poll::Ready(Err(())),
                }
            }
            _ => {
                Poll::Ready(Err(()))
            }
        }
    }   
}
struct ReadLockGuard<'a, T>{
    rwlock: &'a AsRwLock<T>,
}

impl<'a, T> ReadLockGuard<'a, T>{
    fn new(rwlock: &'a AsRwLock<T>) -> Self{
        Self{
            rwlock,
        }
    }
}

impl<'a, T> Deref for ReadLockGuard<'a, T>{
    type Target = T;
    fn deref(&self) -> &Self::Target{
        unsafe{&*self.rwlock.data.get()}
    }
}

impl <'a, T> Drop for ReadLockGuard<'a, T>{
    fn drop(&mut self){
        self.rwlock.read_count.fetch_sub(1, Ordering::Release);
        if self.rwlock.read_count.load(Ordering::Acquire) == 0{
            if self.rwlock.locked.compare_exchange(2, 0, Ordering::Release, Ordering::Acquire).is_ok(){
                if let Some(waker)=self.rwlock.write_list.pop(){
                    waker.wake();
                }
            }
        }else{
            let list=self.rwlock.read_list.pop_all();
            if !list.is_empty(){
                for waker in list{
                    waker.wake();
                }
            }
        }
    }
}

struct WriteLock<'a, T>{
    rwlock: &'a AsRwLock<T>,
    waker:Option<Waker>,
}

impl<'a, T> WriteLock<'a, T>{
    fn new(rwlock: &'a AsRwLock<T>) -> Self{
        Self{
            rwlock,
            waker:None,
        }
    }
}

impl <'a, T> Future for WriteLock<'a, T> {
    type Output = Result<WriteLockGuard<'a, T>,()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        let state=self.rwlock.locked.load(Ordering::Acquire);
        match state{
            0 => {
                if self.rwlock.locked.compare_exchange(0, 1, Ordering::Release, Ordering::Acquire).is_ok(){
                    Poll::Ready(Ok(WriteLockGuard::new(self.rwlock)))
                }else{
                    self.waker = Some(cx.waker().clone());
                    match self.rwlock.write_list.push(cx.waker().clone()){
                        Ok(_) => Poll::Pending,
                        Err(_) => Poll::Ready(Err(())),
                    }
                }
            }
            2 => {
                self.waker = Some(cx.waker().clone());
                match self.rwlock.write_list.push(cx.waker().clone()){
                    Ok(_) => Poll::Pending,
                    Err(_) => Poll::Ready(Err(())),
                }
            }
            1 => {
                self.waker = Some(cx.waker().clone());
                match self.rwlock.write_list.push(cx.waker().clone()){
                    Ok(_) => Poll::Pending,
                    Err(_) => Poll::Ready(Err(())),
                }
            }
            _ => {
                Poll::Ready(Err(()))
            }
        }
    }   
}

struct WriteLockGuard<'a, T>{
    rwlock: &'a AsRwLock<T>,
}

impl<'a, T> WriteLockGuard<'a, T>{
    fn new(rwlock: &'a AsRwLock<T>) -> Self{
        Self{
            rwlock,
        }
    }
}

impl<'a, T> Deref for WriteLockGuard<'a, T>{
    type Target = T;
    fn deref(&self) -> &Self::Target{
        unsafe{&*self.rwlock.data.get()}
    }
}

impl<'a, T> DerefMut for WriteLockGuard<'a, T>{
    fn deref_mut(&mut self) -> &mut Self::Target{
        unsafe{&mut *self.rwlock.data.get()}
    }
}

impl <'a, T> Drop for WriteLockGuard<'a, T>{
    fn drop(&mut self){
        if let Some(waker)=self.rwlock.write_list.pop(){
            waker.wake();
        }else{
            let list=self.rwlock.read_list.pop_all();
            for waker in list{
                waker.wake();
            }
        }
        self.rwlock.locked.store( 0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::DispatchCenter::multi::scheduler::Scheduler;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    #[test]
    fn test_as_mutex(){
        let mutex=Arc::new(AsMutex::new(0));
        let mutex1=mutex.clone();
        let mutex2=mutex.clone();
        let future1=async move {
            println!("future1 start");
            let n=mutex1.lock().await;
            match n{
                Ok(mut n) => {
                    *n+=1;
                }
                Err(_) => {
                    panic!("lock failed");
                }
            }
            println!("future1 done");
        };
        let future2=async move {
            println!("future2 start");
            let n=mutex2.lock().await;
            match n{
                Ok(mut n) => {
                    *n+=1;
                }
                Err(_) => {
                    panic!("lock failed");
                }
            }
            println!("future2 done");
        };
        Scheduler::new(2);
        println!("start");
        Scheduler::add(vec![Box::pin(future1), Box::pin(future2)]);
        println!("join");
        Scheduler::join();
        println!("after join");
        let k=mutex.try_lock();
        match k{
            Ok(n) => {
                println!("try lock success");
            }
            Err(_) => {
                panic!("lock failed");
            }
        }
    }

    #[test]
    fn test_as_rwlock(){
        let rwlock=Arc::new(AsRwLock::new(0));
        let rwlock1=rwlock.clone();
        let rwlock2=rwlock.clone();
        let future1=async move {
            println!("future1 start");
            let n=rwlock1.read().await;
            match n{
                Ok(n) => {
                    println!("future1 read: {}", *n);
                }
                Err(_) => {
                    panic!("lock failed");
                }
            }
            println!("future1 done");
        };
        let future2=async move {
            println!("future2 start");
            let n=rwlock2.write().await;
            match n{
                Ok(mut n) => {
                    *n+=1;
                }
                Err(_) => {
                    panic!("lock failed");
                }
            }
            println!("future2 done");
        };
        Scheduler::new(2);
        println!("start");
        Scheduler::add(vec![Box::pin(future1), Box::pin(future2)]);
        println!("join");
        Scheduler::join();
        println!("after join");
        {
            let k=rwlock.try_read();
            match k{
                Ok(n) => {
                    println!("try read success: {}", *n);
                }
                Err(_) => {
                    panic!("lock failed");
                }
            }
        }
        let k=rwlock.try_write();
        match k{
            Ok(mut n) => {
                *n+=1;
                println!("try write success: {}", *n);
            }
            Err(_) => {
                panic!("lock failed");
            }
        }
    }
}
