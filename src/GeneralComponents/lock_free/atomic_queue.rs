use super::{prt::Atomic,thread_gruad::*};
use std::sync::{atomic::{Ordering,AtomicU64,AtomicBool}};
use std::cell::UnsafeCell;
use super::error::QueueError;


pub struct Node<T>{
    pub value: Atomic<T>,
    pub next: Atomic<Node<T>>,
    pub id: AtomicU64,//全局世代
    pub sequence: UnsafeCell<u64>,//节点序列
    drop: AtomicBool, 
}

impl<T> Node<T>{
    fn new(value: T,id:u64) -> Self {
        Self {
            value: Atomic::new(value),
            next: Atomic::null(),
            id: AtomicU64::new(id),
            sequence: UnsafeCell::new(0),
            drop: AtomicBool::new(false),
        }
    }

    pub fn set_sequence(&self,sequence:u64){
        unsafe{*self.sequence.get()=sequence;}
    }

    pub fn sequence(&self)->u64{
        unsafe{*self.sequence.get()}
    }
}

// 原子链表,
pub struct AtomicQueue<T>{
    head: Atomic<Node<T>>,
    tail: Atomic<Node<T>>,
    id: AtomicU64,//全局世代
    drop_head: Atomic<Node<T>>,// 逻辑删除但物理未删除的节点头
    len: AtomicU64,//队列长度
    run: AtomicBool,
    operation_times: AtomicU64,//操作次数
    max_operation_times:AtomicU64,//最大操作次数
}

impl<T> AtomicQueue<T>{
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            tail: Atomic::null(),
            id: AtomicU64::new(0),
            drop_head: Atomic::null(),
            len: AtomicU64::new(0),
            run: AtomicBool::new(true),
            operation_times: AtomicU64::new(0),
            max_operation_times: AtomicU64::new(30),//最大操作次数
        }
    }

    fn fresh_tail(&self)->*mut Node<T>{
        let mut tail=self.tail.load();
        if !tail.is_null(){
            loop{
                let next=unsafe{(*tail).next.load()};
                if next.is_null(){
                    return tail;
                }else{
                    let _=self.tail.compare_exchange(tail, next);
                    tail=self.tail.load();
                }
            }
        }else{
            return tail;
        }
    }

    pub fn push(&self, value: T)->Result<*mut Node<T>,T>{
        let result:Result<*mut Node<T>,T>= {
            let _gruad=Gruad::new();
            if !self.run.load(Ordering::Acquire){
                return Err(value);
            }
            let version=_gruad.version();
            self.id.store(version, Ordering::Release);
            let node=Box::into_raw(Box::new(Node::new(value,version)));
            let mut backoff = 1u32;
            loop{ 
                let tail=self.fresh_tail();
                if tail.is_null(){
                    if self.tail.compare_exchange(std::ptr::null_mut(), node).is_ok(){
                        let _=self.head.compare_exchange(std::ptr::null_mut(), node);
                        let len=self.len.fetch_add(1, Ordering::Release);
                        unsafe{(*node).set_sequence(len);}
                        self.operation_times.fetch_add(1, Ordering::Release);
                        break Ok(node);
                    }
                }else{
                    let next=unsafe{(*tail).next.load()};
                    if next.is_null(){
                        if unsafe{(*tail).next.compare_exchange(std::ptr::null_mut(), node).is_ok()}{
                            let _=self.tail.compare_exchange(tail, node);
                            let len=self.len.fetch_add(1, Ordering::Release);
                            unsafe{(*node).set_sequence(len);}
                            self.operation_times.fetch_add(1, Ordering::Release);
                            break Ok(node);
                        }
                    }
                }
                for _ in 0..backoff {
                    std::hint::spin_loop();
                }
                if backoff < 64 {
                    backoff *= 2;
                }
            }
        };
        self.cont_clear();
        result
    }

    pub fn try_push(&self,value:T)->Result<*mut Node<T>,(QueueError,T)>{
        let result={
            let _gruad=Gruad::new();
            if !self.run.load(Ordering::Acquire){
                Err((QueueError::QueueNotRun,value))
            }else{
                let version=_gruad.version();
                self.id.store(version, Ordering::Release);
                let node=Box::into_raw(Box::new(Node::new(value,version)));
                let tail=self.fresh_tail();
                if tail.is_null(){
                    if self.tail.compare_exchange(std::ptr::null_mut(), node).is_ok(){
                        let _=self.head.compare_exchange(std::ptr::null_mut(), node);
                        let len=self.len.fetch_add(1, Ordering::Release);
                        unsafe{(*node).set_sequence(len);}
                        self.operation_times.fetch_add(1, Ordering::Release);
                        Ok(node)
                    }else{
                        let value=unsafe{Box::from_raw(node).value.take().unwrap()};  
                        Err((QueueError::CasFailed,value))
                    }
                }else{
                    let next=unsafe{(*tail).next.load()};
                    if next.is_null(){
                        if unsafe{(*tail).next.compare_exchange(std::ptr::null_mut(), node).is_ok()}{
                            let _=self.tail.compare_exchange(tail, node);
                            let len=self.len.fetch_add(1, Ordering::Release);
                            unsafe{(*node).set_sequence(len);}
                            self.operation_times.fetch_add(1, Ordering::Release);
                            Ok(node)
                        }else{
                            let value=unsafe{Box::from_raw(node).value.take().unwrap()};  
                            Err((QueueError::CasFailed,value))
                        }
                    }else{
                        let value=unsafe{Box::from_raw(node).value.take().unwrap()};  
                        Err((QueueError::CasFailed,value))
                    }
                }
            }
        };
        self.cont_clear();
        result
    }

    pub fn pop(&self)->Option<T>{
        let result:Option<T>= {
            let _gruad=Gruad::new();
            if !self.run.load(Ordering::Acquire){
                return None;
            }
            let version=_gruad.version();
            self.id.store(version, Ordering::Release);
            let mut head=self.head.load();
            let mut backoff = 1u32;
            loop{
                if head.is_null(){
                    break None;
                }else{
                    let mut next=unsafe{(*head).next.load()};
                    if next.is_null(){
                        if self.head.compare_exchange(head,next).is_ok(){
                            let _=self.tail.compare_exchange(head,next);
                            let drop_head=self.drop_head.load();
                            unsafe{(*head).drop.store(true,Ordering::Release)};
                            match unsafe{(*head).value.take()}{
                                Some(value) => {
                                    if !drop_head.is_null(){    
                                        if unsafe{(*drop_head).sequence()}>=unsafe{(*head).sequence()}{
                                            let _= self.drop_head.compare_exchange(drop_head,head);
                                        }
                                    }
                                    self.len.fetch_sub(1, Ordering::Release);
                                    self.operation_times.fetch_add(1, Ordering::Release);
                                    break Some(value);
                                },
                                None => {
                                    return None;
                                },
                            }
                        }else{
                            head=self.head.load();
                        }
                    }else{
                        if self.head.compare_exchange(head,next).is_ok(){
                            let drop_head=self.drop_head.load();
                            unsafe{(*head).drop.store(true,Ordering::Release)};
                            match unsafe{(*head).value.take()}{
                                Some(value) => {
                                    if !drop_head.is_null(){    
                                        if unsafe{(*drop_head).sequence()}<unsafe{(*head).sequence()}{
                                            let _= self.drop_head.compare_exchange(drop_head,head);
                                        }
                                    }
                                    self.len.fetch_sub(1, Ordering::Release);
                                    self.operation_times.fetch_add(1, Ordering::Release);
                                    break Some(value);
                                },
                                None => {
                                    head=next;
                                    next=unsafe{(*head).next.load()};
                                    let _=self.head.compare_exchange(head,next);
                                },
                            }
                        }else{
                            head=self.head.load();
                        }
                    }
                }
                for _ in 0..backoff {
                    std::hint::spin_loop();
                }
                if backoff < 64 {
                    backoff *= 2;
                }
            }
        };
        self.cont_clear();
        result
    }

    pub fn try_pop(&self)->Result<Option<T>,QueueError>{
        let result={
            let _gruad=Gruad::new();
            if !self.run.load(Ordering::Acquire){
                Err(QueueError::QueueNotRun)
            }else{
                let version=_gruad.version();
                self.id.store(version, Ordering::Release);
                let head=self.head.load();
                if head.is_null(){
                    return Ok(None);
                }else{
                    let next=unsafe{(*head).next.load()};
                    if next.is_null(){
                        if self.head.compare_exchange(head,std::ptr::null_mut()).is_ok(){
                            let _=self.tail.compare_exchange(head,std::ptr::null_mut());
                            let drop_head=self.drop_head.load();
                            unsafe{(*head).drop.store(true,Ordering::Release)};
                            match unsafe{(*head).value.take()}{
                                Some(value) => {
                                    if !drop_head.is_null(){    
                                        if unsafe{(*drop_head).sequence()}>=unsafe{(*head).sequence()}{
                                            let _= self.drop_head.compare_exchange(drop_head,head);
                                        }
                                    }
                                    self.len.fetch_sub(1, Ordering::Release);
                                    self.operation_times.fetch_add(1, Ordering::Release);
                                    Ok(Some(value))
                                },
                                None => {
                                    Err(QueueError::CasFailed)
                                },
                            }
                        }else{
                            Err(QueueError::CasFailed)
                        }
                    }else{
                        if self.head.compare_exchange(head,next).is_ok(){
                            let drop_head=self.drop_head.load();
                            unsafe{(*head).drop.store(true,Ordering::Release)};
                            match unsafe{(*head).value.take()}{
                                Some(value) => {
                                    if !drop_head.is_null(){    
                                        if unsafe{(*drop_head).sequence()}>=unsafe{(*head).sequence()}{
                                            let _= self.drop_head.compare_exchange(drop_head,head);
                                        }
                                    }
                                    self.len.fetch_sub(1, Ordering::Release);
                                    self.operation_times.fetch_add(1, Ordering::Release);
                                    Ok(Some(value))
                                },
                                None => {
                                    Err(QueueError::CasFailed)
                                },
                            }
                        }else{
                            Err(QueueError::CasFailed)
                        }
                    }
                }
            }
            
        };
        self.cont_clear();
        result
    }

    pub fn pop_all(&self)->Vec<T>{
        if !self.run.load(Ordering::Acquire){
            return Vec::new();
        }
        let mut list=Vec::new();
        loop{
            match self.pop(){
                Some(value) => {
                    list.push(value);
                },
                None => break,
            }
        }
        list
    }

    pub fn len(&self)->u64{
        if !self.run.load(Ordering::Acquire){
            return 0;
        }
        self.len.load(Ordering::Acquire)
    }

    //设置最大操作次数，超过此次数后，会清空队列
    pub fn set_max_times(&self,times:u64){
        self.max_operation_times.store(times, Ordering::Release);
    }

    //根据操作次数检查是否需要清空队列
    pub fn cont_clear(&self){
        let times=self.operation_times.load(Ordering::Acquire);
        if times>=self.max_operation_times.load(Ordering::Acquire){
            if let Some(_)=EPOCH.get(){
                fresh_version();
            }
            self.operation_times.store(times-self.max_operation_times.load(Ordering::Acquire), Ordering::Release);
            self.clear();
        }
        
    }

    //清空标记为drop的节点
    pub fn clear(&self){
        if !self.run.load(Ordering::Acquire){
            return;
        }
        if !get_gruads(){
            return;
        }
        let version=EPOCH.get().unwrap().id();
        let mut head=self.drop_head.load();
        while !head.is_null(){
            let next=unsafe{(*head).next.load()};
            if unsafe{(*head).id.load(Ordering::Acquire)}<version-1{
                if unsafe{(*head).drop.load(Ordering::Acquire)}{
                    let next=unsafe{(*head).next.load()};
                    head=next;
                }else {
                    let _=self.drop_head.compare_exchange(head,next);
                    drop(unsafe{Box::from_raw(head)});
                    break;
                }
            }else{
                let _=self.drop_head.compare_exchange(head,next);
                break;
            }
        }
        EPOCH.get().unwrap().clear_drop();
    }
}

impl<T> Drop for AtomicQueue<T>{
    fn drop(&mut self) {
        self.run.store(false, Ordering::Release);
        let mut old_head={
            let drop=self.drop_head.load();
            if !drop.is_null(){
                drop
            }else{
                self.head.load()
            }
        };
        let mut head=old_head;
        self.head.store(std::ptr::null_mut());
        self.tail.store(std::ptr::null_mut());
        self.drop_head.store(std::ptr::null_mut());
        let mut n=0;
        loop{
            if n>100{
                let h=unsafe{(*head).next.swap(std::ptr::null_mut())};
                drop(unsafe{Box::from_raw(old_head)});
                old_head=h;
                head=h;
                n=0;
            }
            if head.is_null(){
                break;
            }else{
                let next=unsafe{(*head).next.load()};
                head=next;
                n+=1;
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn test_atomic_queue_push() {
            let queue1=Arc::new(AtomicQueue::new());
            let mut list=Vec::new();
            for i in 0..100{
                let q=queue1.clone();
                let n=std::thread::spawn(move ||{
                    println!("线程{}开始操作",i);
                    for _ in 0..10000{
                        q.push(i).unwrap();
                    }
                    i
                });
                list.push(n);
            }
            for n in list{
                let i=n.join().unwrap();
                println!("线程{}操作完成",i);

            }
            assert_eq!(queue1.len(),1000000);
    }

    #[test]
    fn test_atomic_queue_pop(){
        let queue1=Arc::new(AtomicQueue::new());
        let mut list=Vec::new();
        for k in 0..100000{
            queue1.push(k).unwrap();
        }
        for i in 0..100{
            let q=queue1.clone();
            let n=std::thread::spawn(move ||{
                println!("线程{}开始操作",i);
                for _ in 0..1000{
                    let _=q.pop();
                }
                i
            });
            list.push(n);
        }
        for n in list{
            let i=n.join().unwrap();
            println!("线程{}操作完成",i);

        }
        assert_eq!(queue1.len(),0);
    }
}
