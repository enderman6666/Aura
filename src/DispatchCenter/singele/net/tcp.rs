use mio::{
    net::{TcpStream,TcpListener,},
    Interest,
    event::Source,
    Token,
};
use std::io::Write;
use std::{
    net::SocketAddr,
    rc::Rc,
    cell::RefCell,
    task::{Context,Poll,Waker},
    pin::Pin,
    future::Future,
};
use futures::{Stream};
use log::debug;
use super::{evenloop::EventLoop,stream::{StreamType,StreamRead,StreamWrite}};

pub struct TcpServer{
    listener:Rc<RefCell<TcpListener>>,
    addr:SocketAddr,
    eventloop:Rc<RefCell<EventLoop>>,
    running:bool,
}

impl TcpServer{
    pub fn new(addr:SocketAddr,eventloop:Rc<RefCell<EventLoop>>) -> Result<Self,std::io::Error> {
        Ok(Self {
            addr,
            listener:Rc::new(RefCell::new(TcpListener::bind(addr)?)),
            eventloop,
            running:false,
        })
    }

    pub fn listen(&mut self) -> TcpAccept<'_> {
        self.running = true;
        TcpAccept::new(self)
    }

    pub fn stop(&mut self) {
        self.running = false;
    }
}

pub struct TcpAccept<'a>{
    server:&'a mut TcpServer,
    waker:Option<Waker>,
}

impl<'a> TcpAccept<'a>{
    pub fn new(server:&'a mut TcpServer) -> Self {
        Self {
            server,
            waker:None,
        }
    }
}

impl<'a> Stream for TcpAccept<'a>{
    type Item = Result<(SocketAddr,Rc<RefCell<TcpStream>>),std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        if !self.server.running {
            return Poll::Ready(None);
        }
        let result = {
            let listener_r = Rc::clone(&self.server.listener);
            listener_r.borrow_mut().accept()
        };
        match result {
            Ok((stream,addr)) => {
                Poll::Ready(Some(Ok((addr,Rc::new(RefCell::new(stream))))))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let listener_r = Rc::clone(&self.server.listener);
                self.server.eventloop.borrow_mut().register(listener_r,Interest::READABLE,self.waker.clone().unwrap());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Some(Err(e))),
        }
    }
}

pub struct TcpConnet {
    socket: Rc<RefCell<TcpStream>>,
    eventloop:Rc<RefCell<EventLoop>>,
}

impl TcpConnet{
    pub fn new(socket:Rc<RefCell<TcpStream>>,eventloop:Rc<RefCell<EventLoop>>) -> Self {
        Self {
            socket,
            eventloop,
        }
    }

     pub async fn build(addr:SocketAddr,eventloop:Rc<RefCell<EventLoop>>) -> Result<Self,std::io::Error> {
        let stream = TcpConnectFuture::new(Rc::clone(&eventloop),addr)?.await?;
        Ok(Self::new(stream,Rc::clone(&eventloop)))
    }

    pub fn read(&self) -> StreamRead{
        StreamRead::new(StreamType::TcpStream(self.socket.clone()),Rc::clone(&self.eventloop))
    }

    pub fn write(&self,buf:Vec<u8>) -> StreamWrite{
        StreamWrite::new(StreamType::TcpStream(self.socket.clone()),Rc::clone(&self.eventloop),buf)
    }
}

pub struct TcpConnectFuture{
    eventloop:Rc<RefCell<EventLoop>>,
    stream:Rc<RefCell<TcpStream>>,
    waker:Option<Waker>,
    token:Option<Token>,
    connected:bool,
}

impl TcpConnectFuture{
    pub fn new(eventloop:Rc<RefCell<EventLoop>>,addr:SocketAddr) ->Result<Self,std::io::Error> {
        let stream = TcpStream::connect(addr)?;
        
        Ok(Self {
            eventloop,
            stream:Rc::new(RefCell::new(stream)),
            waker:None,
            token:None,
            connected:false,
        })
    }
}

impl Future for TcpConnectFuture{
    type Output = Result<Rc<RefCell<TcpStream>>,std::io::Error>;
    fn poll(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Self::Output> {
        self.waker = Some(cx.waker().clone());
        if self.token.is_none() {
            // 转换为Rc<RefCell<dyn Source>>类型
            let source: Rc<RefCell<dyn Source>> = Rc::clone(&self.stream) as Rc<RefCell<dyn Source>>;
            let token = self.eventloop.borrow_mut().register(source, Interest::WRITABLE, cx.waker().clone());
            self.token = Some(token);
        }
        let error = {
            let stream_b = self.stream.borrow_mut();
            stream_b.take_error()
        };
        
        if let Err(e) = error {
            if let Some(mut token) = self.token.take() {
                let source: Rc<RefCell<dyn Source>> = Rc::clone(&self.stream) as Rc<RefCell<dyn Source>>;
                self.eventloop.borrow_mut().deregister(&mut token, source);
            }
            return Poll::Ready(Err(e));
        }
        let write_result = {
            let mut stream_b = self.stream.borrow_mut();
            stream_b.write(&[])
        };
        
        match write_result {
            Ok(_) => {
                debug!("连接建立成功");
                if let Some(mut token) = self.token.take() {
                    let source: Rc<RefCell<dyn Source>> = Rc::clone(&self.stream) as Rc<RefCell<dyn Source>>;
                    self.eventloop.borrow_mut().deregister(&mut token, source);
                }
                Poll::Ready(Ok(Rc::clone(&self.stream)))
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Poll::Pending
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
                Poll::Pending
            },
            Err(e) => {
                if let Some(mut token) = self.token.take() {
                    let source: Rc<RefCell<dyn Source>> = Rc::clone(&self.stream) as Rc<RefCell<dyn Source>>;
                    self.eventloop.borrow_mut().deregister(&mut token, source);
                }
                Poll::Ready(Err(e))
            },
        }
    }
}
