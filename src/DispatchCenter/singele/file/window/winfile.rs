use std::{
    task::{Waker,Context,Poll},
    rc::Rc,
    cell::RefCell,
    pin::Pin,
};
use futures::Stream;
use windows::{
    Win32::System::IO::*,
    Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, *},
    Win32::Foundation::{HANDLE, GENERIC_READ, GENERIC_WRITE},
    core::PCWSTR
};
use super::evenloop::WinFileCenter;
use crate::DispatchCenter::singele::file::FileCenter;

//WinFile结构体，用于表示Windows文件句柄以及提供读写方法
pub struct WinFile{
    id:u64,
    center:Rc<RefCell<dyn FileCenter>>,
    handle:HANDLE,
    
}

impl WinFile{
    pub fn new(center:Rc<RefCell<dyn FileCenter>>,path:&str)->windows::core::Result<Self>{
        let wide_path: Vec<u16> = path
        .encode_utf16()
        .chain(std::iter::once(0))  // 添加null终止符
        .collect();
        let handle=unsafe{
            CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0 | FILE_APPEND_DATA.0,
                FILE_SHARE_READ|FILE_SHARE_WRITE,
                None,
                OPEN_ALWAYS,
                FILE_FLAG_OVERLAPPED,
                None
            )
        }?;
        
        let id = {
            let mut center_borrow = center.borrow_mut();
            let win_file_center = center_borrow.as_any_mut().downcast_mut::<WinFileCenter>()
                .expect("Expected WinFileCenter");
            win_file_center.register(handle)
        };
        
        Ok(Self{
            id,
            center,
            handle,
        })
    }

    //检查异步操作是否完成
    //如果操作完成，返回写入或读取的字节数
    //如果操作未完成，返回None
    pub fn check(handle:HANDLE, overlapped:*mut OVERLAPPED)->Result<Option<usize>,String>{
        if overlapped.is_null(){
            return Err("Overlapped is null".to_string());
        }
        let mut bytes_transferred: u32 = 0;
        let result = unsafe{
            GetOverlappedResult(
                handle,
                overlapped,
                &mut bytes_transferred,
                false
            )
        };
        
        if result.is_ok(){
            if bytes_transferred == 0 {
                Ok(None) // 文件结束
            } else {
                Ok(Some(bytes_transferred as usize))
            }
        } else {
            let error = result.unwrap_err();
            let code = error.code().0 as u32;
            match code {
                0xC0000005 => Err("访问被拒绝".to_string()),
                0xC0000011 => Ok(None), // 文件结束
                0xC0000016 => Err("文件不存在".to_string()),
                0xC0000022 => Err("拒绝访问".to_string()),
                0xC0000043 => Err("共享冲突".to_string()),
                0xC000009C => Err("资源不足".to_string()),
                0xC0000120 => Err("操作被取消".to_string()),
                _ => Err(format!("未知错误:0x{:X}", code)),
            }
        }
    }
    
    //创建一个读取流，用于异步读取文件内容
    pub fn read(self:&mut Self)->WinReadStream<'_>{
        let mut overlapped=Box::new(OVERLAPPED::default());
        let overlapped_ptr = overlapped.as_mut() as *mut OVERLAPPED;
        let overlapped_id = overlapped_ptr as usize;
        let readfile=WinReadStream{
            file:self,
            buffer:[0;1024],
            waker:None,
            overlapped,
            overlapped_id,
            pending:false,
            error:false,
        };
        readfile
    }

    //创建一个写入流，用于异步写入文件内容
    pub fn write(self:&mut Self,buffer:Vec<u8>,writeside:Option<u64>)->WinWriteStream<'_>{
        //writeside参数描述位距离文件开始位置的偏移量，根据高低位偏移，None表示追加写入
        let mut overlapped=Box::new(OVERLAPPED::default());
        let mut file_size = 0i64;
        if let Some(side)=writeside{
            unsafe {
                overlapped.Anonymous.Anonymous.Offset = (side & 0xFFFFFFFF) as u32;
                overlapped.Anonymous.Anonymous.OffsetHigh = (side >> 32) as u32;
            }
        }else{
            unsafe {
                let _ = GetFileSizeEx(self.handle, &mut file_size);
            }
            let file_size_u64 = file_size as u64;
            unsafe {
                overlapped.Anonymous.Anonymous.Offset = (file_size_u64 & 0xFFFFFFFF) as u32;
                overlapped.Anonymous.Anonymous.OffsetHigh = (file_size_u64 >> 32) as u32;
            }
        }
        let overlapped_ptr = overlapped.as_mut() as *mut OVERLAPPED;
        let overlapped_id = overlapped_ptr as usize;
        let writefile=WinWriteStream{
            file:self,
            buffer,
            waker:None,
            overlapped,
            overlapped_id,
            pending:false,
            error:false,
        };
        writefile
    }
}

pub struct WinReadStream<'a>{
    file:&'a mut WinFile,
    buffer:[u8;1024],
    waker:Option<Waker>,
    overlapped:Box<OVERLAPPED>,
    overlapped_id:usize,
    pending:bool,
    error:bool,
}

impl Stream for WinReadStream<'_>{
    type Item=Result<Vec<u8>,String>;
    fn poll_next(mut self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<Option<Self::Item>>{
        if self.error{
            return Poll::Ready(None);
        }
        if !self.pending {
            let mut bytes_read: u32 = 0;
            let overlapped_ptr = self.overlapped.as_mut() as *mut OVERLAPPED;
            let ret=unsafe{
                ReadFile(
                    self.file.handle,
                    Some(&mut self.buffer)                              ,
                    Some(&mut bytes_read),
                    Some(overlapped_ptr),
                )
            };
            
            match ret {
                Ok(_) => {
                    if bytes_read == 0 {
                        Poll::Ready(None)
                    } else {
                        // 手动更新文件指针位置
                        let current_offset = unsafe {
                            self.overlapped.Anonymous.Anonymous.Offset as u64 + 
                            self.overlapped.Anonymous.Anonymous.OffsetHigh as u64 * 
                            (u32::MAX as u64 + 1)
                        };
                        let new_offset = current_offset + bytes_read as u64;
                        unsafe {
                            self.overlapped.Anonymous.Anonymous.Offset = (new_offset & 0xFFFFFFFF) as u32;
                            self.overlapped.Anonymous.Anonymous.OffsetHigh = (new_offset >> 32) as u32;
                        };
                        
                        let data=self.buffer[..bytes_read as usize].to_vec();
                        Poll::Ready(Some(Ok(data)))
                    }
                }
                Err(e) => {
                    let code = e.code().0;
                    if code == 997 || code == -2147023899 {
                        self.waker=Some(cx.waker().clone());
                        self.file.center.borrow_mut().listinto(self.overlapped_id,cx.waker().clone());
                        self.pending = true;
                        Poll::Pending
                    } else {
                        self.error=true;
                        Poll::Ready(Some(Err(format!("ReadFile failed: {:?}", e))))
                    }
                }
            }
        } else {
            let overlapped_ptr = self.overlapped.as_mut() as *mut OVERLAPPED;
            let result=WinFile::check(self.file.handle, overlapped_ptr);
            match result{
                Ok(Some(0))=>{
                    self.pending = false;
                    Poll::Ready(None)
                }
                Ok(Some(bytes_read))=>{
                    self.pending = false;
                    // 手动更新文件指针位置
                    let current_offset = unsafe {
                        self.overlapped.Anonymous.Anonymous.Offset as u64 + 
                        self.overlapped.Anonymous.Anonymous.OffsetHigh as u64 * 
                        (u32::MAX as u64 + 1)
                    };
                    let new_offset = current_offset + bytes_read as u64;
                    unsafe {
                        self.overlapped.Anonymous.Anonymous.Offset = (new_offset & 0xFFFFFFFF) as u32;
                        self.overlapped.Anonymous.Anonymous.OffsetHigh = (new_offset >> 32) as u32;
                    };
                    
                    let data=self.buffer[..bytes_read as usize].to_vec();
                    Poll::Ready(Some(Ok(data)))
                }
                Ok(None)=>{
                    self.pending = false;
                    Poll::Ready(None)
                }
                Err(e)=>{
                    self.pending = false;
                    self.error=true;
                    Poll::Ready(Some(Err(e)))
                }
            }
        }
    }
}

pub struct WinWriteStream<'a>{
    file:&'a mut WinFile,
    buffer:Vec<u8>,
    overlapped:Box<OVERLAPPED>,
    overlapped_id:usize,
    pending:bool,
    error:bool,
    waker:Option<Waker>,
}

impl Stream for WinWriteStream<'_>{
    type Item = Result<usize, String>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.error{
            return Poll::Ready(None);
        }
        if !self.pending{
            let mut bytes_written=0u32;
            let overlapped_ptr = self.overlapped.as_mut() as *mut OVERLAPPED;
            let ret=unsafe{
                WriteFile(
                    self.file.handle,
                    Some(self.buffer.as_mut()),
                    Some(&mut bytes_written),
                    Some(overlapped_ptr),
                )
            };
            match ret{
                Ok(_)=>{
                    self.pending=true;
                    self.file.center.borrow_mut().listinto(self.overlapped_id,cx.waker().clone());
                    self.buffer.drain(..bytes_written as usize);
                    Poll::Ready(Some(Ok(bytes_written as usize)))
                }
                Err(e)=>{
                    let code = e.code().0;
                    if code == 997 || code == -2147023899 {
                        self.waker=Some(cx.waker().clone());
                        self.file.center.borrow_mut().listinto(self.overlapped_id,cx.waker().clone());
                        self.pending = true;
                        Poll::Pending
                    } else {
                        self.error=true;
                        Poll::Ready(Some(Err(format!("WriteFile failed: {:?}", e))))
                    }
                }
            }
        }else{
            let overlapped_ptr = self.overlapped.as_mut() as *mut OVERLAPPED;
            let result=WinFile::check(self.file.handle, overlapped_ptr);
            match result{
                Ok(Some(0))=>{
                    self.pending = false;
                    Poll::Ready(None)
                }
                Ok(Some(bytes_written))=>{
                    self.pending = false;
                    self.buffer.drain(..bytes_written as usize);
                    Poll::Ready(Some(Ok(bytes_written as usize)))
                }
                Ok(None)=>{
                    self.pending = false;
                    Poll::Ready(None)
                }
                Err(e)=>{
                    self.error=true;
                    Poll::Ready(Some(Err(e)))
                }
            }
        }
    }
}



#[cfg(test)]
mod test{
    use super::*;
    use crate::DispatchCenter::singele::singele_runtime::SingeleRuntime;
    use log::{info,LevelFilter};
    use env_logger;
    use std::time::Duration;
    fn setup_logger(level:LevelFilter){
            env_logger::Builder::new()
            .filter_level(level)
            .filter_module("Aura::DispatchCenter::singele::file::window", LevelFilter::Trace) 
            .filter_module("mio", LevelFilter::Warn)
            .is_test(true)
            .format_timestamp_nanos()
            .try_init()
            .ok();
        }
    #[test]
    fn test_winfile_read(){
        use futures::StreamExt;
        setup_logger(LevelFilter::Trace);
        let file=SingeleRuntime::file("C:\\Users\\Administrator\\Desktop\\项目\\Aura\\test.txt");
        match file{
            Ok(mut file)=>{
                SingeleRuntime::run(
                    async move {
                        let mut read_stream=file.read();
                        while let Some(Ok(bytes)) = read_stream.next().await{
                            println!("Read file content: {}",String::from_utf8_lossy(&bytes));
                        }
                    }
                )
            }
            Err(e)=>{
                println!("Error opening file: {}",e);
            }
        }
    }

    #[test]
    fn test_winfile_write(){
        use futures::StreamExt;
        setup_logger(LevelFilter::Trace);
        let file=SingeleRuntime::file("C:\\Users\\Administrator\\Desktop\\项目\\Aura\\test.txt");
        match file{
            Ok(mut file)=>{
                SingeleRuntime::run(
                    async move {
                        let data="输入完毕".as_bytes().to_vec();
                        let mut write_stream=file.write(data,None);
                        while let Some(Ok(bytes_written)) = write_stream.next().await{
                            println!("Write {} bytes",bytes_written);
                        }
                        let mut read_stream=file.read();
                        while let Some(Ok(bytes)) = read_stream.next().await{
                            println!("Read file content: {}",String::from_utf8_lossy(&bytes));
                        }
                    }
                )
            }
            Err(e)=>{
                println!("Error opening file: {}",e);
            }
        }
    }
}
