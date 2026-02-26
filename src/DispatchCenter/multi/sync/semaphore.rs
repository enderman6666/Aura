use std::{
    sync::atomic::{AtomicU32, Ordering},
    task::{Context, Poll, Waker},
    future::Future,
    pin::Pin,
};
use crate::GeneralComponents::lock_free::{atomic_queue::AtomicQueue};


/// 信号量
/// 用于控制同时访问资源的异步任务数量
pub struct Semaphore{
    max_count: u32,
    count: AtomicU32,
    waiters: AtomicQueue<Waker>,
}

impl Semaphore{
    pub fn new(max_count: u32) -> Self {
        Self {
            max_count,
            count: AtomicU32::new(0),
            waiters: AtomicQueue::new(),
        }
    }

    pub fn acquire(&'static self) -> SemaphoreAcquire{
        SemaphoreAcquire::new(self)
    }
}

/// 信号量Guard
/// 用于在SemaphoreGuard被drop时释放信号量
pub struct SemaphoreGuard{
    semaphore: &'static Semaphore,
}

impl SemaphoreGuard{
    pub fn new(semaphore: &'static Semaphore) -> Self {
        Self { semaphore }
    }
}

impl Drop for SemaphoreGuard{
    fn drop(&mut self) {
        self.semaphore.count.fetch_sub(1, Ordering::Release);
        if let Some(waker) = self.semaphore.waiters.pop() {
            waker.wake();
        }
    }
}

/// 信号量Acquire
/// 用于在SemaphoreAcquire被poll时获取信号量
pub struct SemaphoreAcquire{
    semaphore: &'static Semaphore,
    waker: Option<Waker>,
}

impl SemaphoreAcquire{
    fn new(semaphore: &'static Semaphore) -> Self {
        Self {
            semaphore,
            waker: None,
        }
    }
}

impl Future for SemaphoreAcquire{
    type Output = SemaphoreGuard;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop{
            let count = self.semaphore.count.load(Ordering::Acquire);
            if count < self.semaphore.max_count {
                if self.semaphore.count.compare_exchange_weak(count, count + 1, Ordering::Release, Ordering::Acquire).is_ok() {
                    return Poll::Ready(SemaphoreGuard::new(self.semaphore));
                }else{
                    continue;
                }
            } else {
                self.waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }
    }
}