use super::exchange_list;
use crate::DispatchCenter::multi::scheduler::{TaskMsg, BackMsg};
use std::{
    i128, pin::Pin, sync::{Arc, Condvar, Mutex, OnceLock, RwLock, atomic::{AtomicBool, AtomicPtr, Ordering}}
};
use crate::DispatchCenter::multi::runtime::Status;
use super::thread_wait::ThreadWait;
use log::{info,error};


// 全局双缓冲交换列表
pub static EXCHANGE_LIST: OnceLock<Vec<((
    exchange_list::ExchangeList<TaskMsg>, // 任务下发通道
    exchange_list::ExchangeList<BackMsg>, ),//信息返回通道
    RwLock<Option<Status>>,// 公共状态锁，由运行时改变
    Arc<ThreadWait>,  //等待条件
)>> = OnceLock::new();

pub fn init(num: usize){
    if EXCHANGE_LIST.get().is_some() {
        return;
    }
    let mut list = Vec::new();
    for _ in 0..num{
        list.push(
            ((exchange_list::ExchangeList::new(),exchange_list::ExchangeList::new(),), 
            RwLock::new(None),
            Arc::new(ThreadWait::new(Box::new(|i:usize| 
                {
                    let exchange = EXCHANGE_LIST.get().unwrap()[i].1.read().unwrap();
                    let is_idle = match *exchange{
                        Some(s) => match s{
                            Status::Idle => true,
                            _ => false,
                        },
                        None => false,
                    };
                    let list =&EXCHANGE_LIST.get().unwrap()[i].0;
                    if list.1.check_write()&&is_idle{
                        true
                    }else{
                        false
                    }

                }
        )))));
    }
    let _ = EXCHANGE_LIST.set(list);
}

pub fn get_list(threadid: usize)->Option<&'static (
    exchange_list::ExchangeList<TaskMsg>, 
    exchange_list::ExchangeList<BackMsg>)>
{
    let list = EXCHANGE_LIST.get()?;
    let exchange = &list[threadid].0;
    Some(exchange)
}


pub fn loop_get_list(threadid: usize)->&'static (
    exchange_list::ExchangeList<TaskMsg>, 
    exchange_list::ExchangeList<BackMsg>)
{
    while let None = get_list(threadid){
        std::thread::sleep(std::time::Duration::from_millis(100));
        continue;
    }
    get_list(threadid).unwrap()
}

pub fn loop_get_map()->& 'static Vec<((exchange_list::ExchangeList<TaskMsg>, 
    exchange_list::ExchangeList<BackMsg>), RwLock<Option<Status>>, Arc<ThreadWait>)>
{
    while let None = EXCHANGE_LIST.get(){
        std::thread::sleep(std::time::Duration::from_millis(100));
        continue;
    }
    EXCHANGE_LIST.get().unwrap()
}

