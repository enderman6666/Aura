use std::{sync::{
        Arc, 
        Mutex,
        mpsc::{Sender, Receiver, channel},
    }, 
    thread::{self, Thread}};
use super::woker::Worker;
use super::super::DispatchCenter::runtime::Aura;

pub struct ThreadCenter<T: 'static> {
    pub workers: Vec<Option<Worker<T>>>,
    sender: Option<Sender<Box<dyn FnOnce() + Send+'static>>>,
}

impl<T: 'static> ThreadCenter<T> {
    pub fn new(runtime: Vec<Option<Arc<Mutex<Aura<T>>>>>) -> Self {
        let (sender, receiver) = channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = runtime.into_iter()
            .enumerate()
            .map(|(id, runtime)| Some(Worker::new(id, Arc::clone(&receiver), runtime)))
            .collect::<Vec<_>>();
        Self {
            workers,
            sender: Some(sender),
        }
    }

    pub fn start(&mut self){
        for worker in self.workers.iter_mut(){
            match worker{
                Some(worker) => {
                    worker.start();
                }
                None => {}
            }
        }
    }

    pub fn sqawn(&self, f: Box<dyn FnOnce() + Send+'static>)->Result<(),String>{
        for worker in self.workers.iter(){
            match worker{
                Some(worker) => {
                    match worker.runtime{
                        Some(_) => {}
                        None=> {
                            if let Some(sender) = &self.sender{
                                sender.send(f).unwrap();
                            }
                            return Ok(());
                        }
                    }
                }
                None => {}
            }
        }
        Err("没有线程用于接受并行闭包".to_string())
    }
}

impl<T> Drop for ThreadCenter<T> {
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in &mut self.workers{
            match worker.take(){
                Some(mut worker) => {
                    match worker.runtime{
                        Some(runtime) => {
                            runtime.lock().unwrap().stop();
                        }
                        None => {
                            if let Some(thread) = worker.thread.take(){
                                thread.join().unwrap();
                            }
                        }
                    }
                }
                None => {}
            }
        }
    }
}
