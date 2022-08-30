use extendr_api::prelude::*;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct ParFun(pub Function);

unsafe impl Send for ParFun {}
unsafe impl Sync for ParFun {}

/// @export
#[extendr]
fn par_call_RCapi(robj: Robj) -> &'static str {
    // Concurrent and safe call of RCapi.
    // Motto "If you cannot call them, arrange to have them callen"
    //rewrite of https://www.quotery.com/quotes/cant-beat-arrange-beaten

    use std::sync::mpsc;
    use std::thread;

    //channel to send requests to main thread
    let (tx, rx) = mpsc::channel();

    //channel for main thread to answeer requestee
    let (tx2, rx2) = mpsc::channel::<String>();

    {
        //a child phone allow any child threads to ask main thread to make a call to RCapi and get back retun value.
        //child threads can all have a clone of the phone, but must wait in turn to use it.
        let child_phone = Arc::new(Mutex::new((tx, rx2)));

        thread::spawn(move || {
            //move child_phone, must not exist in main thread because channel must be destroyed when
            // no child threads no longer hold a child phone. Otherwise main thread will not hang up the phone.

            {
                dbg!("child_1 try make call with child_phone");
                let cp = child_phone.as_ref().lock().unwrap();

                let cp_child = child_phone.clone();
                thread::spawn(move || {
                    dbg!("child_2 also tries make call with child_phone");
                    let cp = cp_child.as_ref().lock().unwrap();

                    dbg!("finally child_2 has started a call");

                    let val =
                        String::from("{print('R is working for thread_2'); as.character(2+3)}");
                    cp.0.send(val).unwrap();
                    dbg!("child_2 sent request to main");
                    //wait for return
                    let val2 = cp.1.recv().unwrap();
                    dbg!("child_2 recieved an answer");
                    rprintln!("\nmain thread gave child_2 this msg: {}", val2);
                });

                dbg!("child_1 has started call");
                //ask main thread to call R-Capi
                let val = String::from("{print('R is working for thread_1'); as.character(2+2)}");
                cp.0.send(val).unwrap();
                dbg!("child_1 sent request to main");
                //wait for return
                let val2 = cp.1.recv().unwrap();
                dbg!("child_1 recieved an answer");
                rprintln!("\nmain thread gave child_1 this msg: {}", val2);
            } //release guard
            dbg!("child_1 hung up the phone");

            // ... maybe do other stuff or acquire childphone guard again
        });
    } //dropping guard child thread can start sending requests

    //main thread serve incomming requests from child threads
    for msg in rx.iter() {
        dbg!("main recieved phone call from a child");
        rprintln!(
            "\nchild is saying hey could you(Main thread) do this for me?{}",
            msg
        );
        let robj: Robj = extendr_api::eval_string(&msg).unwrap();
        dbg!("main thread talked to RCapi");
        let call_result = robj.as_str().unwrap().to_string();
        let _send_result = tx2.send(call_result);
        dbg!("main thread gave answer to child");
    }

    dbg!("main thread noticed all children dropped their phone, time to wrap up and return");

    "threadding complete"
}

// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod extendrpar;
    fn par_call_RCapi;
}
