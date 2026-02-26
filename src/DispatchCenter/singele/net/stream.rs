use mio::{
    net::{TcpStream,UdpSocket,},
    Interest,
    Poll as MioPoll,
    Token,
};
use std::{
    io::{Read,Write},
    task::{Context,Poll,Waker},
    pin::Pin,
    rc::Rc,
    cell::RefCell,
};
use futures::{Stream,StreamExt};
use super::evenloop::EventLoop;


// 流类型,用于表示TCP流或UDP套接字
pub enum StreamType{
    TcpStream(Rc<RefCell<TcpStream>>),
    UdpSocket(Rc<RefCell<UdpSocket>>),
}
pub struct StreamRead{
    token:Option<Token>,
    stream:StreamType,
    waker:Option<Waker>,
    eventloop:Rc<RefCell<EventLoop>>,
}

impl StreamRead{
    pub fn new(stream:StreamType,eventloop:Rc<RefCell<EventLoop>>) -> Self {
        Self {
            token:None,
            stream,
            waker:None,
            eventloop,
        }
    }
}

impl Stream for StreamRead{
    type Item = Result<Vec<u8>,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut buf = Vec::new();
        self.waker = Some(cx.waker().clone());
        match &self.stream {
            StreamType::TcpStream(tcp) => {
                let mut tcp_b = tcp.borrow_mut();
                match tcp_b.read(&mut buf) {
                    Ok(0) => {
                        Poll::Ready(None)
                    }
                    Ok(n) => {
                        let tcp_r = Rc::clone(&tcp);
                        self.eventloop.borrow_mut().register(tcp_r,Interest::READABLE,self.waker.clone().unwrap());
                        let data = buf.drain(..n).collect::<Vec<_>>();
                        Poll::Ready(Some(Ok(data)))
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        Poll::Pending
                    }
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
            StreamType::UdpSocket(socket) => {
                let socket_b = Rc::clone(&socket);
                let mut buf = Vec::with_capacity(65535);
                buf.resize(65535, 0);
                let result=socket_b.borrow_mut().recv(&mut buf);
                match result {
                    Ok(0) => {
                        let socket_r = Rc::clone(&socket);
                        self.eventloop.borrow_mut().register(socket_r,Interest::READABLE,self.waker.clone().unwrap());
                        Poll::Pending
                    }
                    Ok(n) => {
                        let data = buf.drain(..n).collect::<Vec<_>>();
                        Poll::Ready(Some(Ok(data)))
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        let socket_r = Rc::clone(&socket);
                        self.eventloop.borrow_mut().register(socket_r,Interest::READABLE,self.waker.clone().unwrap());
                        Poll::Pending
                    }
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
        }
    }
}



pub struct StreamWrite{
    token:Option<Token>,
    stream:StreamType,
    waker:Option<Waker>,
    eventloop:Rc<RefCell<EventLoop>>,
    buf:Vec<u8>,
}

impl StreamWrite{
    pub fn new(stream:StreamType,eventloop:Rc<RefCell<EventLoop>>,buf:Vec<u8>) -> Self {
        Self {
            token:None,
            stream,
            waker:None,
            eventloop,
            buf,
        }
    }
}

impl Stream for StreamWrite{
    type Item = Result<usize,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        let buf_len = self.buf.len();
        let mut buf_copy = Vec::with_capacity(buf_len);
        buf_copy.extend_from_slice(&self.buf);
        let write_result: Result<usize, std::io::Error>;
        let mut stream_clone: Option<StreamType> = None;
        match &self.stream {
            StreamType::TcpStream(tcp) => {
                { 
                    let mut tcp_b = tcp.borrow_mut();
                    write_result = tcp_b.write(&buf_copy);
                }
                stream_clone = Some(StreamType::TcpStream(Rc::clone(tcp)));
            }
            StreamType::UdpSocket(socket) => {
                { 
                    let socket_b = socket.borrow_mut();
                    write_result = socket_b.send(&buf_copy);
                }
                stream_clone = Some(StreamType::UdpSocket(Rc::clone(socket)));
            }
        }
        match write_result {
            Ok(0) => {
                if let Some(StreamType::TcpStream(tcp)) = stream_clone {
                    self.eventloop.borrow_mut().register(tcp, Interest::WRITABLE, self.waker.clone().unwrap());
                } else if let Some(StreamType::UdpSocket(socket)) = stream_clone {
                    self.eventloop.borrow_mut().register(socket, Interest::WRITABLE, self.waker.clone().unwrap());
                }
                Poll::Pending
            }
            Ok(n) => {
                self.buf.drain(..n);
                if self.buf.is_empty() {
                    Poll::Ready(None)
                } else {
                    if let Some(StreamType::TcpStream(tcp)) = stream_clone {
                        self.eventloop.borrow_mut().register(tcp, Interest::WRITABLE, self.waker.clone().unwrap());
                    } else if let Some(StreamType::UdpSocket(socket)) = stream_clone {
                        self.eventloop.borrow_mut().register(socket, Interest::WRITABLE, self.waker.clone().unwrap());
                    }
                    Poll::Ready(Some(Ok(n)))
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Poll::Pending
            }
            Err(e) => {
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}