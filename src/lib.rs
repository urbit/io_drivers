use tokio::{
    runtime,
    sync::mpsc::{self, Receiver, Sender},
};

/// Last line of a Message.
const MESSAGE_DELIM: &'static str = "==\n";

/// Either an IO request or an IO response.
type Message = Vec<String>;

/// Read incoming IO requests from stdin.
fn read_stdin(req_tx: Sender<Message>) {
    let stdin = std::io::stdin();
    let mut should_exit = false;
    loop {
        let mut req = Vec::new();
        loop {
            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Err(_) | Ok(0) => {
                    should_exit = true;
                    break;
                }
                Ok(_) => {
                    if line == MESSAGE_DELIM {
                        break;
                    } else {
                        req.push(line);
                    }
                }
            }
        }
        if should_exit {
            break;
        }
        req_tx.blocking_send(req).unwrap();
    }
    println!("stdin: exiting");
}

/// Schedule IO requests received from the stdin thread.
async fn schedule_requests(mut req_rx: Receiver<Message>) {
    while let Some(req) = req_rx.recv().await {
        for line in req {
            print!("io: {}", line);
        }
    }
}

/// Library entry point.
pub fn run() {
    // TODO: decide if there's a better upper bound for number of unscheduled requests.
    let (req_tx, req_rx): (Sender<Message>, Receiver<Message>) = mpsc::channel(1024);

    // Read requests in a dedicated thread because tokio doesn't seem to implement async reads from
    // stdin.
    let stdin_thr = std::thread::spawn(move || {
        read_stdin(req_tx);
    });

    runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap()
        .block_on(schedule_requests(req_rx));
    // TODO: decide Runtime::shutdown_timeout() should be used.

    stdin_thr.join().unwrap();
    println!("main: exiting");
}
