use std::ops::Deref;

pub struct IdBulider{
    current_id:u64,
    droplist:Vec<u64>,
}
impl IdBulider{
    pub fn new()->Self{
        Self{
            current_id:0,
            droplist:Vec::new(),
        }
    }

    pub fn get_id(&mut self)->u64{
        if self.droplist.is_empty(){
            let id=self.current_id;
            self.current_id+=1;
            id
        }else{
            let id=self.droplist.pop().unwrap();
            id
        }
    }

    pub fn drop_id(&mut self,id:u64){
        self.droplist.push(id);
    }
}
