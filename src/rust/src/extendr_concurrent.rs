use extendr_api::prelude::*;

//use std::sync::mpsc::{Receiver, Sender};
use std::sync::RwLock;
use std::thread;

use flume;
use flume::{Receiver, Sender};

use state::Storage;

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
    mains_tx: Sender<(S, Sender<R>)>,
    child_rx: Receiver<R>,
    child_tx: Sender<R>,
}

impl<S, R> ThreadCom<S, R> {
    //return tupple with ThreadCom for child thread and m_rx to stay in main_thread
    pub fn create() -> (Self, Receiver<(S, Sender<R>)>) {
        let (m_tx, m_rx) = flume::unbounded::<(S, Sender<R>)>();
        let (c_tx, c_rx) = flume::unbounded::<R>();

        (
            ThreadCom {
                mains_tx: m_tx,
                child_tx: c_tx,
                child_rx: c_rx,
            },
            m_rx,
        )
    }

    // pub fn from_main_sender(
    //     mc: (Sender<(S, Sender<R>)>, Receiver<(S, Sender<R>)>),
    // ) -> (Self, flume::Receiver<(S, flume::Sender<R>)>) {
    //     let (m_tx, m_rx) = (mc.0, mc.1);
    //     let (c_tx, c_rx) = flume::unbounded::<R>();

    //     (
    //         ThreadCom {
    //             mains_tx: m_tx,
    //             child_tx: c_tx,
    //             child_rx: c_rx,
    //         },
    //         m_rx,
    //     )
    // }

    //send request to main thread
    pub fn send(&self, s: S) {
        self.mains_tx.send((s, self.child_tx.clone())).unwrap()
    }

    //wait to recieve answer from main thread
    pub fn recv(&self) -> R {
        self.child_rx.recv().unwrap()
    }

    //clone only main unbounded, create new unique child unbounded (such that each thread has unique comminucation when main)
    pub fn clone(&self) -> Self {
        let (tx, rx) = flume::unbounded::<R>();
        ThreadCom {
            mains_tx: self.mains_tx.clone(),
            child_rx: rx,
            child_tx: tx,
        }
    }
}

pub fn concurrent_handler<F, I, R, S, T>(f: F, i: I, conf: &Storage<RwLock<ThreadCom<S, R>>>) -> T
where
    F: FnOnce(ThreadCom<S, R>) -> T + Send + 'static,
    I: Fn(S) -> R + Send + 'static,
    R: Send + 'static,
    S: Send + 'static + std::fmt::Display,
    T: Send + 'static,
{
    //start cßom unboundeds

    let (thread_com, main_rx) = ThreadCom::create();

    //set or update global thread_com
    let conf_status = conf.set(RwLock::new(thread_com.clone()));
    dbg!(conf_status);
    if !conf_status {
        let mut gtc = conf
            .get()
            .write()
            .expect("failed to modify GLOBAL therad_com");
        *gtc = thread_com.clone();
    }

    //start child thread(s)
    let handle = thread::spawn(move || f(thread_com));

    //serve any request from child threads until all child_phones are dropped or R interrupt
    let mut before = std::time::Instant::now();
    let mut planned_sleep = std::time::Duration::from_micros(2);
    let mut loop_counter = 0u64;
    loop {
        loop_counter += 1;
        let now = std::time::Instant::now();
        let duration = now - before;
        before = std::time::Instant::now();
        dbg!(duration, loop_counter);
        //look for
        let any_new_msg = main_rx.try_recv();

        //avoid using unwrap/unwrap_err if msg is Debug
        if let Ok(packet) = any_new_msg {
            let (s, c_tx) = packet;
            let answer = i(s); //handle requst with g closure
            let _send_result = c_tx.send(answer).unwrap();

            //stuff to do!! sleep less if ever idle
            planned_sleep =
                std::time::Duration::max(planned_sleep / 4, std::time::Duration::from_nanos(100));
        } else {
            if let Err(recv_err) = any_new_msg {
                dbg!(recv_err);
                match recv_err {
                    //no connections left, shut down loop, does not happen after one global tx always exists
                    //in theory a thread or main could destroy global thread_com to terminate this way
                    flume::TryRecvError::Disconnected => {
                        dbg!(&recv_err);
                        break;
                    }
                    //idling, sleep double as long as last time
                    flume::TryRecvError::Empty => {
                        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.

                        if duration >= std::time::Duration::from_secs(1) {
                            before = now;
                            let res_res =
                                extendr_api::eval_string(&"print('check user');Sys.sleep(0)");
                            if res_res.is_err() {
                                rprintln!("user interrupt");
                                break;
                            }
                        }

                        //check if spawned thread has ended, then stop. (most normal end)
                        if handle.is_finished() {
                            dbg!(&handle);
                            break;
                        }

                        //sleep thead takes 50-100 micros also
                        if planned_sleep > std::time::Duration::from_micros(60) {
                            dbg!(planned_sleep);
                            thread::sleep(planned_sleep);
                        }

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
