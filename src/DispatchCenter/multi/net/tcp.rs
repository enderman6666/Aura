use mio::{
    net::{TcpStream,TcpListener,},
    Interest,
    event::Source,
    Token,
};
use std::{
    sync::{
        Arc,
        atomic::{
            AtomicPtr,AtomicBool,Ordering
        }
    },
    pin::Pin,
    task::{Context,Waker,Poll},
    io::Write,
    cell::RefCell,
};
use futures::{Stream,StreamExt};
use super::{evenloop::EventLoop,stream::{StreamType,StreamRead,StreamWrite}};

//TCP服务器，用于监听连接
pub struct TcpServer{
    listen:AtomicPtr<TcpListener>,
    addr:std::net::SocketAddr,
    token:Token,
    running:AtomicBool,
    waker:Option<Waker>,
    reg:RefCell<bool>,
}



impl TcpServer{
    pub fn bind(addr:std::net::SocketAddr) -> Self {
        let token = EventLoop::get().get_token();
        let listen = TcpListener::bind(addr).unwrap();
        Self {
            listen:AtomicPtr::new(Box::into_raw(Box::new(listen))),
            addr,
            token,
            running:AtomicBool::new(true),
            waker:None,
            reg:RefCell::new(false),
        }
    }

    pub fn get_token(&self) -> Token {
        self.token
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }

}

impl Drop for TcpServer{
    fn drop(&mut self){
        unsafe{
            drop(Box::from_raw(self.listen.load(Ordering::Acquire)));
        }
    }
}

impl Stream for TcpServer{
    type Item = Result<(TcpStreamExt,std::net::SocketAddr),std::io::Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let running = self.is_running();
        let mut token=self.token;
        if !running {
            if *self.reg.borrow() {
                EventLoop::get().deregister(&mut token, unsafe { &mut *self.listen.load(Ordering::Acquire) });
                *self.reg.borrow_mut() = false;
            }
            return Poll::Ready(None);
        }
        let listen = unsafe { &mut *self.listen.load(Ordering::Acquire) };
        match listen.accept() {
            Ok((stream,addr)) => {
                self.set_running(true);
                if !*self.reg.borrow() {
                    EventLoop::get().register(listen, Interest::READABLE,self.get_token(), cx.waker().clone());
                    *self.reg.borrow_mut() = true;
                }
                Poll::Ready(Some(Ok((TcpStreamExt::new(stream),addr))))
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock => {
                        if !*self.reg.borrow() {
                            EventLoop::get().register(listen, Interest::READABLE,self.get_token(), cx.waker().clone());
                            *self.reg.borrow_mut() = true;
                        }
                        self.waker = Some(cx.waker().clone());
                        Poll::Pending
                    }
                    _ => {
                        Poll::Ready(Some(Err(e)))
                    }
                }
            }
        }
    }
}



//用于主动发起连接
struct TcpConnet{
    stream:AtomicPtr<Option<TcpStream>>,
    addr:std::net::SocketAddr,
    token:Token,
    waker:Option<Waker>,
    reg:RefCell<bool>,
}

impl TcpConnet{
    pub fn bind(addr:std::net::SocketAddr) -> Result<Self,std::io::Error> {
        let stream = TcpStream::connect(addr)?;
        Ok(Self {
            stream:AtomicPtr::new(Box::into_raw(Box::new(Some(stream)))),
            addr,
            token: EventLoop::get().get_token(),
            waker:None,
            reg:RefCell::new(false),
        })
    }
    pub fn get_token(&self) -> Token {
        self.token
    }
}

impl Drop for TcpConnet{
    fn drop(&mut self){
        unsafe{
            drop(Box::from_raw(self.stream.load(Ordering::Acquire)));
        }
    }
}

impl Future for TcpConnet{
    type Output = Result<TcpStreamExt,std::io::Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut token = self.token;
        let stream = unsafe { &mut *self.stream.load(Ordering::Acquire) }.as_mut().unwrap();
        let error=stream.take_error();
        if let Err(e) = error {
            if *self.reg.borrow_mut() {
                *self.reg.borrow_mut() = false;
                EventLoop::get().deregister(&mut token, stream);
            }
            return Poll::Ready(Err(e));
        }
        match stream.write(&[]) {
            Ok(_) => {
                if *self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = false;
                    EventLoop::get().deregister(&mut token, stream);
                }
                let stream = unsafe{
                    let stream = self.stream.swap(Box::into_raw(Box::new(None)), Ordering::Acquire);
                    (*stream).take().unwrap()
                };
                Poll::Ready(Ok(TcpStreamExt::new(stream)))
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock => {
                        self.waker = Some(cx.waker().clone());
                        if !*self.reg.borrow() {
                            EventLoop::get().register(stream, Interest::READABLE,token, cx.waker().clone());
                            *self.reg.borrow_mut() = true;
                        }
                        Poll::Pending
                    }
                    std::io::ErrorKind::NotConnected => {
                        if !*self.reg.borrow() {
                            EventLoop::get().register(stream, Interest::WRITABLE,token, cx.waker().clone());
                            *self.reg.borrow_mut() = true;
                        }
                        self.waker = Some(cx.waker().clone());
                        Poll::Pending
                    }
                    _ => {
                        if *self.reg.borrow_mut() {
                            *self.reg.borrow_mut() = false;
                            EventLoop::get().deregister(&mut token, stream);
                        }
                        Poll::Ready(Err(e))
                    }
                }
            }
        }
    }
}

//TCP流扩展，用于提供流式接受和发送
pub struct TcpStreamExt{
    stream:AtomicPtr<TcpStream>,
    token:Token,
}

impl TcpStreamExt{
    pub fn new(stream:TcpStream) -> Self {
        Self {
            stream:AtomicPtr::new(Box::into_raw(Box::new(stream))),
            token: EventLoop::get().get_token(),
        }
    }

    pub fn send(&self,buf:Vec<u8>) -> StreamWrite {
        StreamWrite::new(StreamType::Tcp(self.stream.load(Ordering::Acquire).into()),self.token,buf)
    }

    pub fn recv(&self) -> StreamRead {
        StreamRead::new(StreamType::Tcp(self.stream.load(Ordering::Acquire).into()),self.token)
    }
}

impl Drop for TcpStreamExt{
    fn drop(&mut self){
        unsafe{
            drop(Box::from_raw(self.stream.load(Ordering::Acquire)));
        }
    }
}

 #[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::scheduler::Scheduler;
    use log::{info,LevelFilter};
        fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("Aura::DispatchCenter::multi::scheduler", LevelFilter::Trace) 
            .filter_module("Aura::DispatchCenter::multi::runtime", LevelFilter::Off)
            .filter_module("Aura::DispatchCenter::multi::net::tcp", LevelFilter::Trace)
            .filter_module("Aura::DispatchCenter::multi::net::stream", LevelFilter::Trace)
            .filter_module("mio", LevelFilter::Warn)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }

    #[test]
    fn test_tcp_server() {
        use futures::StreamExt;
        setup_logger(LevelFilter::Trace);
        Scheduler::new(2);
        let mut listener = TcpServer::bind("127.0.0.1:6666".parse().unwrap());//绑定监听端口
        let connet = TcpConnet::bind("127.0.0.1:6666".parse().unwrap()).unwrap();//连接服务器
        let s="hello world".to_string();
        let future1=async move {
            while let Some(Ok((stream,addr))) = listener.next().await {
                info!("accept: {:?}", addr);
                let future2=async move {
                    let mut data = Vec::new();
                    while let Some(d) = stream.recv().next().await {
                        match d {
                            Ok(d) => {
                                data.extend(d);
                            }
                            Err(e) => {
                                info!("recv error: {:?}", e);
                            }
                        }
                    }
                    info!("close: {:?}", addr);
                    data.into_iter().map(|x| x as char).collect::<String>()
                };
                let data = Scheduler::spawn(future2).await;
                info!("recv: {:?}", data);
                assert_eq!(data, "hello world");
            }
        };
        let future2=async move {
            let c = connet.await;
            match c {
                Ok(stream) => {
                    info!("connect success");
                    while let Some(data) = stream.send(s.clone().into_bytes()).next().await {
                        info!("send: {:?}", data);
                    }
                }
                Err(e) => {
                    info!("connect error: {:?}", e);
                }
            }
        };
        Scheduler::add(vec![Box::pin(future1),Box::pin(future2)]);
        Scheduler::join();
    }
}
