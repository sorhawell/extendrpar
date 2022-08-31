use extendr_api::prelude::*;

use std::sync::{Arc, Mutex, MutexGuard};

use std::sync::mpsc;
use std::thread;

#[derive(Clone, Debug)]
pub struct ParRObj(pub Robj);

impl From<Robj> for ParRObj {
    fn from(robj: Robj) -> Self {
        ParRObj(robj)
    }
}

unsafe impl Send for ParRObj {}
unsafe impl Sync for ParRObj {}

#[derive(Debug, Clone)]
pub struct ChildPhone<S, R>(Arc<Mutex<(mpsc::Sender<S>, mpsc::Receiver<R>)>>);

#[derive(Debug)]
pub struct ChildCall<'a, S, R>(MutexGuard<'a, (mpsc::Sender<S>, mpsc::Receiver<R>)>);

impl<'a, S, R> ChildCall<'a, S, R> {
    pub fn send(&self, s: S) {
        self.0 .0.send(s).unwrap();
    }

    pub fn recieve(&self) -> R {
        self.0 .1.recv().unwrap()
    }
}

impl<S, R> ChildPhone<S, R> {
    pub fn open_call(&self) -> ChildCall<S, R> {
        ChildCall(self.0.as_ref().lock().unwrap())
    }

    pub fn make_request(&self, s: S) -> R {
        let child_call = self.open_call();
        child_call.send(s);
        child_call.recieve()
    }
}

pub fn do_concurrent_r<F, T, R>(f: F) -> String
where
    F: FnOnce(ChildPhone<String, R>) -> T + Send + 'static,
    T: Send + 'static,
    R: From<Robj> + Send + 'static,
{
    //start com channels
    let (tx, rx) = mpsc::channel::<String>();
    let (tx2, rx2) = mpsc::channel::<R>();
    let child_phone = ChildPhone::<String, R>(Arc::new(Mutex::new((tx, rx2))));

    //start child thread(s)
    let _handle = thread::spawn(move || f(child_phone));

    //serve any request from child threads until all child_phones are dropped or R interrupt
    let mut before = std::time::Instant::now();
    loop {
        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.
        let now = std::time::Instant::now();
        let duration = now - before;
        if duration >= std::time::Duration::from_secs(1) {
            before = now;
            let res_res = extendr_api::eval_string(&"Sys.sleep(0)");
            if res_res.is_err() {
                rprintln!("user interrupt");
                break;
            }
        }

        //look for
        let any_new_msg = rx.try_recv();
        if any_new_msg.is_ok() {
            let msg = any_new_msg.unwrap();
            println!("main: recieved following msg call from a child: {}", msg);
            println!("main: will execute latest recieved a message in R session");
            let robj: Robj = extendr_api::eval_string(&msg).unwrap().clone();
            println!("main: returned from R session");
            //let call_result = robj.as_str().unwrap().to_string();
            let _send_result = tx2.send(R::from(robj));
            println!("main: thread gave R answer to a child");
        } else {
            let recv_err = any_new_msg.unwrap_err();
            //dbg!(&recv_err);
            match recv_err {
                mpsc::TryRecvError::Disconnected => break, //shut down loop
                mpsc::TryRecvError::Empty => {}
            }
        }

        //wait a short time
        thread::sleep(std::time::Duration::from_micros(100));
    }

    "done".to_string()
}

pub fn concurrent_closure<F, S, T, R, Q>(f: F) -> Q
where
    F: FnOnce(ChildPhone<S, R>) -> T + Send + 'static,
    S: FnOnce() -> R + Send + 'static,
    T: Send + 'static,
    R: From<Robj> + Send + 'static,
    Q: From<T>,
{
    //start com channels
    let (tx, rx) = mpsc::channel::<S>();
    let (tx2, rx2) = mpsc::channel::<R>();
    let child_phone = ChildPhone::<S, R>(Arc::new(Mutex::new((tx, rx2))));

    //start child thread(s)
    let handle = thread::spawn(move || f(child_phone));

    //serve any request from child threads until all child_phones are dropped or R interrupt
    let mut before = std::time::Instant::now();
    loop {
        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.
        let now = std::time::Instant::now();
        let duration = now - before;
        if duration >= std::time::Duration::from_secs(1) {
            before = now;
            let res_res = extendr_api::eval_string(&"Sys.sleep(0)");
            if res_res.is_err() {
                rprintln!("user interrupt");
                break;
            }
        }

        //look for
        let any_new_msg = rx.try_recv();
        if any_new_msg.is_ok() {
            println!("main: thread gave R answer to a child");

            if let Ok(s) = any_new_msg {
                let answer: R = s();
                let _send_result = tx2.send(answer).unwrap();
            } else {
                panic!("this should never happen!");
            }
        } else {
            if let Err(recv_err) = any_new_msg {
                match recv_err {
                    mpsc::TryRecvError::Disconnected => break, //shut down loop
                    mpsc::TryRecvError::Empty => {}
                }
            } else {
                panic!("this should not happen!!!");
            }
        }

        //wait a short time
        thread::sleep(std::time::Duration::from_micros(100));
    }

    let thread_return_value = handle.join().unwrap();

    Q::from(thread_return_value)
}

#[extendr]
fn par_c_api_epic() -> i32 {
    //return val is Q
    //outer closure is F
    let return_val = concurrent_closure(|cp| {
        //inner closure is S

        let answer = cp.make_request(|| {
            let robj: Robj = extendr_api::eval_string(&"print('hej hej');1:3")
                .unwrap()
                .clone();
            ParRObj(robj)
        });

        dbg!(answer);

        //thread return is T
        42i32
    });

    dbg!(&return_val);

    return_val
}

#[extendr]
fn par_c_api_fancy() -> String {
    do_concurrent_r::<_, String, ParRObj>(|cp| {
        let answer = cp.make_request(String::from(
            "{print('R is working for thread_1\n'); letters}",
        ));
        let cp_cloned = cp.clone();
        thread::spawn(move || {
            let cp = cp_cloned;
            let answer = cp.make_request("print('hello on behalf of thread_2'); 1:25".to_string());
            dbg!(answer)
        });

        dbg!(answer);

        "child_1 goodbye".to_string()
    })
}

/// @export
#[extendr]
fn par_c_api_calls() -> &'static str {
    // Concurrent and safe call of RCapi.
    // Motto "If you cannot call them, arrange to have them callen"
    //rewrite of https://www.quotery.com/quotes/cant-beat-arrange-beaten

    //channel to send requests to main thread
    let (tx, rx) = mpsc::channel();

    //channel for main thread to answeer requesteeßß
    let (tx2, rx2) = mpsc::channel::<ParRObj>();

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

                    let val = String::from("{print('R is working for thread_2\n'); letters}");
                    child_call.0.send(val).unwrap();
                    println!("child_2: sent request to main");

                    //thread::sleep(std::time::Duration::from_millis(10000));
                    //wait for return
                    let val2 = child_call.1.recv().unwrap();

                    println!("child_2: got this answer from main: {:?}", val2);

                    println!("child_2: will end the call and destroy it's phone");
                });

                //ask main thread to call R-Capi
                let val = String::from(
                    "{print('R is working for thread_1\n'); lm(y~x, data.frame(x=1:10,y=1:10))}",
                );
                child_call.0.send(val).unwrap();
                println!("child_1: has sent request to main");

                //wait for answer
                let val2 = child_call.1.recv().unwrap();
                println!("child_1: recieved a msg but has not peeked into it yet");

                println!("child_1: got this answer from main: {:?}", val2);
            } //release guard
            println!("child_1: hung up the phone / ended the call");

            // ... maybe do other stuff or acquire childphone guard again
        });
        println!("child_1: destroyed it's phone (surpringly printed first, but whatever)");
    } //dropping guard child thread can start sending requests

    //main thread serve incomming requests from child threads

    let mut before = std::time::Instant::now();
    loop {
        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.
        let now = std::time::Instant::now();
        let duration = now - before;
        if duration >= std::time::Duration::from_secs(1) {
            before = now;
            let res_res = extendr_api::eval_string(&"Sys.sleep(0)");
            if res_res.is_err() {
                rprintln!("user interrupt");
                break;
            }
        }

        let any_new_msg = rx.try_recv();
        if any_new_msg.is_ok() {
            let msg = any_new_msg.unwrap();
            println!("main: recieved following msg call from a child: {}", msg);
            println!("main: will execute latest recieved a message in R session");
            let robj: Robj = extendr_api::eval_string(&msg).unwrap().clone();
            println!("main: returned from R session");
            //let call_result = robj.as_str().unwrap().to_string();
            let _send_result = tx2.send(ParRObj(robj));
            println!("main: thread gave R answer to a child");
        } else {
            let recv_err = any_new_msg.unwrap_err();
            //dbg!(&recv_err);
            match recv_err {
                mpsc::TryRecvError::Disconnected => break, //shut down loop
                mpsc::TryRecvError::Empty => {}
            }
        }
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
    fn par_c_api_fancy;
    fn par_c_api_epic;
}
