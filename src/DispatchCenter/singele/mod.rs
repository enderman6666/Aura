mod singele_runtime;
mod task;
mod join_handle;
mod time;
mod sync;
mod evenloop;
mod io;


#[cfg(test)]
mod test_mod{
    use super::*;
    mod test_singele_runtime{
        use crate::DispatchCenter::singele::singele_runtime::SingeleRuntime;
        use std::thread;
        #[test]
        fn test_singele_runtime(){
            env_logger::try_init().ok();
            SingeleRuntime::run(
                async{
                    let join_handle_1 = SingeleRuntime::spawn(async{
                        1
                    });
                    let join_handle_2 = SingeleRuntime::spawn(async{
                        2
                    });
                    let result_1 = join_handle_1.await;
                    let result_2 = join_handle_2.await;
                    assert_eq!(result_1+result_2+3, 6);
                }
            );
        }
        #[test]
        fn  thread_test(){
            env_logger::try_init().ok();
            let thread_1 = thread::spawn(||{
                SingeleRuntime::run(
                    async{
                        let join_handle_1 = SingeleRuntime::spawn(async{
                            1
                        });
                        let join_handle_2 = SingeleRuntime::spawn(async{
                            2
                        });
                        let result_1 = join_handle_1.await;
                        let result_2 = join_handle_2.await;
                        assert_eq!(result_1+result_2+3, 6);
                    }
                );
            });
            let thread_2 = thread::spawn(||{
                SingeleRuntime::run(
                    async{
                        let join_handle_1 = SingeleRuntime::spawn(async{
                            7
                        });
                        let join_handle_2 = SingeleRuntime::spawn(async{
                            8
                        });
                        let result_1 = join_handle_1.await;
                        let result_2 = join_handle_2.await;
                        assert_eq!(result_1+result_2+9, 24);
                    }
                );
            });
            thread_2.join().unwrap();
            thread_1.join().unwrap();
        }
    }
}


