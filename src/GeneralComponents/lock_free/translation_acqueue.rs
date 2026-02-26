use super::{prt::Atomic,thread_gruad::*,error::QueueError};
use std::sync::{
        atomic::{
            AtomicU64,
            AtomicBool,
            Ordering
        }
    };

//有界原子队列
pub struct TrAtomicQueue<T>{
    buffer:Vec<Atomic<T>>,
    max_count:usize,
    size:AtomicU64,
    running:AtomicBool,
    head:AtomicU64,
    tail:AtomicU64,
}

impl<T> TrAtomicQueue<T>{
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

    // 推入元素
    fn push(&self,data:T)->Result<(),T>{
        let result={
            let _gruad=Gruad::new();
            let data=Box::into_raw(Box::new(data));
            if !self.running.load(Ordering::Acquire){
                Err(unsafe{*Box::from_raw(data)})
            }else{
                if self.size.load(Ordering::Acquire)>=self.max_count as u64{
                    Err(unsafe{*Box::from_raw(data)})
                }else{
                    loop{
                        let tail = self.tail.load(Ordering::Acquire);
                        let next_tail = (tail+1)%self.max_count as u64;
                        if self.tail.compare_exchange_weak(tail,next_tail,Ordering::Release,Ordering::Relaxed).is_ok(){
                            if self.buffer[tail as usize].compare_exchange(std::ptr::null_mut(),data).is_ok(){
                                self.size.fetch_add(1,Ordering::Release);
                                break Ok(());
                            }
                        }
                    }
                }
            }
        };
        return result;
    }


    // 弹出元素
    pub fn pop(&self)->Result<T,()>{
        let result={
            let _gruad=Gruad::new();
            if !self.running.load(Ordering::Acquire){
                Err(())
            }else{
                if self.size.load(Ordering::Acquire)==0{
                    Err(())
                }else{
                    loop{
                        let head = self.head.load(Ordering::Acquire);
                        let next_head = (head+1)%self.max_count as u64;
                        let data=self.buffer[head as usize].load();
                        if self.head.compare_exchange_weak(head,next_head,Ordering::Release,Ordering::Relaxed).is_ok(){
                            if self.buffer[head as usize].compare_exchange(data, std::ptr::null_mut()).is_ok(){
                                self.size.fetch_sub(1,Ordering::Release);
                                break Ok(unsafe{*Box::from_raw(data)});
                            }
                        }
                    }
                }
            }
        };
        return result;
    }


    // 尝试弹出元素
    //通常用于其它的方法包装实现
    pub fn try_pop(&self)->Result<Option<T>,QueueError>{
        let result={
            let _gruad=Gruad::new();
            if !self.running.load(Ordering::Acquire){
                Err(QueueError::QueueNotRun)
            }else{
                if self.size.load(Ordering::Acquire)==0{
                    Ok(None)
                }else{
                    let head = self.head.load(Ordering::Acquire);
                    let next_head = (head+1)%self.max_count as u64;
                    let data=self.buffer[head as usize].load();
                    if self.head.compare_exchange_weak(head,next_head,Ordering::Release,Ordering::Relaxed).is_ok(){
                        if self.buffer[head as usize].compare_exchange(data, std::ptr::null_mut()).is_ok(){
                            self.size.fetch_sub(1,Ordering::Release);
                            Ok(Some(unsafe{*Box::from_raw(data)}))
                        }else{
                            Err(QueueError::CasFailed)
                        }
                    }else{
                        Err(QueueError::CasFailed)
                    }
                }
            }
        };
        return result;
    }

    // 尝试推入元素
    //通常用于其它的方法包装实现
    pub fn try_push(&self,data:T)->Result<*mut T,(QueueError,T)>{
        let result={
            let _gruad=Gruad::new();
            let data=Box::into_raw(Box::new(data));
            if !self.running.load(Ordering::Acquire){
                Err((QueueError::QueueNotRun,unsafe{*Box::from_raw(data)}))
            }else{
                if self.size.load(Ordering::Acquire)>=self.max_count as u64{
                    Err((QueueError::QueueFull,unsafe{*Box::from_raw(data)}))
                }else{
                    let tail = self.tail.load(Ordering::Acquire);
                    let next_tail = (tail+1)%self.max_count as u64;
                    if self.tail.compare_exchange_weak(tail,next_tail,Ordering::Release,Ordering::Relaxed).is_ok(){
                        if self.buffer[tail as usize].compare_exchange(std::ptr::null_mut(),data).is_ok(){
                            self.size.fetch_add(1,Ordering::Release);
                            Ok(data)
                        }else{
                            Err((QueueError::CasFailed,unsafe{*Box::from_raw(data)}))
                        }
                    }else{
                        Err((QueueError::CasFailed,unsafe{*Box::from_raw(data)}))
                    }
                }
            }
        };
        return result;
    }

    // 获取队列长度
    pub fn len(&self)->usize{
        self.size.load(Ordering::Acquire) as usize
    }

    // 判断队列是否为空
    pub fn is_empty(&self)->bool{
        self.len()==0
    }


    // 判断队列是否已满
    pub fn is_full(&self)->bool{
        self.len()==self.max_count as usize-1
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
    #[should_panic]
    fn test_translation_acqueue(){
        let queue=TrAtomicQueue::<u64>::new(10);
        for i in 0..11{
            queue.try_push(i).unwrap();
        }
    }
    #[test]
    fn test_translation_acqueue_pop(){
        let queue=TrAtomicQueue::<u64>::new(10);
        for i in 0..10{
            queue.try_push(i).unwrap();
        }
        for i in 0..10{
            let data=queue.try_pop().unwrap().unwrap();
            assert_eq!(data,i);
        }
    }
}