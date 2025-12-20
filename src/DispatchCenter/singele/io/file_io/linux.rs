use rustix::{IoUring, opcode, types};
use std::{
    os::unix::io::AsRawFd,
    fs::File,
};


struct IoFile {
    buf: Vec<u8>,
    fd: types::Fd,
}

impl IoFile {
    fn new(file_path: &str) -> Self {
        let file = File::open(file_path).unwrap();
        Self {
            buf: vec![0; 4096],
            fd: file.as_raw_fd(),
        }
    }

    pub fn set_buf(&mut self, buf: Vec<u8>) {
        self.buf = buf;
    }

    fn read(&mut self) {
        let op = opcode::Read::new(
            self.fd, 
            self.buf.as_mut_ptr(), 
            self.buf.len() as u32)
            .build();
        
    }
}