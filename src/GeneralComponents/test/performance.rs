use std::time::{Duration,Instant};
use std::pin::Pin;
use log::info;

pub fn test(times:u64) -> (u64,Duration){
    let start_time=Instant::now();
    let mut value = 1u64;
        for _ in 0..times {
            value = value.wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
        }
    std::hint::black_box(value);
    (times,Instant::now()-start_time)
}

pub fn create_test(task_len:u64) -> Vec<Pin<Box<dyn Future<Output=()> + Send>>>{
    let mut tasks:Vec<Pin<Box<dyn Future<Output=()> + Send>>> = Vec::new();
    let times=[100,1000,10000,100000];
    for _ in 0..task_len{
        for i in 0..times.len(){
            tasks.push(Box::pin(async move {
                let (times,duration) = test(times[i]);
            }));
        }
    }
    tasks
}

pub fn compare_performance(task_len:u64){
    let times=[100];
    for _ in 0..task_len{
        for i in 0..times.len(){
            let (times,duration) = test(times[i]);
            info!("计算{}次,运行时间: {:?}",times,duration);
        }
    }
}

#[cfg(test)]
mod performance_test{
    use super::*;
    use crate::DispatchCenter::multi::scheduler::Scheduler;
    use log::{info,LevelFilter};
    fn setup_logger(level:LevelFilter){
        env_logger::Builder::new()
            .filter_level(level)
            .filter_module("Aura::DispatchCenter::multi::scheduler", LevelFilter::Off) 
            .filter_module("Aura::DispatchCenter::multi::runtime", LevelFilter::Off) 
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
    }
    fn test_performance(thread_num:usize,task_len:u64){
        setup_logger(LevelFilter::Off);
        Scheduler::new(thread_num);
        let tasks=create_test(task_len);
        let start_time=Instant::now();
        info!("测试开始，线程数: {}, 任务数: {}",thread_num,task_len);
        Scheduler::add(tasks);
        Scheduler::join();
        let end_time=Instant::now();
        info!("测试结束，运行时间: {:?}",end_time-start_time);
    }
    
    
    
    #[test]
    fn async_test(){
        test_performance(4,1000);
    }

    #[test]
    fn compare_test(){
        compare_performance(1000);
    }

}