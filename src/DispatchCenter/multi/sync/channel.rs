use std::{
    sync::Arc
};
use crate::GeneralComponents::lock_free::{atomic_queue::{AtomicQueue},error::QueueError};
use super::super::async_yield::AsyncYield;

/// 无界通道
/// 用于在不同线程之间传递数据
/// 发送端和接收端可以在不同的线程中使用，使用异步方法优化CAS竞争
pub struct Channel<T>{
    data:AtomicQueue<T>,
}

impl<T> Channel<T>{
    pub fn new() -> (Sender<T>,Receiver<T>) {
        let channel = Arc::new(Self {
            data: AtomicQueue::new(),
        });
        (Sender::new(channel.clone()),Receiver::new(channel.clone()))
    }
}


/// 发送端,用于发送数据,一次只能发送一个数据
pub struct Sender<T>{
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T>{
    pub fn new(channel: Arc<Channel<T>>) -> Self {
        Self { channel}
    }

    pub async fn send(&mut self, data: T)->Result<(),(QueueError,T)>{
        let mut backoff=1u32;
        let mut data=data;
        loop{
            match self.channel.data.try_push(data){
                    Ok(_)=>{
                        backoff=0;
                        break;
                    },
                    Err((QueueError::CasFailed,re_d))=>{
                        data=re_d;
                        for _ in 0..backoff {
                            std::hint::spin_loop();
                        }
                        if backoff < 64 {
                            backoff *= 2;
                        }else{
                            AsyncYield::new().await;
                        }
                    },
                    Err((QueueError::QueueNotRun,re_d))=>{
                        return Err((QueueError::QueueNotRun,re_d));
                    }
                    Err(_)=>{
                        break
                    }
            }
        }
        Ok(())
    }
}

impl<T> Unpin for Sender<T> {}

impl <T> Clone for Sender<T>{
    fn clone(&self) -> Self {
        Self { channel: self.channel.clone()}
    }
}


/// 接收端,用于接收数据,一次只能接收一个数据
pub struct Receiver<T>{
    channel: Arc<Channel<T>>,
}

impl<T> Receiver<T>{
    pub fn new(channel: Arc<Channel<T>>) -> Self {
        Self { channel}
    }

    pub async fn recv(&mut self) -> Result<Option<T>,QueueError>{
        let mut backoff=1u32;
        loop{
            match self.channel.data.try_pop(){
                    Ok(data)=>{
                        return Ok(data);
                    },
                    Err(QueueError::CasFailed)=>{
                        for _ in 0..backoff {
                            std::hint::spin_loop();
                        }
                        if backoff < 64 {
                            backoff *= 2;
                        }else{
                            AsyncYield::new().await;
                        }
                    },
                    Err(QueueError::QueueNotRun)=>{
                        return Err(QueueError::QueueNotRun);
                    }
                    Err(_)=>{}
            }
        }
    }
}

impl<T> Unpin for Receiver<T> {}
