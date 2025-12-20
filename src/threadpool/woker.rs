use std::{
    sync::{Arc, Mutex, Weak}, 
    thread::{self, Thread},
    sync::mpsc::{Receiver},
};

use super::super::DispatchCenter::runtime::Aura;

pub struct Worker<T: 'static> {
    id: usize,
    pub thread: Option<thread::JoinHandle<()>>,
    receiver: Arc<Mutex<Receiver<Box<dyn FnOnce() + Send+'static>>>>,
    pub runtime: Option<Arc<Mutex<Aura<T>>>>,
}

impl<T: 'static> Worker<T> {
    pub fn new(id: usize, receiver: Arc<Mutex<Receiver<Box<dyn FnOnce() + Send+'static>>>>, runtime: Option<Arc<Mutex<Aura<T>>>>) -> Self {
        Self {
            id,
            thread: None,
            receiver,
            runtime,
        }
    }

    // 启动工作线程,如果有运行时,则在新线程中运行运行时主循环,否则从通道接收任务并执行
    pub fn start(&mut self) {
        match self.runtime.as_ref() {
            Some(runtime) => {
                let runtime = Arc::clone(runtime);
                self.thread = Some(thread::spawn(move || {
                    let aura = runtime.lock().unwrap().clone();
                    aura.run();
                }));
            }
            None => {
                let receiver = Arc::clone(&self.receiver);
                self.thread = Some(thread::spawn(move || {
                    while let Ok(job) = receiver.lock().unwrap().recv() {
                        job();
                    }
                }));
            }
        }
    }

}