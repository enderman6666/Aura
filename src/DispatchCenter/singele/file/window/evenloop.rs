use std::{
    collections::HashMap,
    task::Waker,
    any::Any,
};
use windows::{
    Win32::System::IO::*,
    Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE},
};
use crate::GeneralComponents::id_bulider::IdBulider;
use super::super::FileCenter;


//WinFileCenter结构体，用于管理Windows文件句柄的IOCP
pub struct WinFileCenter{
    id:IdBulider,
    iocp:HANDLE,
    list:HashMap<usize,Waker>,
}

impl WinFileCenter{
    pub fn new()->Self{
        Self{
            id:IdBulider::new(),
            iocp:unsafe{
                match CreateIoCompletionPort(INVALID_HANDLE_VALUE,
                    None,
                    0,
                    0
                ){
                    Ok(iocp)=>iocp,
                    Err(e)=>{
                        panic!("IOCP创建失败: {:?}", e);
                    }
                }
            },
            list:HashMap::new(),
        }
    }

    //注册文件句柄到IOCP
    pub fn register(&mut self,handle:HANDLE)->u64{
        let id=self.id.get_id();
        let ret=match unsafe{
            CreateIoCompletionPort(
                handle,
                Some(self.iocp),
                id as usize,
                0
            )
        }{
            Ok(handle)=>{
                handle
            },
            Err(e)=>{
                self.id.drop_id(id);
                panic!("IOCP注册失败: {:?}", e);
            }
        };
        unsafe{
            let _=CreateIoCompletionPort(ret,Some(self.iocp),id as usize,0);
        };
        id
    }

    }


//WinFileCenter实现FileCenter trait，用于处理文件IO事件
//在poll方法中，从IOCP中获取完成的IO事件，并唤醒对应的任务
impl FileCenter for WinFileCenter{
    fn poll(&mut self){
         let mut entries: [OVERLAPPED_ENTRY; 64] = unsafe { std::mem::zeroed() };
         let mut entries_removed: u32 = 0;
         if let Ok(_)=unsafe{
             GetQueuedCompletionStatusEx(
                 self.iocp,
                 &mut entries,
                 &mut entries_removed,
                 200,
                 true,
             )
         }{
            for i in 0..entries_removed{
                let entry=&entries[i as usize];
                let ov_prt=entry.lpOverlapped;
                if ov_prt.is_null(){
                    continue;
                }
                let ov=unsafe{&mut *ov_prt};
                let id=ov_prt.addr() as usize;
                if let Some(waker)=self.list.remove(&id){
                    waker.wake();
                }
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn listinto(&mut self, id: usize, waker: Waker) {
        self.list.insert(id, waker);
    }
}