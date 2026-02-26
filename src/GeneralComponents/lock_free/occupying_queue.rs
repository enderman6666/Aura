use super::{prt::Atomic,thread_gruad::*,error::QueueError};
use std::{
        sync::{
            atomic::{
                AtomicU64,
                AtomicBool,
                Ordering
            },
        },
    };

#[derive(Debug,Clone,Copy)]
pub enum State<T>{
    Placeholder,
    Data(T),
}

impl<T> State<T>{
    pub fn unwrap(self)->T{
        match self{
            State::Placeholder=>panic!("unwrap placeholder"),
            State::Data(data)=>data,
        }
    }
}

//有界原子队列
pub struct OccupyQueue<T>{
    buffer:Vec<Atomic<State<T>>>,
    max_count:usize,
    size:AtomicU64,
    running:AtomicBool,
    head:AtomicU64,
    tail:AtomicU64,
}

impl<T> OccupyQueue<T>{
    pub fn new(max_count:usize)->Self{
        Self{
            buffer:vec![Atomic::null();max_count],
            max_count,
            size:AtomicU64::new(0),
            running:AtomicBool::new(true),
            head:AtomicU64::new(0),
            tail:AtomicU64::new(0),
        }
    }

    // 获取队列槽位,成功则直接获取对应槽位的指针
    pub fn get_solt(&self)->Result<*mut State<T>,QueueError>{
        let result={
            let _gruad=Gruad::new();
            let data=Box::into_raw(Box::new(State::Placeholder));
            if !self.running.load(Ordering::Acquire){
                let _=unsafe{Box::from_raw(data)};
                Err(QueueError::QueueNotRun)
            }else{
                if self.size.load(Ordering::Acquire)>=self.max_count as u64{
                    let _=unsafe{Box::from_raw(data)};
                    Err(QueueError::QueueFull)
                }else{
                    loop{
                        let tail = self.tail.load(Ordering::Acquire);
                        let next_tail = (tail+1)%self.max_count as u64;
                        if self.tail.compare_exchange_weak(tail,next_tail,Ordering::Release,Ordering::Relaxed).is_ok(){
                            if self.buffer[tail as usize].compare_exchange(std::ptr::null_mut(),data).is_ok(){
                                self.size.fetch_add(1,Ordering::Release);
                                break Ok(data);
                            }
                        }
                    }
                }
            }
        };
        return result;
    }

    // 检查队列并弹出数据
    pub fn recv(&self)->Result<Option<T>,QueueError>{
        let result={
            let _gruad=Gruad::new();
            if !self.running.load(Ordering::Acquire){
                Err(QueueError::QueueNotRun)
            }else{
                if self.size.load(Ordering::Acquire)==0{
                    Err(QueueError::QueueEmpty)
                }else{
                    loop{
                        let mut head = self.head.load(Ordering::Acquire);
                        if head==self.tail.load(Ordering::Acquire){
                            break Ok(None);
                        }
                        let next_head = (head+1)%self.max_count as u64;
                        let prt=self.buffer[head as usize].load();
                        if !prt.is_null(){
                            match unsafe{&*prt}{
                                State::Placeholder=>{
                                    head=next_head;
                                },
                                State::Data(_)=>{
                                    if self.buffer[head as usize].compare_exchange(prt,std::ptr::null_mut()).is_ok(){
                                        let state = unsafe { std::ptr::read(prt) };
                                        if let State::Data(data) = state{
                                            self.head.fetch_add(1, Ordering::Release);
                                            self.size.fetch_sub(1, Ordering::Release);
                                            break Ok(Some(data));
                                        }
                                    }
                                }
                            }
                        }else{
                            head=next_head;
                        }
                    }
                }
            }
        };
        return result;
    }

    pub fn try_push(&self,data:T)->Result<(),(QueueError,T)>{
        let result={
            let _gruad=Gruad::new();
            if !self.running.load(Ordering::Acquire){
                Err((QueueError::QueueNotRun,data))
            }else{
                if self.size.load(Ordering::Acquire)>=self.max_count as u64{
                    Err((QueueError::QueueFull,data))
                }else{
                    let tail = self.tail.load(Ordering::Acquire);
                    let next_tail = (tail+1)%self.max_count as u64;
                    if self.tail.compare_exchange_weak(tail,next_tail,Ordering::Release,Ordering::Relaxed).is_ok(){
                        let data=Box::into_raw(Box::new(State::Data(data)));
                        if self.buffer[tail as usize].compare_exchange(std::ptr::null_mut(),data).is_ok(){
                            self.size.fetch_add(1,Ordering::Release);
                            Ok(())
                        }else{
                            let data=unsafe{Box::from_raw(data)};
                            Err((QueueError::CasFailed,data.unwrap()))
                        }
                    }else{
                        Err((QueueError::CasFailed,data))
                    }
                }
            }
        };
        return result;
    }

    pub fn try_pop(&self)->Result<Option<T>,QueueError>{
        let result={
            let _gruad=Gruad::new();
            if !self.running.load(Ordering::Acquire){
                Err(QueueError::QueueNotRun)
            }else{
                if self.size.load(Ordering::Acquire)==0{
                    Err(QueueError::QueueEmpty)
                }else{
                    let head = self.head.load(Ordering::Acquire);
                    self.fresh_head();
                    let prt=self.buffer[head as usize].load();
                    if !prt.is_null(){
                        match unsafe{&*prt}{
                            State::Placeholder=>{
                                Err(QueueError::CasFailed)
                            },
                            State::Data(_)=>{
                                if self.buffer[head as usize].compare_exchange(prt,std::ptr::null_mut()).is_ok(){
                                    let state = unsafe { std::ptr::read(prt) };
                                    if let State::Data(data) = state{
                                        self.head.fetch_add(1, Ordering::Release);
                                        self.size.fetch_sub(1, Ordering::Release);
                                        Ok(Some(data))
                                    }else{
                                        Err(QueueError::CasFailed)
                                    }
                                }else{
                                    Err(QueueError::CasFailed)
                                }
                            }
                        }
                    }else{
                        Ok(None)
                    }
                }
            }
        };
        return result;
    }

    pub fn fresh_head(&self){
        let mut head = self.head.load(Ordering::Acquire);
        loop{
            if head==self.tail.load(Ordering::Acquire){
                break;
            }
            let prt=self.buffer[head as usize].load();
            if prt.is_null(){
                let next_head = (head+1)%self.max_count as u64;
                if self.head.compare_exchange_weak(head,next_head,Ordering::Release,Ordering::Relaxed).is_ok(){
                    head=next_head;
                }
            }else{
                break;
            }
        }
    }

    // 判断队列是否为空
    pub fn is_empty(&self)->bool{
        self.size.load(Ordering::Acquire)==0
    }


    // 判断队列是否已满
    pub fn is_full(&self)->bool{
        self.size.load(Ordering::Acquire)>=self.max_count as u64-1
    }


    // 启动队列
    pub fn run(&self){
        self.running.store(true,Ordering::Release);
    }

    // 停止队列
    pub fn stop(&self){
        self.running.store(false,Ordering::Release);
    }

    // 判断队列是否正在运行
    pub fn is_running(&self)->bool{
        self.running.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn test_occupying_queue(){
        let queue=OccupyQueue::new(10);
        let mut list=Vec::new();
        for k in 0..10{
            let slot=if let Ok(slot)=queue.get_solt(){
                slot
            }else{
                panic!("get slot failed,{}",k);
            };
            list.push({
                slot
            });
        }
        let mut n=0;
        for i in list{
            unsafe{
                *i=State::Data(n);
                n+=1;
            }
        }
        n=0;
        while let Ok(Some(data))=queue.try_pop(){
            assert_eq!(data,n);
            n+=1;
        }
    }
}