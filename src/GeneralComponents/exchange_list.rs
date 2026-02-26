use std::{
    sync::{atomic::{AtomicUsize, Ordering},Mutex},
    cell::Cell,
};

#[derive(Debug)]
pub struct ExchangeList<T>{
    list: [Mutex<Vec<T>>; 2], // (threadid, taskid)
    write_index: AtomicUsize,                                                                                                                                                                                                                                                                                                                                                                    
    read_index: AtomicUsize,
    state: AtomicUsize,// 0: 空, 1: 有任务, 2: 交换中
}

impl<T> ExchangeList<T>{
    pub fn new() -> Self{
        Self{
            list: [Mutex::new(Vec::new()), Mutex::new(Vec::new())],
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
            state: AtomicUsize::new(0),
        }
    }

    pub fn push(& self, task: T)->Result<(), String>{
        if self.state.load(Ordering::Acquire) == 2{
            return Err("缓冲区交换中，无法写入".to_string());
        }
        let write_index = self.write_index.load(Ordering::Acquire);
        (*self.list[write_index].lock().unwrap()).push(task);
        self.state.store(1, Ordering::Release);
        Ok(())
    }

    pub fn change_index(& self)->Result<(), String>{
        if self.state.load(Ordering::Relaxed) == 2{
            return Err("缓冲区交换中，无法交换".to_string());
        }
        let k=self.state.load(Ordering::Relaxed);
        self.state.store(2, Ordering::Release);
        let oldread_index = self.read_index.load(Ordering::Relaxed);
        let oldwrite_index=self.write_index.load(Ordering::Relaxed);
        self.read_index.store(oldwrite_index, Ordering::Release);
        self.write_index.store(oldread_index, Ordering::Release);
        self.state.store(k, Ordering::Release);
        Ok(())
    }

    pub fn pop_all(& self) -> Result<Option<Vec<T>>, String>{
        self.change_index()?;
        let read_index = self.read_index.load(Ordering::Relaxed);
        if (*self.list[read_index].lock().unwrap()).is_empty(){
            Ok(None)
        }else{
            Ok(Some((*self.list[read_index].lock().unwrap()).drain(..).collect()))
        }
    }

    pub fn check_write(&self)->bool{
        if self.state.load(Ordering::Relaxed) == 0{
            false
        }else{
            (*self.list[self.write_index.load(Ordering::Relaxed)].lock().unwrap()).is_empty()
        }
    }
}

