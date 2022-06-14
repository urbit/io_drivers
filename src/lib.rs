mod http;

use request::tag::*;
use std::marker::Unpin;
use tokio::{
    self,
    io::{self, AsyncReadExt, AsyncWriteExt, ErrorKind},
    sync::mpsc::{self, Receiver, Sender},
};

type Channel = (Sender<Vec<u8>>, Receiver<Vec<u8>>);

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
async fn recv_requests(mut reader: impl AsyncReadExt + Unpin, req_tx: mpsc::Sender<Vec<u8>>) {
    loop {
        // TODO: make explicit that the length is big endian (network byte order)
        let req_len = match reader.read_u64().await {
            Ok(0) => break,
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

/// Write outgoing response to IO responses to some output source.
async fn send_responses(writer: impl AsyncWriteExt, resp_rx: mpsc::Receiver<Vec<u8>>) {}

/// Library entry point.
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    // TODO: decide if there's a better upper bound for number of unscheduled requests.
    const QUEUE_SIZE: usize = 32;

    // input task -> scheduling task
    let (req_tx, mut req_rx): Channel = mpsc::channel(QUEUE_SIZE);
    let input_task = tokio::spawn(recv_requests(io::stdin(), req_tx));

    // driver tasks -> output task
    let (resp_tx, resp_rx): Channel = mpsc::channel(QUEUE_SIZE);
    let output_task = tokio::spawn(send_responses(io::stdout(), resp_rx));

    // scheduling task -> http client driver task
    let (http_client_tx, http_client_rx): Channel = mpsc::channel(QUEUE_SIZE);
    let http_client_task = tokio::spawn(http::client::run(http_client_rx, resp_tx));

    while let Some(req) = req_rx.recv().await {
        match req[0] {
            HTTP_CLIENT => {}
            _ => todo!("unknown request type"),
        }
    }

    input_task.await.unwrap();
    http_client_task.await.unwrap();
    output_task.await.unwrap();
    println!("main: exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::BufReader, runtime};

    #[test]
    fn recv_requests() {
        runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                const REQ: [u8; 13] = [
                    // Length of payload. Big endian.
                    0, 0, 0, 0, 0, 0, 0, 5, // Payload.
                    b'h', b'e', b'l', b'l', b'o',
                ];
                let reader = BufReader::new(&REQ[..]);
                let (req_tx, mut req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(8);

                tokio::spawn(super::recv_requests(reader, req_tx));

                let req = req_rx.recv().await.unwrap();
                let req = String::from_utf8(req).unwrap();
                assert_eq!(req, "hello");
            });
    }

    #[test]
    fn schedule_requests() {
        runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {});
    }
}
