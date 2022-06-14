use std::marker::Unpin;
use tokio::{
    self,
    io::{self, AsyncReadExt, ErrorKind},
    runtime,
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

/// Read incoming IO requests from some input source.
async fn recv_requests(mut reader: impl AsyncReadExt + Unpin, req_tx: Sender<Vec<u8>>) {
    loop {
        // TODO: make explicit that the length is big endian (network byte order)
        let req_len = match reader.read_u64().await {
            Ok(req_len) => usize::try_from(req_len).expect("usize is smaller than u64"),
            Err(err) => match err.kind() {
                ErrorKind::UnexpectedEof => break,
                _ => todo!(),
            },
        };
        let mut req = Vec::with_capacity(req_len);
        req.resize(req_len, 0);
        match reader.read_exact(&mut req).await {
            Ok(_) => {
                req_tx.send(req).await.unwrap();
            }
            Err(_) => todo!("handle error"),
        }
    }
    println!("stdin: exiting");
}

/// Library entry point.
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    // TODO: decide if there's a better upper bound for number of unscheduled requests.
    let (req_tx, mut req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(1024);

    tokio::spawn(recv_requests(io::stdin(), req_tx));

    while let Some(req) = req_rx.recv().await {
        println!("io: req={}", String::from_utf8(req).unwrap());
    }
    println!("main: exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[test]
    fn send_request_to_stdin() {
        runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                const REQ: [u8; 13] = [
                    // Length of payload. Big endian.
                    0, 0, 0, 0, 0, 0, 0, 5,
                    // Payload.
                    b'h', b'e', b'l', b'l', b'o',
                ];
                let reader = BufReader::new(&REQ[..]);
                let (req_tx, mut req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(8);

                tokio::spawn(recv_requests(reader, req_tx));

                let req = req_rx.recv().await.unwrap();
                let req = String::from_utf8(req).unwrap();
                assert_eq!(req, "hello");
            });
    }
}
