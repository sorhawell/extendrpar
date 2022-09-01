use extendr_api::prelude::*;

use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;

//
#[derive(Clone, Debug)]
pub struct ParRObj(pub Robj);
unsafe impl Send for ParRObj {}
unsafe impl Sync for ParRObj {}

impl From<Robj> for ParRObj {
    fn from(robj: Robj) -> Self {
        ParRObj(robj)
    }
}

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

pub fn concurrent_closure<F, S, T, R, Q>(f: F) -> Q
where
    F: FnOnce(ChildPhone<S, R>) -> T + Send + 'static,
    S: FnOnce() -> R + Send + 'static,
    T: Send + 'static,
    R: Send + 'static,
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

        //avoid using unwrap/unwrap_err if msg is Debug
        if let Ok(s) = any_new_msg {
            let answer: R = s();
            let _send_result = tx2.send(answer).unwrap();
        } else {
            if let Err(recv_err) = any_new_msg {
                match recv_err {
                    mpsc::TryRecvError::Disconnected => break, //shut down loop
                    mpsc::TryRecvError::Empty => {}
                }
            } else {
                unreachable!("something result is neither error or ok");
            }
        }

        //wait a short time
        thread::sleep(std::time::Duration::from_micros(10));
    }

    let thread_return_value = handle.join().unwrap();

    Q::from(thread_return_value)
}

#[extendr]
fn par_c_api_epic() -> i32 {
    //chose outer return type
    let return_val: i32 = concurrent_closure(|cp| {
        //inner closure is S

        let answer = cp.make_request(|| {
            let robj: Robj = extendr_api::eval_string(&"print('hej hej');1:3")
                .unwrap()
                .clone();
            ParRObj(robj) //choose inner return type
        });

        dbg!(answer);

        //thread return is T
        42i32
    });

    dbg!(&return_val);

    return_val
}

// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod extendrpar;
    fn par_c_api_epic;
}
