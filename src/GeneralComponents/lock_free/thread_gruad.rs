use super::link_list::{LinkList,Node};
use std::sync::{OnceLock,atomic::{AtomicU64,Ordering}};
use std::cell::Cell;

// 公共线程数据链表
pub static EPOCH: OnceLock<LinkList> = OnceLock::new();
pub const MAX_DROP_TIMES:AtomicU64=AtomicU64::new(500);

// 线程本地数据
thread_local!{
    static GRUADS: Cell<Option<*mut ThreadData>> = Cell::new(None);
    static NODE: Cell<Option<*mut Node>> = Cell::new(None);
}
pub fn get_gruads()->bool{
    let epoch=EPOCH.get().unwrap();
    let version=epoch.id();
    let list =epoch.pop_all_prt();
    for node in list{
        if unsafe{(*node).id.load(Ordering::Relaxed)}>1{
            if unsafe{(*node).id.load(Ordering::Relaxed)}<version-1{
                unsafe{
                    let n=(**(*node).value.load()).gruads.load(Ordering::Relaxed);
                    if n!=0{
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn fresh_version(){
    if let Some(epoch) = EPOCH.get() {
        let version=epoch.id();
        let list =epoch.pop_all_prt();
        let mut drops=0;
        let mut low=0;
        for node in list{
            if !version==unsafe{(*node).id.load(Ordering::Relaxed)}{
                low=low+1;
                let drop_times=unsafe{(**(*node).value.load()).drop_times.load(Ordering::Relaxed)};
                drops=drops+drop_times;
            }
        }
        if low==0{
            epoch.set_id(version+1);
        }else if drops>MAX_DROP_TIMES.load(Ordering::Relaxed){
            epoch.set_id(version+1);
        }
    }
}

// 线程本地数据结构体
pub struct ThreadData{
    pub version:AtomicU64,
    pub gruads: AtomicU64,
    drop_times:AtomicU64,
}

impl ThreadData{
    pub fn new() -> (*mut Self, u64) {
        if let Some(thread_data) = GRUADS.with(|cell| cell.get()) {
            return (thread_data, unsafe{(*thread_data).version.load(Ordering::Relaxed)});
        }
        let epoch = EPOCH.get_or_init(|| LinkList::new());
        let id=epoch.id();
        let data=Self {
            version: AtomicU64::new(id),
            gruads: AtomicU64::new(1),
            drop_times:AtomicU64::new(0),
        };
        let ptr=Box::into_raw(Box::new(data));
        epoch.push(ptr);
        (ptr, id)
    }

    pub fn fresh_version(&self){
        let version=EPOCH.get().unwrap().id();
        if version>self.version.load(Ordering::Relaxed){
            self.version.store(version,Ordering::Relaxed);
        }
    }
}

impl Drop for ThreadData{
    fn drop(&mut self) {
        let node=EPOCH.get().unwrap().find(self.version.load(Ordering::Relaxed));
        if let Some(node) = node {
            unsafe{
                (*node).value.take();
                (*node).set_drop(self.version.load(Ordering::Relaxed));
            }
        }
    }
}

// 线程本地数据守卫
pub struct Gruad{
    version:u64,
}

impl Gruad{
    pub fn new() -> Self {
        NODE.with(|cell| {
            if let Some(node) = cell.get() {
                unsafe{(*node).set_no_drop();}
            }
        });
        let version=GRUADS.with(|cell| {
            match cell.get() {
                None => {
                    let (thread_data, id) = ThreadData::new();
                    cell.set(Some(thread_data));
                    id
                }
                Some(thread_data) => {
                    unsafe{(*thread_data).fresh_version();}
                    unsafe{(*thread_data).gruads.fetch_add(1,Ordering::Acquire)};
                    let version = unsafe{(*thread_data).version.load(Ordering::Acquire)};
                    version
                }
            }
        });
        Self { version }
    }

    pub fn version(&self) -> u64 {
        self.version
    }
}

impl Drop for Gruad{
    fn drop(&mut self) {
        GRUADS.with(|cell| {
            if let Some(thread_data) = cell.get() {
                if self.version == unsafe{(*thread_data).version.load(Ordering::Relaxed)} {
                    unsafe{(*thread_data).gruads.fetch_sub(1,Ordering::Release)};
                    if unsafe{(*thread_data).gruads.load(Ordering::Relaxed)} == 0 {
                        NODE.with(|cell| {
                            if let Some(node) = cell.get() {
                                unsafe{(*node).set_drop(self.version);}
                                unsafe{(*thread_data).drop_times.fetch_add(1,Ordering::Release);}
                            }
                        });
                    }
                }
            }
        });
    }
}
