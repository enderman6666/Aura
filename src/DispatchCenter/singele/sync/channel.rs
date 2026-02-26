use std::{
    rc::Rc,
    cell::{Cell,RefCell},
    mem::MaybeUninit,
    io::{self,Read,Write},
    collections::VecDeque,
    task::{Context,Waker,Poll},
    pin::Pin,
};
use futures::Stream;

pub struct Channel<T>{
    data:[MaybeUninit<T>;1024],
    freeindex:VecDeque<usize>,
    usedindex:VecDeque<usize>,
    sender_waker:Option<Waker>,
    receiver_waker:Option<Waker>,
}

impl<T> Channel<T>{
    fn new() -> Self {
        let data:[MaybeUninit<T>;1024]=unsafe{ MaybeUninit::uninit().assume_init()};
        let mut indexlist=VecDeque::new();
        for i in 0..1024{
            indexlist.push_back(i);
        }
        Self {
            data,
            freeindex:indexlist,
            usedindex:VecDeque::new(),
            sender_waker:None,
            receiver_waker:None,
        }
    }

    pub fn builder() -> (AsyncSender<T>, AsyncReceiver<T>,Rc<RefCell<Channel<T>>>){
        let chan=Rc::new(RefCell::new(Channel::new()));
        (AsyncSender::new(chan.clone()), AsyncReceiver::new(chan.clone()),chan.clone())
    }

    pub fn clear(&mut self){
        for index in self.usedindex.drain(..){
            let i_data=&mut self.data[index];
            unsafe{
                i_data.as_mut_ptr().write(MaybeUninit::uninit().assume_init());
            }
            self.freeindex.push_back(index);
        }
        
    }
}

pub struct AsyncSender<T>{
    chan:Rc<RefCell<Channel<T>>>,
}

impl<T> AsyncSender<T>{
    pub fn new(chan:Rc<RefCell<Channel<T>>>) -> Self {
        Self {
            chan,
        }
    }

    pub fn send(&mut self, data: T)->Result<(),String>{
        let index=if let Some(index)=self.chan.borrow_mut().freeindex.pop_front(){
            index
        }else{
            return Err("缓存已满，稍后再试".to_string());
        };
        self.chan.borrow_mut().usedindex.push_back(index);
        let i_data=&mut self.chan.borrow_mut().data[index];
        (i_data).write(data);
        Ok(())
    }

    pub fn stream_send(&mut self, data: Vec<T>) -> StreamSender<T>{
        StreamSender::new(self,data)
    }
}

pub struct AsyncReceiver<T>{
    chan:Rc<RefCell<Channel<T>>>,
}

impl<T> AsyncReceiver<T>{
    pub fn new(chan:Rc<RefCell<Channel<T>>>) -> Self {
        Self {
            chan,
        }
    }

    pub fn recv(&mut self) -> Result<T,String>{
        let index=if let Some(index)=self.chan.borrow_mut().usedindex.pop_front(){
            index
        }else{
            return Err("缓存为空，稍后再试".to_string());
        };
        let i_data=&mut self.chan.borrow_mut().data[index].as_ptr();
        unsafe{
            Ok((i_data).read())
        }
    }

    fn r_ref(&mut self) -> Result<*mut T,String>{
        let index=if let Some(index)=self.chan.borrow_mut().usedindex.pop_front(){
            index
        }else{
            return Err("缓存为空，稍后再试".to_string());
        };
        let i_data=self.chan.borrow_mut().data[index].as_mut_ptr();
        Ok(i_data)
    }

    pub fn recv_ref(&mut self) -> Result<&mut T,String>{
        let item=self.r_ref()?;
        Ok(unsafe{&mut *(item)})
    }

    pub fn stream_recv(&mut self) -> StreamReceiver<T>{
        StreamReceiver::new(self)
    }

    pub fn stream_recv_ref(&mut self) -> StreamReceiverRef<T>{
        StreamReceiverRef::new(self)
    }
}


pub struct StreamSender<'a,T>{
    sender:&'a mut  AsyncSender<T>,
    data:Rc<RefCell<Vec<T>>>,
}

impl<'a,T> StreamSender<'a,T>{
    pub fn new(sender:&'a mut AsyncSender<T>,data:Vec<T>) -> Self {
        Self {
            sender,
            data:Rc::new(RefCell::new(data)),
        }
    }
}

impl <'a,T> Stream for StreamSender<'a,T>{
    type Item = Result<(),String>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.sender.chan.borrow_mut().sender_waker=Some(cx.waker().clone());
        let freelen=self.sender.chan.borrow_mut().freeindex.len();
        let datalen=self.data.borrow().len();
        if datalen==0{
            match self.sender.chan.borrow_mut().receiver_waker.take(){
                Some(waker)=>{
                    waker.wake();
                },
                None=>{},
            };
            return Poll::Ready(None);
        }
        if freelen==0{
            match self.sender.chan.borrow_mut().receiver_waker.take(){
                Some(waker)=>{
                    waker.wake();
                },
                None=>{},
            };
            return Poll::Pending;
        }
        if freelen>=datalen{
            let data=self.data.borrow_mut().drain(..).collect::<Vec<T>>();
            for item in data{
                match self.sender.send(item){
                    Ok(_)=>{},
                    Err(_)=>{
                        match self.sender.chan.borrow_mut().receiver_waker.take(){
                            Some(waker)=>{
                                waker.wake();
                            },
                            None=>{},
                        };
                        return Poll::Pending;
                    }
                }
            }
            return Poll::Ready(Some(Ok(())));
        };
        if datalen>0&&freelen<datalen{
            let data=self.data.borrow_mut().drain(..freelen).collect::<Vec<T>>();
            for item in data{
                match self.sender.send(item){
                    Ok(_)=>{},
                    Err(_)=>{
                        return Poll::Pending;
                    }
                }
            }
            return Poll::Ready(Some(Ok(())));
        }
        Poll::Ready(Some(Err("未知错误，请检查各个条件分支是否满足".to_string())))
    }
}


pub struct StreamReceiver<'a,T>{
    receiver:&'a mut AsyncReceiver<T>,
    waker:Option<Waker>,
    n:usize,
}

impl<'a,T>StreamReceiver<'a,T>{
    pub fn new(receiver:&'a mut AsyncReceiver<T>) -> Self {
        Self {
            receiver,
            waker:None,
            n:0,  
        }
    }
}

impl<'a,T> Stream for StreamReceiver<'a,T>{
    type Item = Result<T,String>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        if self.receiver.chan.borrow_mut().usedindex.is_empty()&&self.n==0{
            match self.receiver.chan.borrow_mut().sender_waker.take(){
                Some(waker)=>{
                    waker.wake();
                },
                None=>{},
            };
            return Poll::Pending;
        }
        if !self.receiver.chan.borrow_mut().usedindex.is_empty()&&self.n==0{
            self.n=1;
        }
        let item=match self.receiver.recv(){
            Ok(item)=>Poll::Ready(Some(Result::<T, String>::Ok(item))),
            Err(_)=>{
                if self.n==0{
                    self.n=1;
                    match self.receiver.chan.borrow_mut().sender_waker.take(){
                        Some(waker)=>{
                            waker.wake();
                        },
                        None=>{},
                    };
                    return Poll::Pending;
                }else{
                    return Poll::Ready(None);
                }
            }
        };
        item
    }
}

pub struct StreamReceiverRef<'a,T>{
    receiver:&'a mut AsyncReceiver<T>,
    waker:Option<Waker>,
    n:usize,
}

impl<'a,T>StreamReceiverRef<'a,T>{
    pub fn new(receiver:&'a mut AsyncReceiver<T>) -> Self {
        Self {
            receiver,
            waker:None,
            n:0,  
        }
    }
}

impl<'a,T> Stream for StreamReceiverRef<'a,T>{
    type Item = Result<&'a mut T,String>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker = Some(cx.waker().clone());
        if self.receiver.chan.borrow_mut().usedindex.is_empty()&&self.n==0{
            match self.receiver.chan.borrow_mut().sender_waker.take(){
                Some(waker)=>{
                    waker.wake();
                },
                None=>{},
            };
            return Poll::Pending;
        }
        if !self.receiver.chan.borrow_mut().usedindex.is_empty()&&self.n==0{
            self.n=1;
        }
        let state=self.receiver.r_ref();
        let item=match state{
            Ok(item)=>Poll::Ready(Some(Result::<&mut T, String>::Ok(unsafe{&mut *(item)}))),
            Err(_)=>{
                if self.n==0{
                    self.n=1;
                    match self.receiver.chan.borrow_mut().sender_waker.take(){
                        Some(waker)=>{
                            waker.wake();
                        },
                        None=>{},
                    };
                    return Poll::Pending;
                }else{
                    return Poll::Ready(None);
                }
            }
        };
        item
    }
}


#[cfg(test)]
mod tests{
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;
    use crate::DispatchCenter::singele::{
        singele_runtime::SingeleRuntime,
    };
    use log::{info,LevelFilter};
        fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("aura::DispatchCenter::singele::sync", LevelFilter::Trace) 
            .filter_module("mio", LevelFilter::Warn)
            .filter_module("aura::DispatchCenter::singele::singele_runtime", LevelFilter::Off)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
    #[test]
    fn test_channel(){
        setup_logger(LevelFilter::Info);
        use futures::stream::StreamExt;
        let (mut sender,mut receiver,_)=Channel::<char>::builder();
        let list=vec!['a','b','c','d','e','f','g','h','i','j'];
        let future1=async move {
            let mut stream=sender.stream_send(list);
            while let Some(item)=stream.next().await{
                match item{
                    Ok(_)=>{
                        continue;
                    }
                    Err(_)=>{
                        log::error!("发送通道错误");
                    }
                }
            }
        };
        let future2=async move {
            let mut stream=receiver.stream_recv();
            while let Some(item)=stream.next().await{
                match item{
                    Ok(item)=>{
                        info!("{}",item);
                    }
                    Err(_)=>{
                        log::error!("接收通道错误");
                    }
                }
            }
        };
        SingeleRuntime::run_all(vec![Box::pin(future1),Box::pin(future2)]);
    }
    #[test]
    fn test_recv_ref(){
        setup_logger(LevelFilter::Info);
        use futures::stream::StreamExt;
        let (mut sender,mut receiver,chan)=Channel::<char>::builder();
        let list=vec!['a','b','c','d','e','f','g','h','i','j'];
        let future1=async move {
            let mut stream=sender.stream_send(list);
            while let Some(item)=stream.next().await{
                match item{
                    Ok(_)=>{
                        continue;
                    }
                    Err(_)=>{
                        log::error!("发送通道错误");
                    }
                }
            }
        };
        let future2=async move {
            let mut stream=receiver.stream_recv_ref();
            while let Some(item)=stream.next().await{
                match item{
                    Ok(item)=>{
                        info!("{}",item);
                    }
                    Err(_)=>{
                        log::error!("接收通道错误");
                    }
                }
            }
            chan.borrow_mut().clear();
        };
        SingeleRuntime::run_all(vec![Box::pin(future1),Box::pin(future2)]);
    }
    
}