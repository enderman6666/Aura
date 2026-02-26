use std::{
    task::{Context, Poll, Waker},
    rc::Rc,
    cell::{Cell,RefCell},
    future::Future,
    pin::Pin,
    collections::HashMap,
};
use log::info;
// 通知器,用于通知等待中的任务
#[derive(Copy,Clone)]
enum State{
    Watting,
    Notified,
}


pub struct Notify;

impl Notify {
    pub fn notifyer()->Rc<Notifyer>{
        Rc::new(Notifyer::new())
    }
}

pub struct Notifyer {
    waiters: Rc<RefCell<Vec<Waker>>>,
    stateid:Cell<u64>,
    state:Cell<State>,
}

impl Notifyer {
    pub fn new() -> Self {
        Self {
            waiters: Rc::new(RefCell::new(Vec::new())),
            stateid: Cell::new(0),
            state:Cell::new(State::Watting),
        }
    }

    pub fn notify_one(&self){
        let mut waiters = self.waiters.borrow_mut();
        if waiters.len() > 0 {
            let waker = waiters.pop().unwrap();
                waker.wake();
            self.state.set(State::Notified);
            if waiters.len()==0{
                self.state.set(State::Watting);
                self.stateid.set(self.stateid.get() + 1);
            }
        }else{
            self.state.set(State::Watting);
            self.stateid.set(self.stateid.get() + 1);
        }
    }

    pub fn notify_all(&self) {
        self.stateid.set(self.stateid.get() + 1);
        let mut waiters = self.waiters.borrow_mut();
        for waker in waiters.drain(..) {
            waker.wake();
        }
        self.state.set(State::Watting);
    }

    pub fn get_guard(self:&Rc<Self>) -> NotifyGuard {
        NotifyGuard::new(Rc::clone(&self), self.stateid.get())
    }
}

// 通知守卫,用于等待通知
pub struct NotifyGuard {
    notify: Rc<Notifyer>,
    id: u64,
    waiting: bool,
}
impl NotifyGuard {
    pub fn new(notify: Rc<Notifyer>, id: u64) -> Self {
        Self { notify, id, waiting: false }
    }
}

impl Future for NotifyGuard {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let stateid = self.notify.stateid.get();
        if stateid == self.id{
            let state=self.notify.state.get();
            if let State::Notified = state{
                Poll::Ready(())
            }else{
                if !self.waiting {
                    self.waiting = true;
                    self.notify.waiters.borrow_mut().push(cx.waker().clone());
                }
                Poll::Pending
            }
        } else {
            Poll::Ready(())
        }
    }
}


// 条件通知器,用于通过条件通知等待中的任务
struct ConditionNotify{
    wakers: Vec<Rc<Waiter>>,
}

impl ConditionNotify{
    pub fn new() -> Self {
        Self {
            wakers: Vec::new(),
        }
    }

    // 创建一个等待者,并设置条件
    pub fn create(&mut self,condition:Box<dyn Fn() -> bool+'static>)->Rc<Waiter>{
        let mut waiter=Waiter::new();
        waiter.set_condition(condition);
        self.wakers.push(Rc::new(waiter));
        self.wakers.last().unwrap().clone()
    }
}



struct Waiter{
    condition:Option<Box<dyn Fn() -> bool+'static>>,
    wakers: Rc<RefCell<Vec<Waker>>>,
    id: Cell<u64>,
    state:Cell<State>,
}

impl Waiter{
    pub fn new() -> Self {
        Self {
            condition: None,
            wakers: Rc::new(RefCell::new(Vec::new())),
            id: Cell::new(0),
            state:Cell::new(State::Watting),
        }
    }

    // 设置条件,当条件满足时,通知所有等待者(注：这里闭包的生命周期通常大于捕获的变量的生命周期，推荐使用全局变量传递数据)
    pub fn set_condition(&mut self, condition: Box<dyn Fn() -> bool+'static>) {
        self.condition = Some(condition);
    }

    // 检查条件是否满足,如果满足,则通知所有等待者
    fn check(&self)->bool{
        let mut wakers = self.wakers.borrow_mut();
        let state=self.condition.as_ref().unwrap()();
        if state{
            self.state.set(State::Notified);
            for waker in wakers.drain(..) {
                waker.wake();
            }
            self.id.set(self.id.get() + 1);
            true
        }else{
            false
        }
    }

    pub fn get_id(&self) -> u64 {
        self.id.get()
    }

    //获取等待守卫
    pub fn get_guard(self:&Rc<Self>) -> WaiterGuard {
        WaiterGuard::new(Rc::clone(&self), self.get_id())
    }
}

// 等待守卫,用于等待条件满足
struct WaiterGuard{
    notify:Rc<Waiter>,
    id:Cell<u64>,
    waiting: bool,
}

impl WaiterGuard{
    pub fn new(notify:Rc<Waiter>,id:u64) -> Self {
        Self {
            notify,
            id:Cell::new(id),
            waiting: false,
        }
    }
}

impl Future for WaiterGuard{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let id=self.id.get();
        if id!=self.notify.get_id(){
            return Poll::Ready(());
        }
        let waiter=self.notify.clone();
        if let State::Notified = self.notify.state.get(){
            return Poll::Ready(());
        }else{
            if !self.waiting {
                self.waiting = true;
                waiter.wakers.borrow_mut().push(cx.waker().clone());
            }
            Poll::Pending
        }
    }
}







#[cfg(test)]
mod tests{
    use super::*;
    use crate::DispatchCenter::singele::singele_runtime::SingeleRuntime;
    use log::{info,LevelFilter};
    use env_logger;
    use std::time::Duration;
    fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("aura::DispatchCenter::singele::sync", LevelFilter::Trace) 
            .filter_module("mio", LevelFilter::Warn)
            .filter_module("aura::DispatchCenter::singele::singele_runtime", LevelFilter::Off)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
    
    #[test]
    fn test_notify(){
        setup_logger(LevelFilter::Info);
        let notify= Notify::notifyer();
        let notify1 = notify.get_guard();
        let notify2 = notify.get_guard();
        let notify3 = notify.get_guard();
        let future = async move {
            info!("开始计时");
            SingeleRuntime::sleep(Duration::from_millis(200)).await;
            notify.notify_all();
            info!("通知所有等待者");
        };
        let future1 = async move {
            info!("等待通知1");
            notify1.await;
            info!("通知1完成");
        };
        let future2 = async move {
            info!("等待通知2");
            notify2.await;
            info!("通知2完成");
        };
        let future3 = async move {
            info!("等待通知3");
            notify3.await;
            info!("通知3完成");
        };
        let list: Vec<Pin<Box<dyn Future<Output = ()>>>> = vec![Box::pin(future),Box::pin(future1),Box::pin(future2),Box::pin(future3)];
        SingeleRuntime::run_all(list);
    }

    #[test]
    fn test_notify_one(){
        setup_logger(LevelFilter::Info);
        let notify= Notify::notifyer();
        let notify1 = notify.get_guard();
        let notify2 = notify.get_guard();
        let notify3 = notify.get_guard();
        let future = async move {
            info!("开始计时");
            SingeleRuntime::sleep(Duration::from_millis(50)).await;
            info!("通知1");
            notify.notify_one();
            info!("通知1完成");
            SingeleRuntime::sleep(Duration::from_millis(30)).await;
            info!("通知2");
            notify.notify_one();
            info!("通知2完成");
            SingeleRuntime::sleep(Duration::from_millis(40)).await;
            info!("通知3");
            notify.notify_one();
            info!("通知3完成");
            info!("通知所有等待者");
        };
        let future1 = async move {
            info!("等待通知1");
            notify1.await;
            info!("通知1完成");
        };
        let future2 = async move {
            info!("等待通知2");
            notify2.await;
            info!("通知2完成");
        };
        let future3 = async move {
            info!("等待通知3");
            notify3.await;
            info!("通知3完成");
        };
        let list: Vec<Pin<Box<dyn Future<Output = ()>>>> = vec![Box::pin(future),Box::pin(future1),Box::pin(future2),Box::pin(future3)];
        SingeleRuntime::run_all(list);
    }

    #[test]
    fn test_notify_cond(){
        use std::sync::RwLock;
        setup_logger(LevelFilter::Info);
        static N:RwLock<u64>=RwLock::new(0);
        let mut notify= ConditionNotify::new();
        let water=notify.create(Box::new(
            ||{
                if *N.read().unwrap()==1{
                    info!("条件满足");
                    true
                }else{
                    info!("条件不满足");
                    false
                }
            })
        ) ;
        let notify1 = water.get_guard();
        let notify2 = water.get_guard();
        let notify3 = water.get_guard();
        let future1 = async move {
            info!("1等待条件");
            notify1.await;
            info!("条件满足1");
        };
        let future2 = async move {
            info!("2等待条件");
            notify2.await;
            info!("条件满足2");
        };
        let future3 = async move {
            info!("3等待条件");
            notify3.await;
            info!("条件满足3");
        };
        let future4 = async move {
            info!("开始计时");
            SingeleRuntime::sleep(Duration::from_millis(200)).await;
            *N.write().unwrap()=1;
            water.check();
            info!("通知所有等待者");
        };
        let list: Vec<Pin<Box<dyn Future<Output = ()>>>> = vec![Box::pin(future1),Box::pin(future2),Box::pin(future3),Box::pin(future4)];
        SingeleRuntime::run_all(list);
    }
}


 