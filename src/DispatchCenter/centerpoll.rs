use std::{sync::{Arc, Mutex}, 
    task::{Waker,Wake},
    thread::{self, Thread},
};

use super::runtime::{AuraFuture, Aura};
use super::super::threadpool::thread_center::ThreadCenter;
use super::super::asyncIO::{multi_center::MultiCenter, token_center::builder::TokenCenter};

enum SchedulingLevel {
    ThreadProactive,
    CenterScheduling
}

type mulitfuture<T>=Arc<Mutex<AuraFuture<T>>>;
//多线程模式的中心调度
pub struct CenterPoll <T: 'static> {
    pub all_future: Arc<Mutex<Vec<mulitfuture<T>>>>,//所有的future
    pub ready_future: Arc<Mutex<Vec<mulitfuture<T>>>>,//就绪的future
    pub singel_runtime:Arc<Mutex<Vec<Arc<Mutex<Aura<T>>>>>>,//子线程运行时
    pub threadpool: Option<ThreadCenter<T>>,//线程池
    start: bool,//是否启动
    scheduling_level: SchedulingLevel,//调度级别
    net_event: Option<Arc<Mutex<MultiCenter<T>>>>,//网络事件中心
}

impl<T: 'static> CenterPoll<T> {
    pub fn new(runtimes: Vec<Option<Arc<Mutex<Aura<T>>>>>) -> Self {
        let mut singel_runtime = Vec::new();
        let threadpool =ThreadCenter::new(runtimes);
        for runtime in threadpool.workers.iter(){
            if let Some(wroker) = runtime{
                if let Some(runtime) = wroker.runtime.as_ref(){
                    singel_runtime.push(Arc::clone(runtime));
                }
            }
        }
        Self {
            all_future: Arc::new(Mutex::new(Vec::new())),
            ready_future: Arc::new(Mutex::new(Vec::new())),
            singel_runtime: Arc::new(Mutex::new(singel_runtime)),
            start: false,
            threadpool: Some(threadpool),
            scheduling_level: SchedulingLevel::ThreadProactive,
            net_event: Some(Arc::new(Mutex::new(MultiCenter::new(TokenCenter::new())))),
        }
    }

    pub fn add_allfuture(&mut self, aura_future: mulitfuture<T>){
        self.all_future.lock().unwrap().push(aura_future.clone());
    }

    pub fn add_readyfuture(&mut self, aura_future: mulitfuture<T>){
        self.ready_future.lock().unwrap().push(aura_future.clone());
    }

    pub fn run(mut self)->Result<(),String>{
        if self.start {
            return Err("调度中心已启动".to_string());
        }
        self.start = true;
        self.threadpool.as_mut().unwrap().start();
        loop{
            if self.start{
                self.center_scheduling();
            } else{
                let _ = drop(self.threadpool.unwrap());
                return Ok(());
            }
        }
        
    }

    //依据负载确认调度等级
    fn check_load(&self)->SchedulingLevel{
        let mut n:usize=0;
        for aura in self.singel_runtime.lock().unwrap().iter(){
            let aura = aura.lock().unwrap().clone();
            let load =aura.waiting_future.len()+aura.ready_future.len();
            if load>3{
                n+=1;
            }
        }
        if n==self.singel_runtime.lock().unwrap().len(){
            return SchedulingLevel::CenterScheduling;
        }
        return SchedulingLevel::ThreadProactive;
    }

    //中心调度，根据调度等级进行调度
    fn center_scheduling(&mut self){
        self.scheduling_level = self.check_load();
        match self.scheduling_level{
            SchedulingLevel::ThreadProactive=>{
                thread::sleep(std::time::Duration::from_millis(100));
            }
            SchedulingLevel::CenterScheduling=>{
                let add_load = self.get_add_load();
                for (i,load) in add_load{
                    let mut aura = self.singel_runtime.lock().unwrap().get(i).unwrap().lock().unwrap().clone();
                    let mut ready_future = self.ready_future.lock().unwrap().clone();
                    for _ in 0..load{
                        if let Some(future) = ready_future.pop(){
                            aura.sqawn(future);
                        }
                    }
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    fn get_load_order(&self)->Vec<(usize,usize)>{
        let mut load_order = Vec::new();
        for (i,aura) in self.singel_runtime.lock().unwrap().iter().enumerate(){
            let aura = aura.lock().unwrap().clone();
            let load =aura.waiting_future.len()+aura.ready_future.len();
            load_order.push((i,load));
        }
        load_order.sort_by(|a,b|b.1.cmp(&a.1));
        let mut load_order = load_order.into_iter().map(|x|x).collect::<Vec<(usize,usize)>>();
        load_order
    }

    fn get_add_load(&self)->Vec<(usize,usize)>{
        let mut center_load:usize = self.ready_future.lock().unwrap().len()+self.all_future.lock().unwrap().len();
        let mut thread_load:usize=0;
        for aura in self.singel_runtime.lock().unwrap().iter(){
            let aura = aura.lock().unwrap().clone();
            let load =aura.waiting_future.len()+aura.ready_future.len();
            thread_load+=load;
        }
        let mut mid_load = (center_load+thread_load)/self.singel_runtime.lock().unwrap().len();
        let load_order = self.get_load_order();
        let mut mid_load_order = Vec::new();
        let mut n:usize =0;
        for (i,load) in load_order{
            if load<=mid_load{
                mid_load_order.push((i,mid_load-load));
            }else {
                n+=1;
            }
        }
        let mut add=mid_load*n/(self.singel_runtime.lock().unwrap().len()-n);
        let mut mid_load_order = mid_load_order.into_iter()
        .map(|x|(x.0,x.1+add)).collect::<Vec<(usize,usize)>>();
        mid_load_order
    }
}



