use mio::{Events, Interest,Token,net::{TcpStream, UdpSocket, TcpListener}};
use std::{io, 
    future::Future,
    pin::Pin,
    task::{Context,Poll,Waker},
    collections::HashMap,
    rc::Rc,
    cell::RefCell,
    time::{Duration, Instant},
};

use crate::asyncIO::token_center::builder::TokenCenter;
use super::async_net::{NetRead, NetWrite, ReadFrom, WriteTo};



pub enum NetIO<T: 'static> {
    Read(NetRead<T>),
    Write(NetWrite<T>),
    ReadFrom(ReadFrom<T>),
    WriteTo(WriteTo<T>),
}

pub enum NetType {
    TcpStream(Rc<RefCell<TcpStream>>),
    UdpSocket(Rc<RefCell<UdpSocket>>),
    TcpListener(Rc<RefCell<TcpListener>>),
}

impl Clone for NetType {
    fn clone(&self) -> Self {
        match self {
            NetType::TcpStream(stream) => NetType::TcpStream(stream.clone()),
            NetType::UdpSocket(stream) => NetType::UdpSocket(stream.clone()),
            NetType::TcpListener(listener) => NetType::TcpListener(listener.clone()),
        }
    }
}

// 单例中心，用于管理所有的I/O操作
pub struct SingleCenter<T: 'static> {
    poll: Rc<RefCell<mio::Poll>>,//事件轮询器
    pub IO_pool: Rc<RefCell<HashMap<Token, NetIO<T>>>>,//IOfuture池
    pub token_center: TokenCenter,//令牌中心
    waker: Rc<RefCell<Option<Waker>>>,//任务唤醒器
    run: bool,//是否运行
}

impl <T: 'static> SingleCenter<T> {
    pub fn new() -> Self {
        Self {
            poll: Rc::new(RefCell::new(mio::Poll::new().unwrap())),
            IO_pool: Rc::new(RefCell::new(HashMap::new())),
            token_center: TokenCenter::new(),
            waker: Rc::new(RefCell::new(None)),
            run: true,//是否运行
        }
    }

    pub fn register(&mut self, token: Token, stream: &mut NetType, interest: Interest) -> io::Result<()> {
        match stream {
            NetType::TcpStream(stream) => {
                self.poll.borrow_mut().registry().register(&mut *stream.borrow_mut(), token, interest)?;
                return Ok(());
            }
            NetType::UdpSocket(stream) => {
                self.poll.borrow_mut().registry().register(&mut *stream.borrow_mut(), token, interest)?;
                return Ok(());
            }
            NetType::TcpListener(listener) => {
                self.poll.borrow_mut().registry().register(&mut *listener.borrow_mut(), token, interest)?;
                return Ok(());
            }
        }
    }

    pub fn stop(&mut self) {
        self.run = false;
    }
}

impl <T>Future for SingleCenter<T> {
    type Output = io::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.run {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "调度中心已关闭")));
        }
        let waker = cx.waker().clone();
        self.waker.borrow_mut().replace(waker);
        let mut events = Events::with_capacity(1024);
        self.poll.borrow_mut().poll(&mut events, Some(Duration::from_millis(0)))?;
        let mut wakers: Vec<Waker> = Vec::new();
        for event in events.iter() {
            let token = event.token();
            let mut pool = self.IO_pool.borrow_mut();
            let future = pool.get_mut(&token);
            if let Some(net_io) = future{
                match net_io{
                    NetIO::Read(net_read) => {
                        if Instant::now() - net_read.time >= net_read.timeout {
                            if let Some(waker) = net_read.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        } else {
                            if let Some(waker) = net_read.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        }
                    }
                    NetIO::Write(net_write) => {
                        if Instant::now() - net_write.time >= net_write.timeout {
                            if let Some(waker) = net_write.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        } else {
                            if let Some(waker) = net_write.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        }
                    }
                    NetIO::ReadFrom(read_from) => {
                        if Instant::now() - read_from.time >= read_from.timeout {
                            if let Some(waker) = read_from.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        } else {
                            if let Some(waker) = read_from.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        }
                    }
                    NetIO::WriteTo(write_to) => {
                        if Instant::now() - write_to.time >= write_to.timeout {
                            if let Some(waker) = write_to.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        } else {
                            if let Some(waker) = write_to.waker.borrow_mut().take() {
                                wakers.push(waker);
                            }
                        }
                    }
                }
            }
        }
        wakers.iter().for_each(|waker| waker.wake_by_ref());
        Poll::Pending
    }
}