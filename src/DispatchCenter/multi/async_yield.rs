use std::task::{Context, Poll};
use std::pin::Pin;


/// 异步yield
/// 在当前任务的节点切换出去并下次执行
pub struct AsyncYield{
    wake:bool,
}

impl AsyncYield{
    pub fn new() -> Self{
        Self{
            wake:false,
        }
    }
}

impl Future for AsyncYield{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.wake{
            Poll::Ready(())
        }else{
            self.wake=true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests{
    use std::sync::{Arc, Mutex};
    use super::*;
    use super::super::scheduler::Scheduler;
    #[test]
    fn test_async_yield(){
        let n=Arc::new(Mutex::new(0));
        let n1=Arc::clone(&n);
        let n2=Arc::clone(&n);
        let future1=async move {
            let mut n=n1.lock().unwrap();
            *n+=1;
            println!("future1 done");
        };
        let future2=async move{
            loop{
                if *n2.lock().unwrap() == 1{
                    break;
                }else{
                    println!("future2 yield");
                    AsyncYield::new().await;
                }
            }
        };
        Scheduler::new(2);
        Scheduler::add(vec![Box::pin(future2),Box::pin(future1)]);
        Scheduler::join();
    }
}

