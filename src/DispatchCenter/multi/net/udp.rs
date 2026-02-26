use mio::{Token,net::UdpSocket,Interest};
use  std::{
    pin::Pin,
    task::{Context,Poll,Waker},
    sync::{atomic::{AtomicPtr,Ordering}},
    cell::RefCell,
};
use futures::{Stream,StreamExt};
use crate::DispatchCenter::multi::net::evenloop::EventLoop;
use super::stream::{StreamType,StreamWrite,StreamRead};
use log::info;

//UDP监听器
struct UdpHandeler{
    socket:AtomicPtr<UdpSocket>,
    addr:std::net::SocketAddr,
    token:Token,
    waker:Option<Waker>,
}

impl UdpHandeler{
    pub fn bind(addr:std::net::SocketAddr) -> Result<Self,std::io::Error> {
        let socket = UdpSocket::bind(addr)?;
        Ok(Self {
            socket:AtomicPtr::new(Box::into_raw(Box::new(socket))),
            addr,
            token: EventLoop::get().get_token(),
            waker:None,
        })
    }

    //连接到一个地址，后续发送的数据都将发送到这个地址
    //无法接收和发送其他地址的数据
    pub fn connect(&self,addr:std::net::SocketAddr) -> Result<(),std::io::Error> {
        unsafe { &mut *self.socket.load(Ordering::Acquire) }.connect(addr)?;
        Ok(())
    }

    //创建一个接收流
    pub fn recv_from(&self) -> UdpRecvFrom<'_> {
        UdpRecvFrom::new(self)
    }

    //创建一个发送流
    pub fn send_to(&self,ip:std::net::SocketAddr,buf:Vec<u8>) -> UdpSendTo<'_> {
        UdpSendTo::new(self,ip,buf)
    }

    //创建一个发送流,在使用connect后,才可使用
    pub fn recv(&self) -> StreamRead {
        StreamRead::new(StreamType::Udp(self.socket.load(Ordering::Acquire).into()),self.token)
    }

    //创建一个发送流,在使用connect后,才可使用
    pub fn send(&self,buf:Vec<u8>) -> StreamWrite {
        StreamWrite::new(StreamType::Udp(self.socket.load(Ordering::Acquire).into()),self.token,buf)
    }
}

impl Drop for UdpHandeler{
    fn drop(&mut self){
        unsafe{
            drop(Box::from_raw(self.socket.load(Ordering::Acquire)));
        }
    }
}

struct UdpRecvFrom<'a>{
    handeler: &'a UdpHandeler,
    erro:RefCell<bool>,
    buf:Vec<u8>,
    waker:Option<Waker>,
    reg:RefCell<bool>,
}

impl<'a> UdpRecvFrom<'a>{
    pub fn new(handeler:&'a UdpHandeler) -> Self {
        Self {
            handeler,
            erro:RefCell::new(false),
            buf:Vec::new(),    
            waker:None,
            reg:RefCell::new(false),
        }
    }
}

impl Stream for UdpRecvFrom<'_>{
    type Item = Result<(Vec<u8>,std::net::SocketAddr),std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        let mut token=self.handeler.token;
        if *self.erro.borrow_mut() {
            return Poll::Ready(None);
        }
        let result = unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }.recv_from(&mut self.buf);
        match result {
            Ok((0,_)) => {
                if *self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = false;
                    EventLoop::get().deregister( &mut token,unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) });  
                }
                Poll::Ready(None)
            }
            Ok((len,addr)) => {
                if !*self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = true;
                    EventLoop::get().register( unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }, Interest::READABLE, token, cx.waker().clone());
                }
                Poll::Ready(Some(Ok((self.buf[..len].to_vec(),addr))))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !*self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = true;
                    EventLoop::get().register( unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }, Interest::READABLE, token, cx.waker().clone());
                }
                Poll::Pending
            }
            Err(e) => {
                *self.erro.borrow_mut() = true;
                if *self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = false;
                    EventLoop::get().deregister( &mut token,unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) });  
                }
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}

struct UdpSendTo<'a>{
    handeler: &'a UdpHandeler,
    buf:Vec<u8>,
    reg:RefCell<bool>,
    waker:Option<Waker>,
    ip:std::net::SocketAddr,
    erro:RefCell<bool>,
}

impl<'a> UdpSendTo<'a>{
    pub fn new(handeler:&'a UdpHandeler,ip:std::net::SocketAddr,buf:Vec<u8>) -> Self {
        let len=buf.len();
        Self {
            handeler,
            buf,
            waker:None,
            ip,
            erro:RefCell::new(false),
            reg:RefCell::new(false),
        }
    }
}

impl Stream for UdpSendTo<'_>{
    type Item = Result<usize,std::io::Error>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        let ip = self.ip;
        let mut token=self.handeler.token;
        if *self.erro.borrow_mut() {
            return Poll::Ready(None);
        }
        let result = unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }.send_to(&self.buf,ip);
        match result {
            Ok(n) => {
                self.buf.drain(..n);
                if self.buf.is_empty() {
                    if *self.reg.borrow_mut() {
                        *self.reg.borrow_mut() = false;
                        EventLoop::get().deregister( &mut token,unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) });  
                    }
                    return Poll::Ready(None);
                }else{
                    if !*self.reg.borrow_mut() {
                        *self.reg.borrow_mut() = true;
                        EventLoop::get().register( unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }, Interest::WRITABLE, token, cx.waker().clone());
                    }
                    Poll::Ready(Some(Ok(n)))
                }

            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !*self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = true;
                    EventLoop::get().register( unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) }, Interest::WRITABLE, token, cx.waker().clone());
                }
                Poll::Pending
            }
            Err(e) => {
                *self.erro.borrow_mut() = true;
                if *self.reg.borrow_mut() {
                    *self.reg.borrow_mut() = false;
                    EventLoop::get().deregister( &mut token,unsafe { &mut *self.handeler.socket.load(Ordering::Acquire) });  
                }
                Poll::Ready(Some(Err(e)))
            }
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
            .filter_module("Aura::DispatchCenter::multi::net::udp", LevelFilter::Trace)
            .filter_module("Aura::DispatchCenter::multi::net::stream", LevelFilter::Trace)
            .filter_module("mio", LevelFilter::Warn)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
     #[test]
    fn test_udp_stream_ext() {
        setup_logger(LevelFilter::Trace);
        let target_addr = "192.168.31.108:6666".parse().unwrap();
        let bind_addr = "0.0.0.0:6666".parse().unwrap();
        Scheduler::new(1);
        let connet = UdpHandeler::bind(bind_addr).unwrap();
        let s="hello world".to_string();
        let future=async move {
            while let Some(n) = connet.send_to(target_addr,s.clone().into_bytes(),).next().await {
                info!("send: {:?}", n);
            }
        };
        Scheduler::spawn(future);
        Scheduler::join();
    }

    #[test]
    fn test_udp_stream_recv() {
        setup_logger(LevelFilter::Trace);
        let bind_addr = "0.0.0.0:6666".parse().unwrap();
        Scheduler::new(1);
        let connet = UdpHandeler::bind(bind_addr).unwrap();
        let future=async move {
            while let Some(n) = connet.recv().next().await {
                info!("recv: {:?}", n.unwrap().into_iter().map(|x| x as char).collect::<Vec<_>>());
                break
            }
        };
        Scheduler::spawn(future);
        Scheduler::join();
    }
}
