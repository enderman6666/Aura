use crate::asyncIO::token_center::builder::TokenCenter;
use mio::{Poll,Interest,Token,event::Source,Events,net::TcpStream};
use std::{collections::HashMap,task::Waker,time::Duration,rc::Rc,cell::RefCell};

// 事件循环,用于处理异步NETIO事件
pub struct EventLoop{
    poll:Poll,
    token_center:TokenCenter,
    eventmap:HashMap<Token,Waker>,
    events:Events,
}

impl EventLoop{
    pub fn new() -> Self {
        Self {
            poll: Poll::new().unwrap(),
            token_center: TokenCenter::new(),
            eventmap: HashMap::new(),
            events: Events::with_capacity(1024),
        }
    }

    pub fn register(&mut self, source: Rc<RefCell<dyn Source>>, interest: Interest,waker:Waker) -> Token {
        let token = self.token_center.get_token();
        let mut source = source.borrow_mut();
        self.poll.registry().register(&mut *source, token, interest).unwrap();
        self.eventmap.insert(token, waker);
        token
    }

    pub fn register_source(&mut self, source: &mut TcpStream, interest: Interest,waker:Waker) -> Token {
        let token = self.token_center.get_token();
        self.poll.registry().register(source, token, interest).unwrap();
        self.eventmap.insert(token, waker);
        token
    }

    pub fn deregister(&mut self, token: &mut Token,source: Rc<RefCell<dyn Source>>) {
        let mut source = source.borrow_mut();
        self.poll.registry().deregister(&mut *source).unwrap();
        self.eventmap.remove(&token);
    }

    pub fn poll(&mut self) -> Result<(), std::io::Error> {
        self.poll.poll(&mut self.events, Some(Duration::from_millis(0)))?;
        for event in &self.events {
            let token = event.token();
            if let Some(waker) = self.eventmap.get_mut(&token) {
                waker.wake_by_ref();
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.eventmap.is_empty()
    }
}