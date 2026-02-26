use crate::GeneralComponents::lock_free::{
    occupying_queue::{OccupyQueue,State},
    atomic_queue::{AtomicQueue},
    error::QueueError,
};
use futures::{stream::Stream,Sink,SinkExt};
use std::{
    sync::{Arc},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::{Context, Poll,Waker},
    pin::Pin,
    marker::Unpin,
};


//有界异步通道
struct TranslationChannel<T>{
    queue:OccupyQueue<T>,
    recv_waker:AtomicQueue<Waker>,
    send_waker:AtomicQueue<Waker>,
    send_count:AtomicUsize,
    recv_count:AtomicUsize,
}

impl<T> TranslationChannel<T>{
    pub fn new(size:usize) -> (TrSender<T>,TrReceiver<T>){
        let channel = Arc::new(Self{
            queue:OccupyQueue::new(size),
            recv_waker:AtomicQueue::new(),
            send_waker:AtomicQueue::new(),
            send_count:AtomicUsize::new(1),
            recv_count:AtomicUsize::new(1),
        });
        (TrSender::new(channel.clone()),TrReceiver::new(channel.clone()))
    }
}

struct TrSender<T>{
    channel:Arc<TranslationChannel<T>>,
    waker:Option<Waker>,
    erro:bool,
    watting:bool,
    prt:Option<*mut State<T>>,
}

impl<T> TrSender<T>{
    pub fn new(channel:Arc<TranslationChannel<T>>) -> Self{
        Self{
            channel,
            waker:None,
            erro:false,
            watting:false,
            prt:None,
        }
    }

}

impl<T> Sink<T> for TrSender<T>{
    type Error = QueueError;
    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.erro{
            return Poll::Ready(Err(QueueError::QueueNotRun));
        }
        if !self.channel.queue.is_running(){
            return Poll::Ready(Err(QueueError::QueueNotRun));
        }
        if !self.channel.queue.is_full(){
            let mut backoff=1u32;
            loop{
                match self.channel.queue.get_solt(){
                    Ok(ptr)=> {
                        self.prt=Some(ptr);
                        break;
                    },
                    Err(QueueError::QueueFull)=>return Poll::Pending,
                    Err(QueueError::QueueNotRun)=>{
                        self.erro=true;
                        return Poll::Ready(Err(QueueError::QueueNotRun));
                    },
                    Err(QueueError::CasFailed)=>{
                        for _ in 0..backoff {
                            std::hint::spin_loop();
                        }
                        if backoff < 64 {
                            backoff *= 2;
                        }else{
                            cx.waker().clone().wake();
                            return Poll::Pending;
                        }
                    },
                    Err(_)=>{
                        cx.waker().clone().wake();
                        return Poll::Pending;
                    }
                };
            }
            return Poll::Ready(Ok(()));
        }else{
            if !self.watting{
                let this = self.get_mut();
                this.watting=true;
                this.waker=Some(cx.waker().clone());
            }
            return Poll::Pending;
        }
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        if self.erro{
            return Err(QueueError::QueueNotRun);
        }
        let this = self.get_mut();
        match this.prt{
            Some(ptr)=>{
                unsafe{
                    *ptr=State::Data(item);
                }
            },
            None=>{
                return Err(QueueError::Unknow);
            }
        }
        if let Some(waker)=this.waker.take(){
            waker.wake();
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.erro{
            return Poll::Ready(Err(QueueError::QueueNotRun));
        }else{
            return Poll::Ready(Ok(()));
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        return Poll::Ready(Ok(()));
    }

}

impl<T> Unpin for TrSender<T>{}

impl <T> Clone for TrSender<T>{
    fn clone(&self) -> Self {
        let _ = self.channel.send_count.fetch_add(1,Ordering::Release);
        Self{
            channel:self.channel.clone(),
            waker:None,
            erro:false,
            watting:false,
            prt:None,
        }
    }
}

struct TrReceiver<T>{
    channel:Arc<TranslationChannel<T>>,
    waker:Option<Waker>,
    erro:bool,
    watting:bool,
}

impl<T> TrReceiver<T>{
    pub fn new(channel:Arc<TranslationChannel<T>>) -> Self{
        Self{
            channel,
            waker:None,
            erro:false,
            watting:false,
        }
    }
}

impl<T> Stream for TrReceiver<T>{
    type Item = Result<Option<T>,QueueError>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.waker=Some(cx.waker().clone());
        if this.erro{
            return Poll::Ready(None);
        }
        if this.channel.send_count.load(Ordering::Acquire)==0&&this.channel.queue.is_empty(){
            return Poll::Ready(None);
        }
        let mut backoff=1u32;
        loop{
            match this.channel.queue.try_pop(){
                Ok(item)=>{
                    match item{
                        Some(item)=>{
                            return Poll::Ready(Some(Ok(Some(item))));
                        },
                        None=>{
                            if this.watting{
                                match this.channel.recv_waker.push(cx.waker().clone()){
                                    Ok(_)=>{
                                        return Poll::Pending;
                                    },
                                    Err(_)=>{
                                        return Poll::Ready(None);
                                    },
                                }
                            }
                            this.watting=true;
                            return Poll::Pending;
                        },
                    }
                },
                Err(QueueError::CasFailed)=>{
                    for _ in 0..backoff {
                        std::hint::spin_loop();
                    }
                    if backoff < 64 { 
                        backoff *= 2;
                    }
                },
                Err(QueueError::QueueEmpty)=>{
                    let _ = this.channel.recv_waker.push(cx.waker().clone());
                    return Poll::Pending;
                },
                Err(QueueError::QueueNotRun)=>{
                    this.erro=true;
                    return Poll::Ready(Some(Err(QueueError::QueueNotRun)));
                },
                Err(_)=>{
                    let _ = this.channel.recv_waker.push(cx.waker().clone());
                    return Poll::Pending;
                },
            }
        }
    }
}

impl<T> Drop for TrReceiver<T>{
    fn drop(&mut self) {
        let _ = self.channel.recv_count.fetch_sub(1,Ordering::Release);
    }
}

impl<T> Clone for TrReceiver<T>{
    fn clone(&self) -> Self {
        let _ = self.channel.recv_count.fetch_add(1,Ordering::Release);
        Self{
            channel:self.channel.clone(),
            waker:None,
            erro:false,
            watting:false,
        }
    }
}
