use extendr_api::prelude::*;

use std::sync::mpsc;
use std::thread;

//shamelessly make Robj send + sync
//no crashes so far the 'data'-SEXPS as Vectors, lists, pairlists
//CLOSEXP crashes every time
#[derive(Clone, Debug)]
pub struct ParRObj(pub Robj);
unsafe impl Send for ParRObj {}
unsafe impl Sync for ParRObj {}

impl From<Robj> for ParRObj {
    fn from(robj: Robj) -> Self {
        ParRObj(robj)
    }
}

//ThreadCom allow any sub thread to request main thread to e.g. run R code
//main thread handles requests sequentially and return answer to the specific requestee thread
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

    //send request to main thread
    pub fn send(&self, s: S) {
        self.mains_tx.send((s, self.child_tx.clone())).unwrap()
    }

    //wait to recieve answer from main thread
    pub fn recv(&self) -> R {
        self.child_rx.recv().unwrap()
    }

    //clone only main channel, create new unique child channel (such that each thread has unique comminucation when main)
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
        //look for
        let any_new_msg = main_rx.try_recv();

        //avoid using unwrap/unwrap_err if msg is Debug
        if let Ok(packet) = any_new_msg {
            let (s, c_tx) = packet;
            let answer = g(s); //handle requst with g closure
            let _send_result = c_tx.send(answer).unwrap();

            //stuff to do!! sleep less if ever idle
            planned_sleep =
                std::time::Duration::max(planned_sleep / 2, std::time::Duration::from_nanos(100));
        } else {
            if let Err(recv_err) = any_new_msg {
                dbg!(recv_err);
                match recv_err {
                    //no connections left, shut down loop
                    mpsc::TryRecvError::Disconnected => break,
                    //idling, sleep double as long as last time
                    mpsc::TryRecvError::Empty => {
                        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.
                        let now = std::time::Instant::now();
                        let duration = now - before;
                        if duration >= std::time::Duration::from_secs(1) {
                            before = now;
                            let res_res =
                                extendr_api::eval_string(&"print('check user');Sys.sleep(0)");
                            if res_res.is_err() {
                                rprintln!("user interrupt");
                                break;
                            }
                        }

                        dbg!(planned_sleep);
                        thread::sleep(planned_sleep);
                        planned_sleep = std::time::Duration::min(
                            planned_sleep * 4 / 3,
                            std::time::Duration::from_millis(5),
                        );
                    }
                }
            } else {
                unreachable!("a result was neither error or ok");
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
                            // {
                            //     let _drop_me = thread_com;
                            // }
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

// Macro to generate exports.
// This ensures exported functions are registered with R.
// See corresponding C code in `entrypoint.c`.
extendr_module! {
    mod extendrpar;
    fn par_con_handler;
}
