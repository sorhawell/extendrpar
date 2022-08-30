use extendr_api::prelude::*;

use std::sync::{Arc, Mutex};

use std::sync::mpsc;
use std::thread;

#[derive(Clone)]
struct ParFun(pub Function);

unsafe impl Send for ParFun {}
unsafe impl Sync for ParFun {}

/// @export
#[extendr]
fn par_c_api_calls() -> &'static str {
    // Concurrent and safe call of RCapi.
    // Motto "If you cannot call them, arrange to have them callen"
    //rewrite of https://www.quotery.com/quotes/cant-beat-arrange-beaten

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
                //let first child make a call
                println!("child_1: try make call with child_phone");
                let child_call = child_phone.as_ref().lock().unwrap();
                println!("child_1: has started a call");

                //start child-child thread, give this child also a phone
                let cloned_child_phone = child_phone.clone();
                thread::spawn(move || {
                    //consume copied phone and rename
                    let child_phone = cloned_child_phone;

                    //let second child make a call (when possible)
                    println!("child_2: also tries make call with child_phone but will wait\n");
                    let child_call = child_phone.as_ref().lock().unwrap();
                    println!("child_2: finally has started a call");

                    let val =
                        String::from("{print('R is working for thread_2\n'); as.character(2+3)}");
                    child_call.0.send(val).unwrap();
                    println!("child_2: sent request to main");
                    //wait for return
                    let val2 = child_call.1.recv().unwrap();
                    println!("child_2: got this answer from main: {}", val2);

                    println!("child_2: will end the call and destroy it's phone");
                });

                //ask main thread to call R-Capi
                let val = String::from("{print('R is working for thread_1\n'); as.character(2+2)}");
                child_call.0.send(val).unwrap();
                println!("child_1: has sent request to main");

                //wait for answer
                let val2 = child_call.1.recv().unwrap();
                println!("child_1: got this answer from main: {}", val2);
            } //release guard
            println!("child_1: hung up the phone / ended the call");

            // ... maybe do other stuff or acquire childphone guard again
        });
        println!("child_1: destroyed it's phone (surpringly printed first, but whatever)");
    } //dropping guard child thread can start sending requests

    //main thread serve incomming requests from child threads
    for msg in rx.iter() {
        println!("main: recieved following msg call from a child: {}", msg);
        println!("main: will execute latest recieved a message in R session");
        let robj: Robj = extendr_api::eval_string(&msg).unwrap();
        println!("main: returned from R session");
        let call_result = robj.as_str().unwrap().to_string();
        let _send_result = tx2.send(call_result);
        println!("main: thread gave R answer to a child");
    }

    println!("main: thread noticed all children destroyed their phones, time to return");

    "threadding complete"
}

// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod extendrpar;
    fn par_c_api_calls;
}
