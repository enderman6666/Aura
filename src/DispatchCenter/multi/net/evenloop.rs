use mio::{
    Poll,
    Interest,
    event::Source,
    Token,
    Events,
};
use std::{task::Waker,time::Duration,cell::RefCell,rc::Rc,collections::HashMap};
use crate::asyncIO::token_center::builder::TokenCenter;
use log::info;

thread_local!{
    pub static EVENT_LOOP:RefCell<Option<Rc<EventLoop>>> = RefCell::new(None);
}

pub struct EventLoop{
    poll:RefCell<Poll>,
    token_center:RefCell<TokenCenter>,
    eventmap:RefCell<HashMap<Token,Waker>>,
    events:RefCell<Events>,
    polled:RefCell<bool>,
}

impl EventLoop{
    pub fn new(){
        let event_loop = Rc::new(Self {
            poll: RefCell::new(Poll::new().unwrap()),
            token_center: RefCell::new(TokenCenter::new()),
            eventmap: RefCell::new(HashMap::new()),
            events: RefCell::new(Events::with_capacity(1024)),
            polled:RefCell::new(false),
        });
        EVENT_LOOP.with(|cell| *cell.borrow_mut() = Some(event_loop));
    }

    pub fn get() -> Rc<Self>{
        EVENT_LOOP.with(|event_loop| event_loop.borrow_mut().as_ref().unwrap().clone())
    }


    pub fn get_token(&self) -> Token {
        self.token_center.borrow_mut().get_token()
    }

    pub fn register(& self,source: &mut dyn Source, interest: Interest,token:Token,waker:Waker){
        info!("register token: {:?}",token);
        self.poll.borrow_mut().registry().register(source, token, interest).unwrap();
        self.eventmap.borrow_mut().insert(token, waker);
    }

    pub fn deregister(& self,token: &mut Token,source: &mut dyn Source) {
        info!("deregister token: {:?}",token);
        self.poll.borrow_mut().registry().deregister(source).unwrap();
        self.eventmap.borrow_mut().remove(token);
    }

    pub fn poll(& self) -> Result<(), std::io::Error> {
        if *self.polled.borrow() {
            return Ok(());
        }
        *self.polled.borrow_mut() = true;
        self.poll.borrow_mut().poll(&mut self.events.borrow_mut(), Some(Duration::from_millis(0)))?;
        for event in self.events.borrow_mut().iter() {
            let token = event.token();
            if let Some(waker) = self.eventmap.borrow_mut().get_mut(&token){
                waker.wake_by_ref();
            }
        }
        *self.polled.borrow_mut() = false;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.eventmap.borrow().is_empty()
    }
}
