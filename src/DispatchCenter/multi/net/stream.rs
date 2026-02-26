use mio::{
    net::{TcpStream,UdpSocket,},
    Interest,
    Token,
};
use std::{
    io::{Read,Write},
    task::{Context,Poll,Waker},
    pin::Pin,
    cell::RefCell,
    sync::atomic::{AtomicPtr,Ordering},
};
use futures::{Stream,StreamExt};
use crate::DispatchCenter::multi::net::evenloop::EventLoop;
use log::{error,info};
pub enum StreamType{
    Tcp( AtomicPtr<TcpStream>),
    Udp( AtomicPtr<UdpSocket>),
}


pub struct StreamRead{
    token:Token,
    stream:StreamType,
    waker:Option<Waker>,
    erro:RefCell<bool>,
    buf:RefCell<Vec<u8>>,
    reg:RefCell<bool>,
}

impl StreamRead{
    pub fn new(stream:StreamType,token:Token) -> Self {
        Self {
            token,
            stream,
            waker:None,
            erro:RefCell::new(false),
            buf:RefCell::new(Vec::new()),
            reg:RefCell::new(false),
        }
    }
}

impl Stream for StreamRead{
    type Item = Result<Vec<u8>,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        if *self.erro.borrow() {
            return Poll::Ready(None);
        }
        let mut token = self.token;
        match &self.stream {
            StreamType::Tcp(tcp) => {
                let mut tcp_b = unsafe { &mut *tcp.load(Ordering::Acquire) };
                match tcp_b.read(&mut *self.buf.borrow_mut()) {
                    Ok(0) => {
                        if *self.reg.borrow_mut() {
                            *self.reg.borrow_mut() = false;
                            EventLoop::get().deregister(&mut token, tcp_b);
                        }
                        Poll::Ready(None)
                    }
                    Ok(n) => {
                        if !*self.reg.borrow_mut() {
                            *self.reg.borrow_mut() = true;
                            EventLoop::get().register(tcp_b, Interest::READABLE,self.token, cx.waker().clone());
                        }
                        let data = self.buf.borrow_mut().drain(..n).collect::<Vec<_>>();
                        Poll::Ready(Some(Ok(data)))
                    }
                    Err(e)=>{
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock => {
                                if !*self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = true;
                                    EventLoop::get().register(tcp_b, Interest::READABLE,self.token, cx.waker().clone());
                                }
                                Poll::Pending
                            }
                            _ => {
                                if *self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = false;
                                    EventLoop::get().deregister(&mut token, tcp_b);
                                }
                                *self.erro.borrow_mut() = true;
                                Poll::Ready(Some(Err(e)))
                            }
                        }
                    }
                }
            }
            StreamType::Udp(udp) => {
                self.buf.borrow_mut().resize(65535, 0);
                let mut buf = self.buf.borrow_mut();
                let udp_b = unsafe { &mut *udp.load(Ordering::Acquire) };
                match udp_b.recv(&mut buf) {
                    Ok(0) => {
                        if *self.reg.borrow_mut() {
                            *self.reg.borrow_mut() = false;
                            EventLoop::get().deregister(&mut token, udp_b);
                        }
                        Poll::Ready(None)
                    }
                    Ok(n) => {
                        if !*self.reg.borrow_mut() {
                            *self.reg.borrow_mut() = true;
                            EventLoop::get().register(udp_b, Interest::READABLE,self.token, cx.waker().clone());
                        }
                        let data = buf.drain(..n).collect::<Vec<_>>();
                        Poll::Ready(Some(Ok(data)))
                    }
                    Err( e)=>{
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock => {
                                if !*self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = true;
                                    EventLoop::get().register(udp_b, Interest::READABLE,self.token, cx.waker().clone());
                                }
                                Poll::Pending
                            }
                            _ => {
                                if *self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = false;
                                    EventLoop::get().deregister(&mut token, udp_b);
                                }
                                *self.erro.borrow_mut() = true;
                                Poll::Ready(Some(Err(e)))
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct StreamWrite{
    token:Token,
    stream:StreamType,
    waker:Option<Waker>,
    buf:Vec<u8>,
    erro:RefCell<bool>,
    reg:RefCell<bool>,
}

impl StreamWrite{
    pub fn new(stream:StreamType,token:Token,buf:Vec<u8>) -> Self {
        Self {
            token,
            stream,
            waker:None,
            buf,
            erro:RefCell::new(false),
            reg:RefCell::new(false),
        }
    }
}

impl Stream for StreamWrite{
    type Item = Result<usize,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        if *self.erro.borrow() {
            return Poll::Ready(None);
        }
        match &self.stream {
            StreamType::Tcp(tcp) => {
                let tcp_b = unsafe { &mut *tcp.load(Ordering::Acquire) };
                match tcp_b.write(&self.buf) {
                    Ok(n) => {
                        let _ = self.buf.drain(..n);
                        if self.buf.is_empty() {
                            if *self.reg.borrow_mut() {
                                *self.reg.borrow_mut() = false;
                                EventLoop::get().deregister(&mut self.token, tcp_b); 
                            }
                            match tcp_b.shutdown(std::net::Shutdown::Write) {
                                Ok(_) => {
                                    Poll::Ready(None)
                                }
                                Err(e) => {
                                    if !*self.reg.borrow_mut() {
                                        *self.reg.borrow_mut() = false;
                                        EventLoop::get().deregister(&mut self.token, tcp_b);
                                    }
                                    *self.erro.borrow_mut() = true;
                                    Poll::Ready(Some(Err(e)))
                                }
                            }
                        }else{
                            if !*self.reg.borrow_mut() {
                                *self.reg.borrow_mut() = true;
                                EventLoop::get().register(tcp_b, Interest::WRITABLE,self.token, cx.waker().clone());
                            }
                            Poll::Ready(Some(Ok(n)))
                        }
                    }
                    Err(e)=>{
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock => {
                                *self.reg.borrow_mut() = true;
                                EventLoop::get().register(tcp_b, Interest::WRITABLE,self.token, cx.waker().clone());
                                Poll::Pending
                            }
                            _ => {
                                if !*self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = false;
                                    EventLoop::get().deregister(&mut self.token, tcp_b);
                                }
                                *self.erro.borrow_mut() = true;
                                Poll::Ready(Some(Err(e)))
                            }
                        }
                    }
                }
            }
            StreamType::Udp(udp) => {
                let udp_b = unsafe { &mut *udp.load(Ordering::Acquire) };
                match udp_b.send(&self.buf) {
                    Ok(n) => {
                        let _ = self.buf.drain(..n).collect::<Vec<_>>();
                        if self.buf.is_empty() {
                            if !*self.reg.borrow_mut() {
                                *self.reg.borrow_mut() = false;
                                EventLoop::get().deregister(&mut self.token, udp_b);    
                            }
                            Poll::Ready(None)
                        }else{
                            if !*self.reg.borrow_mut() {
                                *self.reg.borrow_mut() = true;
                                EventLoop::get().register(udp_b, Interest::WRITABLE,self.token, cx.waker().clone());
                            }
                            Poll::Ready(Some(Ok(n)))
                        }
                    }
                    Err( e)=>{
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock => {
                                if !*self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = true;
                                    EventLoop::get().register(udp_b, Interest::WRITABLE,self.token, cx.waker().clone());
                                }
                                Poll::Pending
                            }
                            _ => {
                                if !*self.reg.borrow_mut() {
                                    *self.reg.borrow_mut() = false;
                                    EventLoop::get().deregister(&mut self.token, udp_b);
                                }
                                *self.erro.borrow_mut() = true;
                                Poll::Ready(Some(Err(e)))
                            }
                        }
                    }
                }
            }
        }
    }
}
