use std::{
    io::{self, ErrorKind, Read},
    mem::size_of,
};
use tokio::{
    self, runtime,
    sync::mpsc::{self, Receiver, Sender},
};

/// IO request.
mod request {
    /// Tag identifying the type of a serialized IO request.
    pub mod tag {
        pub const HTTP_CLIENT: u8 = 0;
    }

    /// Marker trait to identify the implementing type as an IO request.
    pub trait Request: TryFrom<Vec<u8>> {}
}

/// Synchronously read incoming IO requests from some input source.
fn read_requests(mut reader: impl Read, req_tx: Sender<Vec<u8>>) {
    loop {
        let req_len = {
            const SIZE: usize = size_of::<u64>();
            let mut len_buf = [0; SIZE];
            if let Err(err) = reader.read_exact(&mut len_buf) {
                match err.kind() {
                    ErrorKind::UnexpectedEof => break,
                    _ => todo!("handle error"),
                }
            }
            // TODO: make explicit that the length is big endian (network byte order)
            u64::from_be_bytes(len_buf)
        };
        let mut req = Vec::with_capacity(req_len as usize);
        match reader.read_to_end(&mut req) {
            Ok(0) => break,
            Ok(_) => {
                req_tx.blocking_send(req).unwrap();
            }
            Err(_) => todo!("handle error"),
        }
    }
    println!("stdin: exiting");
}

/// Schedule IO requests received from the stdin thread.
async fn schedule_requests(mut req_rx: Receiver<Vec<u8>>) {
    while let Some(req) = req_rx.recv().await {
        println!("io: req={}", String::from_utf8(req).unwrap());
    }
}

/// Library entry point.
pub fn run() {
    // TODO: decide if there's a better upper bound for number of unscheduled requests.
    let (req_tx, req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(1024);

    // Read requests in a dedicated thread because tokio doesn't seem to implement async reads from
    // stdin.
    let input_thr = std::thread::spawn(move || {
        read_requests(io::stdin().lock(), req_tx);
    });

    runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap()
        .block_on(schedule_requests(req_rx));
    // TODO: decide if Runtime::shutdown_timeout() should be used.

    input_thr.join().unwrap();
    println!("main: exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn send_request_to_stdin() {
        let (req_tx, mut req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(8);

        const REQ: [u8; 13] = [
            // Length of payload. Big endian.
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            5,
            // Payload.
            b'h',
            b'e',
            b'l',
            b'l',
            b'o',
        ];
        let reader = BufReader::new(&REQ[..]);
        let input_thr = std::thread::spawn(move || {
            read_requests(reader, req_tx);
        });

        let req = req_rx.blocking_recv().unwrap();
        let req = String::from_utf8(req).unwrap();

        assert_eq!(req, "hello");

        input_thr.join().unwrap();
    }
}
