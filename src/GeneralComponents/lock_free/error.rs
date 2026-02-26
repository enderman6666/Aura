#[derive(Debug,Clone,Copy)]
pub enum QueueError{
    QueueNotRun,//队列未运行
    QueueEmpty,//队列已空
    QueueFull,//队列已满
    CasFailed,//Cas操作失败
    Unknow,//未知错误
}