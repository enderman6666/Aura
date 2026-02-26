use std::{
    sync::{
        atomic::{AtomicU32,AtomicBool, Ordering},
        Arc,
    },
    pin::Pin,
    task::{Context, Poll, Waker},
    future::Future,
};
use crate::GeneralComponents::lock_free::atomic_queue::{AtomicQueue};

pub struct Notify{
    arc:Arc<Notifyer>,
}

impl Notify{
    pub fn new()->Self{
        Self{
            arc:Arc::new(Notifyer::new()),
        }
    }

    pub fn notify_one(&self){
        self.arc.notify_one();
    }
    
    pub fn notify_all(&self){
        self.arc.notify_all();
    }

    pub fn get(&self)->NotifyerGurad{
        NotifyerGurad::new(self.arc.clone())
    }
}

pub struct Notifyer{
    queue:AtomicQueue<Waker>,
    run:AtomicBool,
    waiting:AtomicU32,
}

impl Notifyer{
    pub fn new()->Self{
        Self{
            queue:AtomicQueue::new(),
            run:AtomicBool::new(true),
            waiting:AtomicU32::new(0),
        }
    }

    pub fn notify_one(&self){
         if let Some(guard) = self.queue.pop(){
            self.waiting.fetch_add(1, Ordering::Release);
            guard.wake();
        }
    }

    pub fn notify_all(&self){
        while let Some(guard) = self.queue.pop(){
            self.waiting.fetch_add(1, Ordering::Release);
            guard.wake();
        }
    }
}

pub struct NotifyerGurad{
    notifyer:Arc<Notifyer>,
    waker:Option<Waker>,
}

impl NotifyerGurad{
    pub fn new(notifyer:Arc<Notifyer>)->Self{
        Self{
            notifyer,
            waker:None,
        }
    }

    pub fn wake(&self){
        if let Some(waker) = self.waker.as_ref(){
            waker.wake_by_ref();
        }
    }
}

impl Future for NotifyerGurad{
    type Output = Result<(),()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>{
        if !self.notifyer.run.load(Ordering::Acquire){
            return Poll::Ready(Ok(()));
        }
        if self.notifyer.waiting.load(Ordering::Acquire) > 0{
            self.notifyer.waiting.fetch_sub(1, Ordering::Release);
            Poll::Ready(Ok(()))
        } else {
            self.waker = Some(cx.waker().clone());
            match self.notifyer.queue.push(cx.waker().clone()){
                Ok(_) => Poll::Pending,
                Err(_) => Poll::Ready(Err(())),
            }
        }
    }
}
