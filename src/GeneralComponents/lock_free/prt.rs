use std::{
    marker::PhantomData,
    sync::atomic::{AtomicPtr, Ordering},
};


pub struct Atomic<T>{
    ptr: AtomicPtr<T>,
    _marker: PhantomData<Box<T>>,
}

impl<T> Atomic<T>{
    pub fn new(data:T) -> Self {
        Self {
            ptr: AtomicPtr::new(Box::into_raw(Box::new(data))),
            _marker: PhantomData,
        }
    }

    pub fn take(&self) -> Option<T> {
        let ptr = self.ptr.load(Ordering::Acquire);
            if !ptr.is_null() {
                if self.ptr.compare_exchange(
                    ptr,
                    std::ptr::null_mut(),
                    Ordering::AcqRel, 
                    Ordering::Acquire).is_ok(){
                    return Some(unsafe{*Box::from_raw(ptr)});
                }else{
                    return None;
                }
        }else{
            return None;
        }
    }

    pub fn load(&self) -> *mut T {
        self.ptr.load(Ordering::Acquire)
    }

    pub fn set(&self, data: T) -> Result<(), Box<T>> {
        let data = Box::into_raw(Box::new(data));
        if self.ptr.compare_exchange(
            std::ptr::null_mut(), 
            data,
            Ordering::AcqRel, 
            Ordering::Acquire).is_ok(){
                return Ok(());
        }else{
            return Err(unsafe{Box::from_raw(data)});
        }
    }

    pub fn null() -> Self {
        Self {
            ptr: AtomicPtr::new(std::ptr::null_mut()),
            _marker: PhantomData,
        }
    }

    pub fn store(&self, ptr: *mut T) {
        self.ptr.store(ptr, Ordering::Release);
    }

    pub fn swap(&self, ptr: *mut T) -> *mut T {
        self.ptr.swap(ptr, Ordering::AcqRel)
    }

    pub fn compare_exchange(&self, old: *mut T, new: *mut T) -> Result<*mut T, *mut T> {
        self.ptr.compare_exchange(
            old,
            new,
            Ordering::AcqRel, 
            Ordering::Acquire)
    }

    pub fn is_null(&self) -> bool {
        let ptr = self.ptr.load(Ordering::Acquire);
        ptr.is_null()
    }
}

impl<T> Drop for Atomic<T>{
    fn drop(&mut self) {
        let ptr = self.ptr.load(Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe{
                let _=Box::from_raw(ptr);
            }
        }
    }
}

impl<T> Clone for Atomic<T>{
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr.load(Ordering::Relaxed).into(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<T> Send for Atomic<T> {}
unsafe impl<T> Sync for Atomic<T> {}