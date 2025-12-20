use mio::Token;

pub struct TokenCenter {
    token_pool: Vec<(usize,Token)>,
    drop_token: Vec<(usize,Token)>,
    next_token: usize,
}

impl TokenCenter {
    pub fn new() -> Self {
        Self {
            token_pool: Vec::new(),
            drop_token: Vec::new(),
            next_token: 0,
        }
    }

    // 从token_center中获取一个token,从旧token中生成新token，否则按序生成
    pub fn get_token(&mut self) -> Token {
        if let Some(token) = self.drop_token.pop() {
            let (id,token) = token;
            let token = Token(id as usize);
            self.token_pool.push((id,token));
            return token;
        } else {
            let token = Token(self.next_token);
            self.token_pool.push((self.next_token,token));
            self.next_token += 1;
            token
        }
    }

    // 将token加入到drop_token中,防止新旧token冲突
    pub fn drop_token(&mut self, token: Token) {
       for (id,t) in self.token_pool.iter().enumerate(){
           if t.1 == token{
               self.drop_token.push(*t);
               self.token_pool.remove(id);
               break;
           }
       }
    }
}

impl Clone for TokenCenter {
    fn clone(&self) -> Self {
        Self {
            token_pool: self.token_pool.clone(),
            drop_token: self.drop_token.clone(),
            next_token: self.next_token,
        }
    }
}