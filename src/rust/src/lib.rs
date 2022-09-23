use extendr_api::prelude::*;

mod extendr_concurrent;
use extendr_concurrent::{concurrent_handler, ParRObj, ThreadCom};

//use std::sync::mpsc::{Receiver, Sender};
use std::sync::RwLock;
use std::thread;

use state::Storage;
static CONFIG: Storage<RwLock<Option<ThreadCom<String, ParRObj>>>> = Storage::new();

#[extendr]
fn par_con_handler() -> i32 {
    //chose outer return type

    let return_val: i32 = concurrent_handler(
        |thread_com| {
            thread_com.send("print('hej hej');1:3".to_string());
            let _answer: ParRObj = thread_com.recv();

            let mut v = Vec::new();
            for _ in 0..3 {
                thread::sleep(std::time::Duration::from_millis(3));
                //let tc_clone = thread_com.clone();

                let h = thread::spawn(move || {
                    let mut vv = Vec::new();
                    //let thread_com = tc_clone;
                    for j in 0..3 {
                        thread::sleep(std::time::Duration::from_millis(3));
                        //oups forgot to bring thread_com
                        //let tc_clone = thread_com.clone();
                        let hh = thread::spawn(move || {
                            //let thread_com = tc_clone;
                            let thread_com = CONFIG
                                .get()
                                .read()
                                .expect("failded to acquire thread_com")
                                .as_ref()
                                .unwrap()
                                .clone();

                            for _ in 0..3 {
                                thread::sleep(std::time::Duration::from_millis(15));
                                thread_com.send(format!("print('hej from thread_{}');1:4", j));
                                let _answer: ParRObj = thread_com.recv();
                            }
                            // {
                            //     let _drop_me = thread_com;
                            // }
                            //thread::sleep(std::time::Duration::from_millis(5000));
                        });
                        vv.push(hh);
                    }

                    return vv;
                });
                v.push(h);
            }
            //thread::sleep(std::time::Duration::from_millis(5000));
            //consume all handles
            v.into_iter().for_each(|h| {
                h.join()
                    .unwrap()
                    .into_iter()
                    .for_each(|hh| hh.join().unwrap())
            });

            //this is the last thread returning, all child thread can be assumed to have finnished
            //kill last threadcom to notify mainthread
            let mut val = CONFIG
                .get()
                .write()
                .expect("another thread crashed while touching CONFIG");
            dbg!("killing CONFIG threadcom");
            *val = None;

            dbg!("killing very last threadcom");
            drop(thread_com);

            //thread return is T
            42i32
        },
        |s| {
            let robj: Robj = extendr_api::eval_string(&(s.to_string())).unwrap();
            ParRObj(robj)
        },
        &CONFIG,
    );

    return_val
}

// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod extendrpar;
    fn par_con_handler;
}
