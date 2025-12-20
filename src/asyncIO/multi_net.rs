use mio::{Events, Interest,Token,net::{TcpStream, UdpSocket, TcpListener}};
use std::{io::{self, Read, Write}, 
    future::Future,
    pin::Pin,
    task::{Context,Poll,Waker},
    collections::HashMap,
    sync::{Arc, Mutex},
    net::SocketAddr,
    time::{Duration, Instant},
};

use super::multi_center::MultiCenter;


pub enum NetType {
    TcpStream(Box<TcpStream>),
    UdpSocket(Box<UdpSocket>),
    TcpListener(Box<TcpListener>),
}



pub enum MutexIO<T> {
    Read(NetRead<T>),
    Write(NetWrite<T>),
    ReadFrom(ReadFrom<T>),
    WriteTo(WriteTo<T>),
}


pub struct MultiNet<T> {
    center: Arc<Mutex<MultiCenter<T>>>,
    net_type: Arc<Mutex<NetType>>,
    listener: Arc<Mutex<Option<(HashMap<SocketAddr, MultiNet<T>>, bool)>>>,
}

impl<T> MultiNet<T> {
    fn new(net_type: NetType, center: Arc<Mutex<MultiCenter<T>>>) -> Self {
        Self {
            center,
            net_type: Arc::new(Mutex::new(net_type)),
            listener: Arc::new(Mutex::new(None)),
        }
    }

    pub fn TcpListener(add:SocketAddr,center:Arc<Mutex<MultiCenter<T>>>) -> Result<MultiNet<T>, io::Error> {
        let listener = if let Ok(listener) = TcpListener::bind(add) {
            listener
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("TcpListener bind failed at {}", add)));
        };
        let mulitnet=MultiNet::new(
            NetType::TcpListener(Box::new(listener)), 
            center
        );
        mulitnet.listener.lock().unwrap().replace((HashMap::new(), true));
        Ok(mulitnet)
    }

    pub fn TcpStream(&mut self, ip: SocketAddr) -> Result<MultiNet<T>, io::Error>{
        let stream = if let Ok(stream) = TcpStream::connect(ip) {
            stream
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("TcpStream connect failed at {}", ip)));
        };
        self.build_stream(stream)
    }

    pub fn UdpSocket(&mut self, ip: SocketAddr) -> Result<MultiNet<T>, io::Error>{
        let socket = if let Ok(socket) = UdpSocket::bind(ip) {
            socket
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("UdpSocket bind failed at {}", ip)));
        };
        let mulitnet=MultiNet::new(
            NetType::UdpSocket(Box::new(socket)), 
            self.center.clone()
        );
        Ok(mulitnet)
    }

    pub fn connect(&mut self, ip: SocketAddr) {
        if let NetType::UdpSocket(socket) = &mut *self.net_type.lock().unwrap() {
            socket.connect(ip).unwrap();
        }
    }

    pub fn build_stream(&mut self,stream:TcpStream) -> Result<MultiNet<T>, io::Error>{
        let mulitnet=MultiNet::new(
            NetType::TcpStream(Box::new(stream)), 
            self.center.clone()
        );
        Ok(mulitnet)
    }

    pub async fn read(&self, timeout: Duration) -> io::Result<Vec<u8>>{
        let future = Arc::new(Mutex::new(self.clone()));
        let net_read = NetRead::new(
            Arc::clone(&future),
            Arc::new(Mutex::new(None)),
            timeout,
        );
        net_read.await
    }

    pub async fn write(&self, buf: Vec<u8>,timeout: Duration) -> io::Result<usize> {
        let future = Arc::new(Mutex::new(self.clone()));
        let net_write = NetWrite::new(
            Arc::clone(&future),
            Arc::new(Mutex::new(None)),
            timeout,
            buf,
        ); 
        net_write.await
    }

    pub async fn read_from(&self, timeout: Duration) -> io::Result<(SocketAddr, Vec<u8>)> {
        let future = Arc::new(Mutex::new(self.clone()));
        let net_read_from = ReadFrom::new(
            Arc::clone(&future),
            Arc::new(Mutex::new(None)),
            timeout,
        );
        net_read_from.await
    }

    pub async fn write_to(&self, addr: SocketAddr, buf: Vec<u8>, timeout: Duration) -> io::Result<usize> {
        let future = Arc::new(Mutex::new(self.clone()));
        let net_write_to = WriteTo::new(
            Arc::clone(&future),
            Arc::new(Mutex::new(None)),
            timeout,
            buf,
            addr,
        ); 
        net_write_to.await
    }



}

impl<T> Clone for MultiNet<T> {
    fn clone(&self) -> Self {
        Self {
            center: self.center.clone(),
            net_type: self.net_type.clone(),
            listener: self.listener.clone(),
        }
    }
}


pub struct NetRead<T> {
    future:Arc<Mutex<MultiNet<T>>>,
    pub waker: Arc<Mutex<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
}

impl<T> NetRead<T> {
    fn new(future:Arc<Mutex<MultiNet<T>>>, waker: Arc<Mutex<Option<Waker>>>, timeout: Duration) -> Self {
        Self {
            future,
            waker,
            time: Instant::now(),
            timeout,
        }
    }
}

impl <T>Future for NetRead<T> {
    type Output = io::Result<Vec<u8>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "读取超时")));
        }
        let waker = cx.waker().clone();
        self.waker.lock().unwrap().replace(waker);
        let future = Arc::clone(&self.future);
        let future = future.lock().unwrap();
        let mut buf = vec![0u8; 1024];
        match &mut *future.net_type.lock().unwrap() {
            NetType::TcpStream(stream) => {
                match stream.read(&mut buf) {
                    Ok(0) => Poll::Pending,
                    Ok(_) => Poll::Ready(Ok(buf)),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            NetType::UdpSocket(socket) => {
                match socket.recv(&mut buf) {
                    Ok(0) => Poll::Pending,
                    Ok(_) => Poll::Ready(Ok(buf)),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            NetType::TcpListener(listener) => {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        let mulitnet=MultiNet::new(
                            NetType::TcpStream(Box::new(stream)), 
                            future.center.clone()
                        );
                        self.future.lock().unwrap().listener.lock().unwrap().as_mut().unwrap().0.insert(addr, mulitnet);
                        Poll::Ready(Ok(buf))
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        }
    }
}

pub struct NetWrite<T> {
    future:Arc<Mutex<MultiNet<T>>>,
    pub waker: Arc<Mutex<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
    buf: Vec<u8>,
}

impl<T> NetWrite<T> {
    fn new(future:Arc<Mutex<MultiNet<T>>>, waker: Arc<Mutex<Option<Waker>>>, timeout: Duration, buf: Vec<u8>) -> Self {
        Self {
            future,
            waker,
            time: Instant::now(),
            timeout,
            buf,
        }
    }
}

impl <T>Future for NetWrite<T> {
    type Output = io::Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "写入超时")));
        }
        let waker = cx.waker().clone();
        self.waker.lock().unwrap().replace(waker);
        let future = Arc::clone(&self.future);
        let mut future = future.lock().unwrap();
        match &mut *future.net_type.lock().unwrap() {
            NetType::TcpStream(stream) => {
                match stream.write(&self.buf) {
                    Ok(0) => Poll::Pending,
                    Ok(_) => Poll::Ready(Ok(self.buf.len())),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            NetType::UdpSocket(socket) => {
                match socket.send(&self.buf) {
                    Ok(0) => Poll::Pending,
                    Ok(_) => Poll::Ready(Ok(self.buf.len())),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            NetType::TcpListener(listener) => {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        let mulitnet=MultiNet::new(
                            NetType::TcpStream(Box::new(stream)), 
                            future.center.clone()
                        );
                        self.future.lock().unwrap().listener.lock().unwrap().as_mut().unwrap().0.insert(addr, mulitnet);
                        Poll::Ready(Ok(self.buf.len()))
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        }
    }
}

pub struct ReadFrom<T> {
    future:Arc<Mutex<MultiNet<T>>>,
    pub waker: Arc<Mutex<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
}

impl <T> ReadFrom<T> {
    fn new(future:Arc<Mutex<MultiNet<T>>>, waker: Arc<Mutex<Option<Waker>>>, timeout: Duration) -> Self {
        Self {
            future,
            waker,
            time: Instant::now(),
            timeout,
        }
    }
}

impl <T> Future for ReadFrom<T> {
    type Output = io::Result<(SocketAddr,Vec<u8>)>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "读取超时")));
        }
        let waker = cx.waker().clone();
        self.waker.lock().unwrap().replace(waker);
        let future = Arc::clone(&self.future);
        let future = future.lock().unwrap();
        if let NetType::UdpSocket(socket) = &mut *future.net_type.lock().unwrap() {
            let mut buf = Vec::new();
            match socket.recv_from(&mut buf) {
                Ok((0, _)) => Poll::Pending,
                Ok((_,addr)) => Poll::Ready(Ok((addr,buf))),
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }else{
            Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "不是UdpSocket")))
        }
    }
}

pub struct WriteTo<T> {
    future:Arc<Mutex<MultiNet<T>>>,
    pub waker: Arc<Mutex<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
    buf: Vec<u8>,
    addr: SocketAddr,
}

impl <T> WriteTo<T> {
    fn new(future:Arc<Mutex<MultiNet<T>>>, waker: Arc<Mutex<Option<Waker>>>, timeout: Duration, buf: Vec<u8>, addr: SocketAddr) -> Self {
        Self {
            future,
            waker,
            time: Instant::now(),
            timeout,
            buf,
            addr,
        }
    }
}

impl <T> Future for WriteTo<T> {
    type Output = io::Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "写入超时")));
        }
        let waker = cx.waker().clone();
        self.waker.lock().unwrap().replace(waker);
        let future = Arc::clone(&self.future);
        let mut future = future.lock().unwrap();
        if let NetType::UdpSocket(socket) = &mut *future.net_type.lock().unwrap() {
            match socket.send_to(&self.buf, self.addr) {
                Ok(0) => Poll::Pending,
                Ok(_) => Poll::Ready(Ok(self.buf.len())),
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }else{
            Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "不是UdpSocket")))
        }
    }
}
