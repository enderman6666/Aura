use mio::{Events, Interest,Token,net::{TcpStream, UdpSocket, TcpListener}};
use std::{io::{self, Read, Write}, 
    future::Future,
    pin::Pin,
    task::{Context,Poll,Waker},
    collections::HashMap,
    rc::Rc,
    cell::RefCell,
    net::SocketAddr,
    time::{Duration, Instant},
};

use super::token_center::builder::TokenCenter;


enum NetType {
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

pub struct SingleCenter<T: 'static> {
    poll: Rc<RefCell<mio::Poll>>,//事件轮询器
    IO_pool: Rc<RefCell<HashMap<Token, NetIO<T>>>>,//IOfuture池
    token_center: TokenCenter,//令牌中心
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

    fn register(&mut self, token: Token, stream: &mut NetType, interest: Interest) -> io::Result<()> {
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
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
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

impl <T>Clone for SingleCenter<T> {
    fn clone(&self) -> Self {
        Self {
            poll: self.poll.clone(),
            IO_pool: self.IO_pool.clone(),
            token_center: self.token_center.clone(),
            waker: self.waker.clone(),
            run: self.run,
        }
    }
}


pub struct SingleIO<T: 'static> {
    center: Rc<RefCell<SingleCenter<T>>>,//中心
    stream: Option<NetType>,//流
    netmap: Option<Rc<RefCell<(HashMap<SocketAddr, SingleIO<T>>, bool)>>>,//存储连接，是否监听
}

impl <T: 'static> SingleIO<T> {
    fn new( center: Rc<RefCell<SingleCenter<T>>>, netmap: Option<Rc<RefCell<(HashMap<SocketAddr, SingleIO<T>>, bool)>>>) -> Self {
        Self {
            center,
            stream: None,
            netmap,
        }
    }

    pub fn TcpListner(ip: SocketAddr, center: Rc<RefCell<SingleCenter<T>>>) -> Result<SingleIO<T>, io::Error>{
        let listener = if let Ok(listener) = TcpListener::bind(ip) {
            listener
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("TcpListener bind failed at {}", ip)));
        };
        let mut tcplistener=SingleIO::new( center.clone(), Some(Rc::new(RefCell::new((HashMap::new(), true)))));
        tcplistener.stream = Some(NetType::TcpListener(Rc::new(RefCell::new(listener))));
        Ok(tcplistener)
    }

    pub fn TcpStream(&mut self, ip: SocketAddr) -> Result<SingleIO<T>, io::Error>{
        let stream = if let Ok(stream) = TcpStream::connect(ip) {
            stream
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("TcpStream connect failed at {}", ip)));
        };
        Ok(self.build_stream(stream))
    }

    pub fn UdpSocket(&mut self, ip: SocketAddr) -> Result<SingleIO<T>, io::Error>{
        let socket = if let Ok(socket) = UdpSocket::bind(ip) {
            socket
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, format!("UdpSocket bind failed at {}", ip)));
        };
        let mut udpsocket = SingleIO::new( self.center.clone(), None);
        socket.connect(ip)?;
        udpsocket.stream = Some(NetType::UdpSocket(Rc::new(RefCell::new(socket))));
        Ok(udpsocket)
    }

    //主动连接UdpSocket，使用该方法后之能点对点连接
    pub fn connect(&mut self, ip: SocketAddr) -> Result<SingleIO<T>, io::Error>{
        if let Some(NetType::UdpSocket(stream)) = &mut self.stream{
            stream.borrow_mut().connect(ip)?;
        }
        Ok(self.clone())
    }

    fn build_stream(&mut self, stream:TcpStream) -> SingleIO<T>{
        let mut tcpstream = SingleIO::new( self.center.clone(), None);
        tcpstream.stream = Some(NetType::TcpStream(Rc::new(RefCell::new(stream))));
        tcpstream
    }

    pub fn stop_listener(&mut self){
        if let Some(a) = &self.netmap{
            a.borrow_mut().1 = false;
        }
    }

    pub fn start_listener(&mut self){
        if let Some(a) = &self.netmap{
            a.borrow_mut().1 = true;
        }
    }

    pub async fn read(&mut self, timeout: Duration)->io::Result<Vec<u8>>{
        let read = NetRead::new(self.clone(), timeout);
        let token = self.center.borrow_mut().token_center.get_token();
        self.center.borrow_mut().register(token, self.stream.as_mut().unwrap(), Interest::READABLE)?;
        self.center.borrow_mut().IO_pool.borrow_mut().insert(token, NetIO::Read(read));
        match self.center.borrow_mut().IO_pool.borrow_mut().get_mut(&token).unwrap(){
            NetIO::Read(read) => {
                read.await
            }
            NetIO::Write(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持读取"));
            }
            NetIO::ReadFrom(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持点对点读取"));
            }
            NetIO::WriteTo(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持读取"));
            }
        }
    }

    pub async fn write(&mut self,buf:Vec<u8>, timeout: Duration)->io::Result<usize>{
        let write = NetWrite::new(self.clone(), buf, timeout);
        let token = self.center.borrow_mut().token_center.get_token();
        self.center.borrow_mut().register(token, self.stream.as_mut().unwrap(), Interest::WRITABLE)?;
        self.center.borrow_mut().IO_pool.borrow_mut().insert(token, NetIO::Write(write));
        match self.center.borrow_mut().IO_pool.borrow_mut().get_mut(&token).unwrap(){
            NetIO::Read(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持写入"));
            }
            NetIO::Write(write) => {
                write.await
            }
            NetIO::ReadFrom(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持写入"));
            }
            NetIO::WriteTo(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持Udp非连接写入"));
            }
        }
    }

    pub async fn recv_from(&mut self, timeout: Duration)->io::Result<(SocketAddr,Vec<u8>)>{
        let read = ReadFrom::new(self.clone(), timeout);
        let token = self.center.borrow_mut().token_center.get_token();
        self.center.borrow_mut().register(token, self.stream.as_mut().unwrap(), Interest::READABLE)?;
        self.center.borrow_mut().IO_pool.borrow_mut().insert(token, NetIO::ReadFrom(read));
        match self.center.borrow_mut().IO_pool.borrow_mut().get_mut(&token).unwrap(){
            NetIO::Read(read) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅用于Udp地址返回读取"));
            }
            NetIO::Write(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持读取"));
            }
            NetIO::ReadFrom(read_from) => {
                read_from.await
            }
            NetIO::WriteTo(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持读取"));
            }
        }
    }

    pub async fn write_to(&mut self, addr: SocketAddr, buf:Vec<u8>, timeout: Duration)->io::Result<usize>{
        let write = WriteTo::new(self.clone(), timeout, buf, addr);
        let token = self.center.borrow_mut().token_center.get_token();
        self.center.borrow_mut().register(token, self.stream.as_mut().unwrap(), Interest::WRITABLE)?;
        self.center.borrow_mut().IO_pool.borrow_mut().insert(token, NetIO::WriteTo(write));
        match self.center.borrow_mut().IO_pool.borrow_mut().get_mut(&token).unwrap(){
            NetIO::Read(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持Udp非连接写入"));
            }
            NetIO::Write(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持Udp非连接写入"));
            }
            NetIO::ReadFrom(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "该操作仅支持Udp非连接写入"));
            }
            NetIO::WriteTo(write_to) => {
                write_to.await
            }
        }
    }
}


impl <T>Clone for SingleIO<T> {
    fn clone(&self) -> Self {
        Self {
            center: self.center.clone(),
            stream: self.stream.clone(),
            netmap: self.netmap.clone(),
        }
    }
}



enum NetIO<T: 'static> {
    Read(NetRead<T>),
    Write(NetWrite<T>),
    ReadFrom(ReadFrom<T>),
    WriteTo(WriteTo<T>),
}

struct  NetRead<T: 'static> {
    future:Rc<RefCell<SingleIO<T>>>,
    waker: Rc<RefCell<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
}

impl<T:'static> NetRead<T> {
    fn new(future:SingleIO<T>, timeout: Duration) -> Self {
        Self {
            future:Rc::new(RefCell::new(future)),
            waker: Rc::new(RefCell::new(None)),
            time: Instant::now(),
            timeout,
        }
    }
}

impl<T:'static> Future for NetRead<T> {
    type Output = io::Result<Vec<u8>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "读取超时")));
        }
        let waker = cx.waker().clone();
        self.waker.borrow_mut().replace(waker);
        let future = self.future.borrow_mut();
        let stream = if let Some(stream) = &future.stream{
            stream
        }else{
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "未绑定流")));
        };
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        match stream {
            NetType::TcpStream(stream) => {
                match stream.borrow_mut().read(&mut buf){
                    Ok(0) => {
                        return Poll::Pending;
                    }
                    Ok(_) => {
                        return Poll::Ready(Ok(buf));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            NetType::UdpSocket(stream) => {
                match stream.borrow_mut().recv(&mut buf){
                    Ok(0) => {
                        return Poll::Pending;
                    }
                    Ok(_) => {
                        return Poll::Ready(Ok(buf));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            NetType::TcpListener(listener) => {
                if let Some(a) = &future.netmap{
                    if !a.borrow_mut().1{
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, "已停止监听")));
                    }
                }
                match listener.borrow_mut().accept(){
                    Ok((stream, addr)) => {
                        let listener = if let Some(listener) = &future.netmap{
                            listener
                        }else{
                            return Poll::Pending;
                        };
                        listener.borrow_mut().0.insert(addr, self.future.borrow_mut().build_stream(stream));
                        return Poll::Pending;
                    }
                    Err(e) => {
                        match e.kind(){
                            io::ErrorKind::WouldBlock => {
                                return Poll::Pending;
                            }
                            _ => {
                                return Poll::Ready(Err(e));
                            }
                        }
                    }
                }
            }
        }
    }
}

struct NetWrite<T: 'static> {
    future:Rc<RefCell<SingleIO<T>>>,
    waker: Rc<RefCell<Option<Waker>>>,
    time: Instant,
    buf: Vec<u8>,
    timeout: Duration,
}

impl<T:'static> NetWrite<T> {
    fn new(future:SingleIO<T>, buf: Vec<u8>, timeout: Duration) -> Self {
        Self {
            future:Rc::new(RefCell::new(future)),
            waker: Rc::new(RefCell::new(None)),
            time: Instant::now(),
            buf,
            timeout,
        }
    }
}

impl<T:'static> Future for NetWrite<T> {
    type Output = io::Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "写入超时")));
        }
        let waker = cx.waker().clone();
        self.waker.borrow_mut().replace(waker);
        let future = self.future.borrow_mut();
        let stream = if let Some(stream) = &future.stream{
            stream
        }else{
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "未绑定流")));
        };
        match stream {
            NetType::TcpStream(stream) => {
                match stream.borrow_mut().write(&self.buf){
                    Ok(0) => {
                        return Poll::Pending;
                    }
                    Ok(n) => {
                        return Poll::Ready(Ok(n));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            NetType::UdpSocket(stream) => {
                match stream.borrow_mut().send(&self.buf){
                    Ok(0) => {
                        return Poll::Pending;
                    }
                    Ok(n) => {
                        return Poll::Ready(Ok(n));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            NetType::TcpListener(listener) => {
                if let Some(a) = &future.netmap{
                    if !a.borrow_mut().1{
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, "已停止监听")));
                    }
                }
                match listener.borrow_mut().accept(){
                    Ok((stream, addr)) => {
                        let listener = if let Some(listener) = &future.netmap{
                            listener
                        }else{
                            return Poll::Pending;
                        };
                        listener.borrow_mut().0.insert(addr, self.future.borrow_mut().build_stream(stream));
                        return Poll::Pending;
                    }
                    Err(e) => {
                        match e.kind(){
                            io::ErrorKind::WouldBlock => {
                                return Poll::Pending;
                            }
                            _ => {
                                return Poll::Ready(Err(e));
                            }
                        }
                    }
                }
            }
        }
    }
}

struct ReadFrom<T: 'static> {
    future:Rc<RefCell<SingleIO<T>>>,
    waker: Rc<RefCell<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
}

impl<T:'static> ReadFrom<T> {
    fn new(future:SingleIO<T>, timeout: Duration) -> Self {
        Self {
            future:Rc::new(RefCell::new(future)),
            waker: Rc::new(RefCell::new(None)),
            time: Instant::now(),
            timeout,
        }
    }
}

impl<T:'static> Future for ReadFrom<T> {
    type Output = io::Result<(SocketAddr,Vec<u8>)>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "读取超时")));
        }
        let waker = cx.waker().clone();
        self.waker.borrow_mut().replace(waker);
        let future = self.future.borrow_mut();
        let stream = if let Some(stream) = &future.stream{
            stream
        }else{
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "未绑定流")));
        };
        let mut buf: Vec<u8> = Vec::new();
        match stream {
            NetType::UdpSocket(stream) => {
                match stream.borrow_mut().recv_from(&mut buf){
                    Ok((0,_))=> {
                        return Poll::Pending;
                    }
                    Ok((_, addr)) => {
                        return Poll::Ready(Ok((addr,buf)));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            _ => {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "该方法只能用于UdpSocket")));
            }
        }
    }
}

struct WriteTo<T: 'static> {
    future:Rc<RefCell<SingleIO<T>>>,
    waker: Rc<RefCell<Option<Waker>>>,
    time: Instant,
    timeout: Duration,
    buf: Vec<u8>,
    addr: SocketAddr,
}

impl<T:'static> WriteTo<T> {
    fn new(future:SingleIO<T>, timeout: Duration, buf: Vec<u8>, addr: SocketAddr) -> Self {
        Self {
            future:Rc::new(RefCell::new(future)),
            waker: Rc::new(RefCell::new(None)),
            time: Instant::now(),
            timeout,
            buf,
            addr,
        }
    }
}

impl<T:'static> Future for WriteTo<T> {
    type Output = io::Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() - self.time >= self.timeout {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "写入超时")));
        }
        let waker = cx.waker().clone();
        self.waker.borrow_mut().replace(waker);
        let future = self.future.borrow_mut();
        let stream = if let Some(stream) = &future.stream{
            stream
        }else{
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "未绑定流")));
        };
        match stream {
            NetType::UdpSocket(stream) => {
                match stream.borrow_mut().send_to(&self.buf, self.addr){
                    Ok(0) => {
                        return Poll::Pending;
                    }
                    Ok(n) => {
                        return Poll::Ready(Ok(n));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                }
            }
            _ => {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "该方法只能用于UdpSocket")));
            }
        }
    }
}