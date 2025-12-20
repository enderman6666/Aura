use mio::{Poll, Events, Token};
use std::io;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};

use super::token_center::builder::TokenCenter;
use super::multi_net::{MutexIO, NetRead, NetWrite, ReadFrom, WriteTo};


//多线程模式的事件中心
pub struct MultiCenter<T> {
    poll: mio::Poll,
    token_center: TokenCenter,
    IO_pool: Arc<Mutex<HashMap<Token, MutexIO<T>>>>,
}

impl<T> MultiCenter<T> {
    pub fn new(token_center: TokenCenter) -> Self {
        Self {
            poll: mio::Poll::new().unwrap(),
            token_center,
            IO_pool: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    //处理事件
    pub fn event_handle(&mut self) -> io::Result<()> {
        let mut events=Events::with_capacity(1024);
        self.poll.poll(&mut events, Some(Duration::from_millis(0)))?;
        let mut wakers:Vec<Waker> = Vec::new();
        for event in &events {
            let token = event.token();
            let pool = self.IO_pool.lock().unwrap();
            let io = pool.get(&token).unwrap().clone();
            match io {
                MutexIO::Read(net_read) => {
                    wakers.push(net_read.waker.lock().unwrap().take().unwrap());
                }
                MutexIO::Write(net_write) => {
                    wakers.push(net_write.waker.lock().unwrap().take().unwrap());
                }
                MutexIO::ReadFrom(net_read_from) => {
                    wakers.push(net_read_from.waker.lock().unwrap().take().unwrap());
                }
                MutexIO::WriteTo(net_write_to) => {
                    wakers.push(net_write_to.waker.lock().unwrap().take().unwrap());
                }
            }
        }
        for waker in wakers {
            waker.wake();
        }
        Ok(())
    }
}