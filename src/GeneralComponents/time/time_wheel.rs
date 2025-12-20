use std::{
    collections::{ HashMap},
    time::{Instant, Duration as StdDuration},
    future::Future,
    pin::Pin,
    task::{Context, Poll,Waker},
};
use log::{debug, info};

pub struct TimeWheel {
    slots: HashMap<Instant, Vec<TimeTask>>,
    slot_duration: StdDuration,
    current_time: Instant,
    waker:Option<Waker>,
}
impl TimeWheel {
    pub fn new(slot_duration: StdDuration) -> Self {
        Self {
            slots: HashMap::new(),
            slot_duration,
            current_time: Instant::now(),
            waker:None,
        }
    }

    // 添加任务到时间轮
    pub fn add_task(&mut self,start_time: Instant,time:StdDuration)->Result<(),String>{
        let end_time = start_time + time;
        if self.current_time <= start_time{
            let duration = (end_time-self.current_time).as_micros();
            let slot_duration = self.slot_duration.as_micros();
            
            // 防止除以0
            if slot_duration == 0 {
                return Err("步长时间不能为0".to_string());
            }
            
            let num = duration / slot_duration;
            let timekey = self.current_time + self.slot_duration * num as u32;
            self.slots.entry(timekey).or_default().push(TimeTask{
                point:end_time,
                waker:None,
            });
            Ok(())
        }else{
            Err("开始时间必须大于当前时间".to_string())
        }
    }

    // 模拟时间轮的轮询，使用基准时间与时间步长来计算需要唤醒的时间槽
    // 并从哈希表中移除已唤醒的任务
    fn poll(&mut self) {
        let current_time = Instant::now();
        let time_len = (current_time - self.current_time).as_micros();
        let slot_duration = self.slot_duration.as_micros();
        
        // 防止除以0
        let num = if slot_duration > 0 {
            time_len / slot_duration
        } else {
            0
        };
        
        for i in 0..num {
            let time = self.current_time + self.slot_duration * i as u32;
            if let Some(wakers) = self.slots.remove(&time) {
                for waker in wakers {
                    if let Some(waker) = waker.waker{
                        waker.wake();
                    }
                }
            }
        }
        let next_time = self.current_time + self.slot_duration * (num+1) as u32;
        if let Some(tasks) = self.slots.get_mut(&next_time) {
            // 收集需要唤醒的任务索引
            let mut to_wake = Vec::new();
            for (i, task) in tasks.iter().enumerate() {
                if task.point <= current_time {
                    to_wake.push(i);
                }
            }
            // 从后往前唤醒任务并移除，避免索引移位
            for &i in to_wake.iter().rev() {
                if let Some(waker) = tasks.remove(i).waker {
                    waker.wake();
                }
            }
        }
        self.current_time = self.current_time + self.slot_duration * num as u32;
    }

    pub fn take_waker(&mut self)->Option<Waker>{
        self.waker.take()
    }
}

impl Future for TimeWheel {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::get_mut(self);
        if this.slots.is_empty(){
            Poll::Ready(())
        }else{
            this.waker = Some(cx.waker().clone());
            this.poll();
            Poll::Pending
        }
    }
}

struct TimeTask{
    point:Instant,
    waker:Option<Waker>,
}