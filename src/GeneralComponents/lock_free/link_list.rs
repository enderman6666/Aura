use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use super::prt::Atomic;
use super::thread_gruad::*;

pub struct Node {
    pub value: Atomic<*mut ThreadData>,
    pub next: Atomic<Node>,
    _marker: PhantomData<*mut ThreadData>,
    pub id: AtomicU64,//全局世代
    pub drop: AtomicBool,//是否被删除
}

impl Node {
    fn new(value: *mut ThreadData, id: u64) -> Self {
        Self {
            value: Atomic::new(value),
            next: Atomic::null(),
            id: AtomicU64::new(id),
            _marker: PhantomData,
            drop: AtomicBool::new(false),
        }
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::Acquire)
    }

    pub fn is_drop(&self) -> bool {
        self.drop.load(Ordering::Acquire)
    }

    pub fn set_drop(&self, id: u64) {
        self.drop.store(true, Ordering::Release);
        self.id.store(id, Ordering::Release);
    }

    pub fn set_no_drop(&self){
        self.drop.store(false, Ordering::Release);
    }
}


pub struct LinkList {
    head: Atomic<Node>,
    tail: Atomic<Node>,
    id: AtomicU64,//全局世代
    operation_times: AtomicU64,//操作次数
    old_head: Atomic<Node>,//逻辑删除但物理未删除的节点头
}

impl LinkList {
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            tail: Atomic::null(),
            id: AtomicU64::new(0),
            operation_times: AtomicU64::new(0),
            old_head: Atomic::null(),//逻辑删除但物理未删除的节点头
        }
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::Acquire)
    }

    pub fn set_id(&self, id: u64) {
        self.id.store(id, Ordering::Release);
    }

    //推送一个元素到链表尾部，返回Ok(())表示成功，返回Err(value)表示失败，其中包含了推送的值
    pub fn push(&self, value: *mut ThreadData)->*mut Node{
        self.clear_drop();
        let id=self.id.load(Ordering::Acquire);
        let node=Box::into_raw(Box::new(Node::new(value, id)));
        let mut tail=self.tail.load();
        let mut backoff = 1u32;
        loop{
            if tail.is_null(){
                if self.tail.compare_exchange(std::ptr::null_mut(), node,).is_ok(){
                        if self.head.compare_exchange(std::ptr::null_mut(), node,).is_ok(){
                            if self.old_head.compare_exchange(std::ptr::null_mut(), node,).is_ok(){
                                self.operation_times.fetch_add(1, Ordering::Release);
                            }else{
                                let _=self.old_head.compare_exchange(node,std::ptr::null_mut(),);
                            }
                            return node;
                        }else{
                            let _=self.tail.compare_exchange(node,std::ptr::null_mut(),);
                        }
                    }
            }else{
                tail=match self.fresh_tail(){
                    Some(tail)=>tail,
                    None=>std::ptr::null_mut(),
                };
                if unsafe{(*tail).next.compare_exchange(std::ptr::null_mut(), node).is_ok()}{
                    let _=self.tail.compare_exchange(tail, node);
                    self.operation_times.fetch_add(1, Ordering::Release);
                    return node;
                }else{
                    tail=unsafe{(*tail).next.load()};
                }
            }
            for _ in 0..backoff {
                std::hint::spin_loop();
            }
            if backoff < 64 {
                backoff *= 2;
            }
        }
    }

    //会弹出未被标记为drop为ture的节点，弹出后会将节点的drop标记设置为true，节点实际未被删除
    pub fn pop(&self)->Option<*mut ThreadData>{
        self.clear_drop();
        let mut head=self.head.load();
        let mut backoff = 1u32;
        loop{
            if head.is_null(){
                self.id.fetch_add(1, Ordering::Release);
                self.operation_times.fetch_add(1, Ordering::Release);
                return None;
            }else{
                head=match self.fresh_head(){
                    Some(head)=>{
                        if !unsafe{(*head).is_drop()}{
                            unsafe{(*head).set_drop(self.id.load(Ordering::Acquire))};
                            if !unsafe{(*head).next.load()}.is_null(){
                                let _=self.head.compare_exchange(head, unsafe{(*head).next.load()});
                            }
                            self.operation_times.fetch_add(1, Ordering::Release);
                            return unsafe{(*head).value.take()};
                        }else{
                            let old_head=unsafe{(*head).next.load()};
                            let _=self.old_head.compare_exchange(old_head,head);
                            unsafe{(*head).next.load()}
                        }
                    },
                    None=>std::ptr::null_mut(),
                };
            }
            for _ in 0..backoff {
                std::hint::spin_loop();
            }
            if backoff < 64 {
                backoff *= 2;
            }
        }
    }

    //弹出所有节点
    pub fn pop_all(&self)->Vec<*mut ThreadData>{
        let mut dropped_nodes=Vec::new();
        loop{
            match self.pop(){
                Some(value)=>{
                    dropped_nodes.push(value);
                }
                None=>{
                    break;
                }
            }
        }
        dropped_nodes
    }

    pub fn find(&self, id: u64)->Option<*mut Node>{
        let mut head=self.head.load();
        loop{
            if head.is_null(){
                return None;
            }else{
                if unsafe{(*head).id() == id}{
                    return Some(head);
                }else{
                    head=unsafe{(*head).next.load()};
                }
            }
        }
    }

    pub fn pop_all_prt(&self)->Vec<*mut Node>{
        let mut dropped_nodes=Vec::new();
        let mut head=self.head.load();
        loop{
            if head.is_null(){
                break;
            }else{
                dropped_nodes.push(head);
                head=unsafe{(*head).next.load()};
            }
        }
        dropped_nodes
    }

    //获取链表的尾部节点，返回Ok(tail)表示成功，其中tail是一个指向节点的指针，返回Err(ChangeErr::NoNode)表示失败
    pub fn fresh_tail(&self)->Option<*mut Node>{
        let mut tail=self.tail.load();
        loop{
            if tail.is_null(){
                return None;
            }else{
                let next = unsafe{(*tail).next.load()};
                if next.is_null(){
                    return Some(tail);
                }else{
                    let _=self.tail.compare_exchange(tail, next);
                    tail=self.tail.load();
                }
            }
        }
    }

    pub fn fresh_head(&self)->Option<*mut Node>{
        let mut head=self.head.load();
        let old_head=head;
        if old_head.is_null(){
            return None;
        }
        loop{
            if head.is_null(){
                if self.head.compare_exchange(old_head, std::ptr::null_mut(),).is_ok(){
                    return None;
                }
            }else{
                if unsafe{(*head).drop.load(Ordering::Acquire)}{
                    head=unsafe{(*head).next.load()};
                }else{
                    return Some(head);
                }
            }
        }
    }

    pub fn clear_drop(&self){
        
        if self.operation_times.load(Ordering::Acquire)<30{
            return;
        }
        let first_head = self.old_head.load();
        if first_head.is_null(){
            return;
        }
        if unsafe{(*first_head).is_drop()}{
            let next = unsafe{(*first_head).next.load()};
            if self.old_head.compare_exchange(first_head, next).is_ok(){
                unsafe{(*first_head).next.store(std::ptr::null_mut())};
                drop(unsafe{Box::from_raw(first_head)});
            }
        }
    }
}

impl Drop for LinkList{
    fn drop(&mut self) {
        let mut o_head={
            let drop=self.old_head.load();
            if !drop.is_null(){
                drop
            }else{
                self.head.load()
            }
        };
        let mut head=o_head;
        self.head.store(std::ptr::null_mut());
        self.tail.store(std::ptr::null_mut());
        self.old_head.store(std::ptr::null_mut());
        let mut n=0;
        loop{
            if n>100{
                let h=unsafe{(*head).next.swap(std::ptr::null_mut())};
                drop(unsafe{Box::from_raw(o_head)});
                o_head=h;
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
        if !head.is_null(){
            let _=unsafe{Box::from_raw(head)};
        }
    }
}

unsafe impl Send for LinkList {}
unsafe impl Sync for LinkList {}
