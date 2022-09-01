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

#[derive(Debug)]
pub struct ThreadCom<S, R> {
    mains_tx: mpsc::Sender<(S, mpsc::Sender<R>)>,
    child_rx: mpsc::Receiver<R>,
    child_tx: mpsc::Sender<R>,
}

impl<S, R> ThreadCom<S, R> {
    //return tupple with ThreadCom for child thread and m_rx to stay in main_thread
    pub fn create() -> (Self, mpsc::Receiver<(S, mpsc::Sender<R>)>) {
        let (m_tx, m_rx) = mpsc::channel::<(S, mpsc::Sender<R>)>();
        let (c_tx, c_rx) = mpsc::channel::<R>();

        (
            ThreadCom {
                mains_tx: m_tx,
                child_tx: c_tx,
                child_rx: c_rx,
            },
            m_rx,
        )
    }

    //send msg to main thread
    pub fn send(&self, s: S) {
        self.mains_tx.send((s, self.child_tx.clone())).unwrap()
    }

    //recieve msg from main thread
    pub fn recv(&self) -> R {
        self.child_rx.recv().unwrap()
    }

    //clone only main channel, create new unique child channel
    pub fn clone(&self) -> Self {
        let (tx, rx) = mpsc::channel::<R>();
        ThreadCom {
            mains_tx: self.mains_tx.clone(),
            child_rx: rx,
            child_tx: tx,
        }
    }
}

pub fn concurrent_handler<F, G, R, S, T>(f: F, g: G) -> T
where
    F: FnOnce(ThreadCom<S, R>) -> T + Send + 'static,
    G: Fn(S) -> R + Send + 'static,
    R: Send + 'static,
    S: Send + 'static + std::fmt::Display,
    T: Send + 'static,
{
    //start com channels

    let (thread_com, main_rx) = ThreadCom::create();

    //start child thread(s)
    let handle = thread::spawn(move || f(thread_com));

    //serve any request from child threads until all child_phones are dropped or R interrupt
    let mut before = std::time::Instant::now();
    let mut planned_sleep = std::time::Duration::from_micros(2);
    loop {
        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.
        let now = std::time::Instant::now();
        let duration = now - before;
        if duration >= std::time::Duration::from_secs(1) {
            before = now;
            let res_res = extendr_api::eval_string(&"print('check user');Sys.sleep(0)");
            if res_res.is_err() {
                rprintln!("user interrupt");
                break;
            }
        }

        //look for
        let any_new_msg = main_rx.try_recv();

        //avoid using unwrap/unwrap_err if msg is Debug
        if let Ok(packet) = any_new_msg {
            let (s, c_tx) = packet;
            let answer = g(s); //handle requst with g closure
            let _send_result = c_tx.send(answer).unwrap();

            //stuff to do sleep less ...
            planned_sleep =
                std::time::Duration::max(planned_sleep / 2, std::time::Duration::from_nanos(100));
            thread::sleep(planned_sleep);
        } else {
            if let Err(recv_err) = any_new_msg {
                dbg!(recv_err);
                match recv_err {
                    //no connections left, shut down loop
                    mpsc::TryRecvError::Disconnected => break,
                    //idling, sleep double as long as last time
                    mpsc::TryRecvError::Empty => {
                        planned_sleep = std::time::Duration::min(
                            planned_sleep * 4 / 3,
                            std::time::Duration::from_millis(5),
                        );
                        thread::sleep(planned_sleep);
                        dbg!(planned_sleep);
                    }
                }
            } else {
                unreachable!("something result is neither error or ok");
            }
        }
    }

    let thread_return_value = handle.join().unwrap();

    thread_return_value
}

#[extendr]
fn par_con_handler() -> i32 {
    //chose outer return type

    let return_val: i32 = concurrent_handler(
        |thread_com| {
            //inner closure is S

            for i in 0..5 {
                thread::sleep(std::time::Duration::from_millis(3));

                let tc_clone = thread_com.clone();
                let _h = thread::spawn(move || {
                    let thread_com = tc_clone;
                    for j in 0..5 {
                        let tc_clone = thread_com.clone();
                        let _h = thread::spawn(move || {
                            let thread_com = tc_clone;

                            thread_com
                                .send(format!("print('hej from thread_{}');1:4", 1 + j + i * 5));
                            let _answer: ParRObj = thread_com.recv();
                            {
                                let _drop_me = thread_com;
                            }
                            thread::sleep(std::time::Duration::from_millis(5000));
                        });
                    }
                });
            }
            thread_com.send("print('hej hej');1:3".to_string());
            let _answer: ParRObj = thread_com.recv();

            //thread return is T
            42i32
        },
        |s| {
            let robj: Robj = extendr_api::eval_string(&(s.to_string())).unwrap();
            ParRObj(robj)
        },
    );

    return_val
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
    fn par_con_handler;
}
