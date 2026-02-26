# Aura

一个基于 Rust 的高性能异步运行时框架

## 项目简介

Aura 是一个用 Rust 编写的异步运行时框架，提供了单线程和多线程两种运行时模式，支持任务调度、网络编程、文件 I/O、无锁数据结构等核心功能。（该项目由个人开发，意在学习交流，部分功能可能存在问题，仍在完善中，请勿随意部署，由该项目导致的任何后果本人概不负责）

## 特性

- **多线程运行时**：支持多线程任务调度与负载均衡
- **单线程运行时**：轻量级单线程异步执行
- **协程调度**：基于 Work-Stealing 算法的任务调度
- **网络编程**：TCP/UDP Socket 支持
- **文件异步 I/O**：Windows 平台异步文件操作
- **同步原语**：Channel、Mutex、Semaphore、Notify 等
- **时间管理**：基于时间轮的高效定时器
- **无锁数据结构**：Lock-Free 队列、原子队列等高性能组件

## 架构概览

```
src/
├── DispatchCenter/          # 调度中心
│   ├── multi/               # 多线程运行时
│   │   ├── runtime.rs       # 运行时核心
│   │   ├── scheduler.rs     # 全局调度器
│   │   ├── task.rs          # 任务定义
│   │   ├── join.rs          # 任务 Join
│   │   ├── time.rs          # 定时器
│   │   ├── net/             # 网络模块
│   │   │   ├── tcp.rs
│   │   │   ├── udp.rs
│   │   │   └── evenloop.rs  # 事件循环
│   │   └── sync/            # 同步原语
│   │       ├── channel.rs
│   │       ├── lock.rs
│   │       ├── notify.rs
│   │       └── semaphore.rs
│   │
│   └── singele/             # 单线程运行时
│       ├── singele_runtime.rs
│       ├── task.rs
│       ├── join_handle.rs
│       ├── net/             # 网络模块
│       ├── sync/            # 同步原语
│       └── file/            # 文件异步 I/O
│
├── GeneralComponents/       # 通用组件
│   ├── time/
│   │   └── time_wheel.rs    # 时间轮实现
│   ├── lock_free/           # 无锁数据结构
│   │   ├── atomic_queue.rs
│   │   ├── occupying_queue.rs
│   │   └── translation_acqueue.rs
│   ├── id_bulider.rs        # ID 生成器
│   ├── exchange_list.rs     # 交换列表
│   ├── static_exchange.rs   # 静态交换
│   └── thread_wait.rs       # 线程等待
│
└── asyncIO/                 # 异步 I/O
    └── token_center/        # Token 管理
```

## 核心模块

### DispatchCenter

调度中心是 Aura 的核心，负责任务的创建、调度和执行。

- **Runtime**：运行时实例，管理单个线程的任务队列
- **Scheduler**：全局调度器，协调多个运行时的工作，支持工作窃取负载均衡
- **Task**：异步任务单元
- **JoinHandle**：任务句柄，用于等待任务完成

### 多线程运行时 (multi)

#### 核心组件

- **runtime.rs**：线程级运行时，管理本地任务队列和时间轮
- **scheduler.rs**：全局调度器，负责任务分配和线程管理
- **task.rs**：异步任务的定义和执行
- **join.rs**：任务等待和结果获取
- **time.rs**：定时器功能

#### 网络模块 (net)

- **tcp.rs**：TCP 服务器和客户端实现
- **udp.rs**：UDP 通信实现
- **evenloop.rs**：事件循环，基于 MIO

#### 同步原语 (sync)

- **channel.rs**：无界通道，用于线程间通信
- **lock.rs**：异步互斥锁和读写锁
- **notify.rs**：通知机制，支持一对一和广播通知
- **semaphore.rs**：信号量实现
- **translation_channel.rs**：转换通道

### 单线程运行时 (singele)

- **singele_runtime.rs**：单线程运行时核心
- **task.rs**：单线程任务实现
- **join_handle.rs**：单线程任务句柄
- **time.rs**：单线程定时器

#### 网络模块 (net)

- **tcp.rs**：单线程 TCP 实现
- **udp.rs**：单线程 UDP 实现

#### 文件模块 (file)

- **window/winfile.rs**：Windows 平台异步文件操作

#### 同步原语 (sync)

- **channel.rs**：单线程通道
- **lock.rs**：单线程锁
- **notify.rs**：单线程通知
- **semaphore.rs**：单线程信号量

### GeneralComponents

#### 时间轮 (TimeWheel)

基于时间轮的高效定时器实现，支持微秒级精度，O(1) 时间复杂度。

#### 无锁数据结构 (lock_free)

- **atomic_queue.rs**：无锁原子队列
- **occupying_queue.rs**：占用队列
- **translation_acqueue.rs**：转换队列
- **link_list.rs**：无锁链表
- **prt.rs**：原子指针封装

#### 线程间通信

- **exchange_list.rs**：线程间交换列表
- **static_exchange.rs**：静态交换机制
- **thread_wait.rs**：线程等待机制

#### 工具组件

- **id_bulider.rs**：ID 生成器
- **file/byte_stream.rs**：字节流处理
- **test/performance.rs**：性能测试工具

## 依赖

```toml
[dependencies]
dashmap = "5.4"
chrono = "0.4.42"
futures = "0.3.31"
log = "0.4.29"
mio = "1.1.0"
windows = "0.62.2"
env_logger = "0.11.8"
loom = "0.7"
```

## 使用示例

### 多线程运行时

```rust
use Aura::DispatchCenter::multi::scheduler::Scheduler;
use std::pin::Pin;

fn main() {
    // 创建多线程调度器，4 个工作线程
    Scheduler::new(4);
    
    // 创建异步任务
    let future1 = async {
        println!("future1");
    };
    let future2 = async {
        println!("future2");
    };
    
    // 添加任务到调度器
    Scheduler::add(vec![Box::pin(future1), Box::pin(future2)]);
    
    // 等待所有任务完成
    Scheduler::join();
}
```

### 任务 Spawn

```rust
use Aura::DispatchCenter::multi::scheduler::Scheduler;

fn main() {
    Scheduler::new(2);
    
    let future = async {
        3 + 4
    };
    
    let future1 = async {
        let n = Scheduler::spawn(future).await;
        assert!(n == 7);
    };
    
    Scheduler::add(vec![Box::pin(future1)]);
    Scheduler::join();
}
```

### 单线程运行时

```rust
use Aura::DispatchCenter::singele::singele_runtime::SingeleRuntime;

fn main() {
    SingeleRuntime::run(
        async {
            println!("Hello from single-threaded runtime!");
            let a = 1;
            let b = 2;
            assert_eq!(a + b, 3);
        }
    );
}
```

### 单线程运行时 Spawn

```rust
use Aura::DispatchCenter::singele::singele_runtime::SingeleRuntime;

fn main() {
    SingeleRuntime::run(
        async {
            let a = 1;
            let b = SingeleRuntime::spawn(async {
                2
            }).await;
            assert_eq!(a + b, 3);
        }
    );
}
```

### 单线程运行时多任务

```rust
use Aura::DispatchCenter::singele::singele_runtime::SingeleRuntime;
use std::pin::Pin;

fn main() {
    let futures: Vec<Pin<Box<dyn std::future::Future<Output=()>>>> = vec![
        Box::pin(async {
            assert_eq!(2 * 1, 2);
        }),
        Box::pin(async {
            assert_eq!(2 * 2, 4);
        }),
        Box::pin(async {
            assert_eq!(3 * 3, 9);
        }),
    ];
    
    SingeleRuntime::run_all(futures);
}
```

### 定时器

```rust
use Aura::DispatchCenter::multi::scheduler::Scheduler;
use std::time::{Duration, Instant};

fn main() {
    Scheduler::new(2);
    
    let future1 = async {
        let start_time = Instant::now();
        let sleep = Scheduler::sleep(Duration::from_millis(100));
        sleep.await;
        let end_time = Instant::now();
        assert!(end_time - start_time >= Duration::from_millis(100));
    };
    
    Scheduler::add(vec![Box::pin(future1)]);
    Scheduler::join();
}
```

### 网络编程 (TCP)

```rust
use Aura::DispatchCenter::multi::scheduler::Scheduler;
use Aura::DispatchCenter::multi::net::tcp::{TcpServer, TcpConnet};
use futures::StreamExt;

fn main() {
    Scheduler::new(2);
    
    let mut listener = TcpServer::bind("127.0.0.1:6666".parse().unwrap());
    let connet = TcpConnet::bind("127.0.0.1:6666".parse().unwrap()).unwrap();
    let s = "hello world".to_string();
    
    let future1 = async move {
        while let Some(Ok((stream, addr))) = listener.next().await {
            println!("accept: {:?}", addr);
            let future2 = async move {
                let mut data = Vec::new();
                while let Some(d) = stream.recv().next().await {
                    match d {
                        Ok(d) => {
                            data.extend(d);
                        }
                        Err(e) => {
                            println!("recv error: {:?}", e);
                        }
                    }
                }
                data.into_iter().map(|x| x as char).collect::<String>()
            };
            let data = Scheduler::spawn(future2).await;
            println!("recv: {:?}", data);
            assert_eq!(data, "hello world");
        }
    };
    
    let future2 = async move {
        let c = connet.await;
        match c {
            Ok(stream) => {
                println!("connect success");
                while let Some(data) = stream.send(s.clone().into_bytes()).next().await {
                    println!("send: {:?}", data);
                }
            }
            Err(e) => {
                println!("connect error: {:?}", e);
            }
        }
    };
    
    Scheduler::add(vec![Box::pin(future1), Box::pin(future2)]);
    Scheduler::join();
}
```

### 性能测试

```rust
use Aura::GeneralComponents::test::performance::{create_test, test_performance};

fn main() {
    // 测试 4 个线程，1000 个任务
    test_performance(4, 1000);
}
```

### 同步原语示例

#### 通道 (Channel)

```rust
use Aura::DispatchCenter::multi::sync::channel::Channel;
use Aura::DispatchCenter::multi::scheduler::Scheduler;

fn main() {
    Scheduler::new(2);
    
    let (mut sender, mut receiver) = Channel::new();
    
    let send_task = async move {
        for i in 0..5 {
            sender.send(i).await.unwrap();
            println!("Sent: {}", i);
        }
    };
    
    let recv_task = async move {
        for _ in 0..5 {
            if let Ok(data) = receiver.recv().await {
                println!("Received: {}", data);
            }
        }
    };
    
    Scheduler::add(vec![Box::pin(send_task), Box::pin(recv_task)]);
    Scheduler::join();
}
```

#### 互斥锁 (AsMutex)

```rust
use Aura::DispatchCenter::multi::sync::lock::AsMutex;
use Aura::DispatchCenter::multi::scheduler::Scheduler;

fn main() {
    Scheduler::new(2);
    
    let mutex = AsMutex::new(0);
    
    let task1 = async {
        let mut guard = mutex.lock().await;
        *guard += 1;
        println!("Task 1: {}", *guard);
    };
    
    let task2 = async {
        let mut guard = mutex.lock().await;
        *guard += 1;
        println!("Task 2: {}", *guard);
    };
    
    Scheduler::add(vec![Box::pin(task1), Box::pin(task2)]);
    Scheduler::join();
}
```

#### 通知 (Notify)

```rust
use Aura::DispatchCenter::multi::sync::notify::Notify;
use Aura::DispatchCenter::multi::scheduler::Scheduler;

fn main() {
    Scheduler::new(2);
    
    let notify = Notify::new();
    
    let wait_task = async {
        let guard = notify.get();
        println!("Waiting for notification...");
        guard.await;
        println!("Notification received!");
    };
    
    let notify_task = async move {
        // 等待一下，确保等待任务已经开始
        Scheduler::sleep(std::time::Duration::from_millis(100)).await;
        println!("Sending notification...");
        notify.notify_one();
    };
    
    Scheduler::add(vec![Box::pin(wait_task), Box::pin(notify_task)]);
    Scheduler::join();
}
```

#### 信号量 (Semaphore)

```rust
use Aura::DispatchCenter::multi::sync::semaphore::Semaphore;
use Aura::DispatchCenter::multi::scheduler::Scheduler;

fn main() {
    Scheduler::new(3);
    
    // 创建一个容量为 2 的信号量
    let semaphore = Semaphore::new(2);
    
    async fn worker(id: usize, sem: &Semaphore) {
        println!("Worker {} waiting for permit...", id);
        let permit = sem.acquire().await;
        println!("Worker {} acquired permit", id);
        
        // 模拟工作
        Scheduler::sleep(std::time::Duration::from_millis(100)).await;
        
        drop(permit); // 释放许可
        println!("Worker {} released permit", id);
    }
    
    let tasks: Vec<_> = (0..5).map(|i| {
        Box::pin(worker(i, &semaphore))
    }).collect();
    
    Scheduler::add(tasks);
    Scheduler::join();
}
```

## 性能特性

- **工作窃取**：多线程间自动负载均衡
- **时间轮**：高精度定时器，O(1) 复杂度
- **无锁队列**：高并发场景下的无锁数据结构
- **MIO 事件驱动**：基于 epoll/kqueue/IOCP 的高效 I/O

## 目标平台

- **Windows** (主要支持)
- 跨平台支持 (部分模块)

## 开发状态

Aura 是一个正在积极开发中的异步运行时框架，提供了现代异步应用所需的核心功能。

## 许可证

Apache-2.0 License

## 作者

enderman <2757482051@qq.com>

## 相关链接

- [GitHub](https://github.com/enderman6666/Aura)
- [文档](https://docs.rs/Aura)
