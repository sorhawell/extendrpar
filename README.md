Safe concurrent RCapi calls with extendr
================
Søren Welling
8/30/2022

## Concurrent calls to the CRapi.

Calling R via extendr\_api from rust threads will very likely lead to a
crash. Some times multi-threadding is neccsary e.g. when porting
libraries which require that. E.g. polars lambda functions. Sometimes it
might just be nice to have. `pyo3` the corrosponding api for python has
the feature `acquire_gil_lock()` allowing threads in turn to call the
python interpreter. Here I show something similar is possible with
extendr\_api, when using two mpsc-channels and a shared mutex. - Main
thread is called `main:` Child threads are called `child_1` and
`child_2`. - Spawned Children recieve a mutex locked reference to a
“phone” where they can call the main thread and ask to do stuff in R for
them. - Only one child can call with main thread at the time. Other
cildren will just wait for their turn. - Main thread will return answers
from R session to the child it is in a call with. Hence the correct
child will recieve the answer and then hang up or make a new request. -
Main thread will terminate when all phones are destroyed.

``` r
#try it out
par_c_api_calls()
```

    ## [1] "R is working for thread_1\n"
    ## [1] "R is working for thread_2\n"

    ## [1] "threadding complete"

Unfortunately threaded rprintln seems not allowed when called from
Knitr-rmarkdown. So here is the full print of what just happend.

    child_1: destroyed it's phone (surpringly printed first, but whatever)
    child_1: try make call with child_phone
    child_1: has started a call
    child_1: has sent request to main
    main: recieved following msg call from a child: {print('R is working for thread_1
    '); as.character(2+2)}
    main: will execute latest recieved a message in R session
    child_2: also tries make call with child_phone but will wait

    [1] "R is working for thread_1\n"
    main: returned from R session
    main: thread gave R answer to a child
    child_1: got this answer from main: 4
    child_1: hung up the phone / ended the call
    child_2: finally has started a call
    child_2: sent request to main
    main: recieved following msg call from a child: {print('R is working for thread_2
    '); as.character(2+3)}
    main: will execute latest recieved a message in R session
    [1] "R is working for thread_2\n"
    main: returned from R session
    main: thread gave R answer to a child
    child_2: got this answer from main: 5
    child_2: will end the call and destroy it's phone
    main: thread noticed all children destroyed their phones, time to return
    [1] "threadding complete"

Here is the rust code…

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
