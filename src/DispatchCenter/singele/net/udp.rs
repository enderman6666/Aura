use mio::{
    net::UdpSocket,
    Interest,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    cell::RefCell,
    task::{Context,Poll,Waker},
    pin::Pin,
};
use futures::{Stream,StreamExt};
use super::{evenloop::EventLoop,stream::{StreamType,StreamWrite,StreamRead}};


// UDP套接字处理，定义UDP专用方法 
pub struct UdpHandeler {
    socket: Rc<RefCell<UdpSocket>>,
    eventloop:Rc<RefCell<EventLoop>>,
}

impl UdpHandeler {
    pub fn new(addr: SocketAddr,eventloop:Rc<RefCell<EventLoop>>) -> Result<Self, std::io::Error>{
        Ok(Self{
            socket: Rc::new(RefCell::new(UdpSocket::bind(addr)?)),
            eventloop,
        })
    }

    pub fn connect(&self, addr: SocketAddr) -> Result<(), std::io::Error>{
        self.socket.borrow_mut().connect(addr)
    }

    pub fn read_from(&self) -> UdpRecvFrom<'_> {
        UdpRecvFrom::new(self)
    }

    pub fn write_to(&self, ip: SocketAddr, buf: Vec<u8>) -> UdpSendTo<'_> {
        UdpSendTo::new(self,ip,buf)
    }

    pub fn write(&self, buf: Vec<u8>) -> StreamWrite{
        StreamWrite::new(StreamType::UdpSocket(self.socket.clone()),self.eventloop.clone(),buf)
    }

    pub fn read(&self) -> StreamRead{
        StreamRead::new(StreamType::UdpSocket(self.socket.clone()),self.eventloop.clone())
    }
}

pub struct UdpRecvFrom<'a> {
    handeler: &'a UdpHandeler,
    waker: Option<Waker>,
    buf: Vec<u8>,
}

impl <'a> UdpRecvFrom<'a>{
    pub fn new(handeler: &'a UdpHandeler) -> Self{
        Self{
            handeler,
            waker: None,
            buf: Vec::new(),
        }
    }
}

impl <'a> Stream for UdpRecvFrom<'a>{
    type Item = Result<Vec<u8>,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        let result = self.handeler.socket.borrow_mut().recv_from(&mut self.buf);
        match result {
            Ok((len,_)) => {
                if !self.buf.is_empty(){
                    self.handeler.eventloop.borrow_mut().register(self.handeler.socket.clone(), Interest::READABLE, self.waker.clone().unwrap());
                    let buf = self.buf.drain(..len);
                    Poll::Ready(Some(Ok(buf.collect())))
                }else {
                    Poll::Ready(None)
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                self.handeler.eventloop.borrow_mut().register(self.handeler.socket.clone(), Interest::READABLE, self.waker.clone().unwrap());
                Poll::Pending
            }
            Err(e) => {
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}

pub struct UdpSendTo<'a> {
    handeler: &'a UdpHandeler,
    waker: Option<Waker>,
    ip: SocketAddr,
    buf: Vec<u8>,
}

impl <'a> UdpSendTo<'a>{
    pub fn new(handeler: &'a UdpHandeler,ip: SocketAddr,buf: Vec<u8>) -> Self{
        Self{
            handeler,
            waker: None,
            ip,
            buf
        }
    }
}

impl <'a> Stream for UdpSendTo<'a>{
    type Item = Result<usize,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        let ip = self.ip;
        let result = self.handeler.socket.borrow_mut().send_to(&self.buf, ip);
        match result {
            Ok(len) => {
                if len > 0 {
                    self.buf.drain(..len);
                    if !self.buf.is_empty() {
                        self.handeler.eventloop.borrow_mut().register(self.handeler.socket.clone(), Interest::WRITABLE, self.waker.clone().unwrap());
                        Poll::Ready(Some(Ok(len)))
                    } else {
                        Poll::Ready(None)
                    }
                } else {
                    Poll::Ready(None)
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                self.handeler.eventloop.borrow_mut().register(self.handeler.socket.clone(), Interest::WRITABLE, self.waker.clone().unwrap());
                Poll::Pending
            }
            Err(e) => {
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}
