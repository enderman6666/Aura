pub mod window;

use std::any::Any;
use std::task::Waker;

pub trait FileCenter: Any {
    fn poll(&mut self);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn listinto(&mut self, id: usize, waker: Waker);
}

