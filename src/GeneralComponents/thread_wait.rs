use std::sync::{Mutex,Condvar};


pub struct ThreadWait{
    condvar: Condvar,
    mutex: Mutex<bool>,
    func: Box<dyn Fn(usize) -> bool+Send+Sync+'static>,
}

impl ThreadWait{
    pub fn new(func: Box<dyn Fn(usize) -> bool+Send+Sync+'static>)->Self{
        Self{
            condvar: Condvar::new(),
            mutex: Mutex::new(false),
            func,
        }
    }

    pub fn wait(& self){
        let mut wait = self.mutex.lock().unwrap();
        while !*wait {
            wait = self.condvar.wait(wait).unwrap();
        }
    }

    pub fn check(&self,i:usize){
        let func=&self.func;
        match self.mutex.lock(){
            Ok(mut wait) => {
                *wait = func(i);
                if *wait {
                    self.condvar.notify_one();
                }
            },
            Err(_) => {},
        }
    }

}