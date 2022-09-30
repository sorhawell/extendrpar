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

    //send request to main thread
    pub fn send(&self, s: S) {
        self.mains_tx.send((s, self.child_tx.clone())).expect("failed to send to main thread, likely do to user interrupt, shutting down with a good old panic!!!");
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

impl<S, R> Drop for ThreadCom<S, R> {
    fn drop(&mut self) {
        println!("Dropping a threadcom somewhere");
    }
}

pub fn concurrent_handler<F, I, R, S, T>(
    f: F,
    i: I,
    conf: &Storage<RwLock<Option<ThreadCom<S, R>>>>,
) -> T
where
    F: FnOnce(ThreadCom<S, R>) -> T + Send + 'static,
    I: Fn(S) -> R + Send + 'static,
    R: Send + 'static,
    S: Send + 'static + std::fmt::Display,
    T: Send + 'static + Default,
{
    //start com unboundeds

    let (thread_com, main_rx) = ThreadCom::create();

    //set or update global thread_com
    let conf_status = conf.set(RwLock::new(Some(thread_com.clone())));
    dbg!(conf_status);
    if !conf_status {
        let mut gtc = conf
            .get()
            .write()
            .expect("failed to modify GLOBAL therad_com");
        *gtc = Some(thread_com.clone());
    }

    //start child thread(s)
    let handle = thread::spawn(move || f(thread_com));

    //serve any request from child threads until all child_phones are dropped or R interrupt
    let mut before = std::time::Instant::now();
    let start_time = std::time::Instant::now();
    let mut loop_counter = 0u64;
    loop {
        loop_counter += 1;
        let now = std::time::Instant::now();
        let duration = now - before;
        before = std::time::Instant::now();
        dbg!(duration, loop_counter);

        let any_new_msg = main_rx.recv_timeout(std::time::Duration::from_millis(100));

        //avoid using unwrap/unwrap_err if msg is Debug
        if let Ok(packet) = any_new_msg {
            let (s, c_tx) = packet;
            let answer = i(s); //handle requst with i closure
            let _send_result = c_tx.send(answer).unwrap();
        } else {
            if let Err(recv_err) = any_new_msg {
                dbg!(recv_err);
                match recv_err {
                    //no connections left, shut down loop, does not happen after one global tx always exists
                    //in theory a thread or main could destroy global thread_com to terminate this way
                    flume::RecvTimeoutError::Disconnected => {
                        dbg!(&recv_err);
                        break;
                    }
                    //idling, sleep double as long as last time
                    flume::RecvTimeoutError::Timeout => {
                        //check for R user interrupt with Sys.sleep(0) every 1 second if not serving a thread.

                        let res_res = extendr_api::eval_string(&"print('check user');Sys.sleep(0)");
                        if res_res.is_err() {
                            rprintln!("R user signalled interrupt");
                            return (T::default());
                        }

                        //TODO sub-optimal way to finish, better if secondlast threadcom destroys the global threadcom
                        //thereby notifying to main thread time to disconnect, could save 50-100ms timeout wait
                        if handle.is_finished() {
                            dbg!(&handle);
                            break;
                        }
                    } //check if spawned thread has ended, then stop. (most normal end)
                }
            } else {
                unreachable!("a result was neither error or ok");
            }
        }
    }

    let thread_return_value = handle.join().unwrap();

    let run_time = std::time::Instant::now() - start_time;
    dbg!(run_time);

    thread_return_value
}
