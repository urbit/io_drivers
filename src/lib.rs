mod http;

use crate::http::client::HttpClient;
use noun::{
    atom::Atom,
    serdes::{Cue, Jam},
    Noun,
};
use std::marker::Unpin;
use tokio::{
    self,
    io::{self, AsyncReadExt, AsyncWriteExt, ErrorKind},
    runtime::{self, Runtime},
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

type Channel<T> = (Sender<T>, Receiver<T>);

type RequestTag = u8;

/// Tag identifying the type of a serialized IO request.
const HTTP_CLIENT: RequestTag = 0;

/// A generic IO driver.
trait Driver: Sized {
    /// Spawns a task to asynchronously handle IO requests.
    ///
    /// This is the driver entry point.
    ///
    /// Handles requests as long as the input channel is open and sends the responses to the output
    /// channel.
    fn run(req_rx: Receiver<Noun>, resp_tx: Sender<Vec<u8>>) -> JoinHandle<()>;
}

/// Reads incoming IO requests from an input source.
async fn recv_io_requests(mut reader: impl AsyncReadExt + Unpin, http_client_tx: Sender<Noun>) {
    loop {
        let req_len = match reader.read_u64_le().await {
            Ok(0) => break,
            Ok(req_len) => usize::try_from(req_len).expect("u64 to usize"),
            Err(err) => match err.kind() {
                ErrorKind::UnexpectedEof => break,
                _ => todo!(),
            },
        };
        let req_tag = match reader.read_u8().await {
            Ok(req_tag) => req_tag,
            Err(_err) => todo!("handle error"),
        };
        let mut req = Vec::with_capacity(req_len);
        req.resize(req.capacity(), 0);
        match reader.read_exact(&mut req).await {
            Ok(_) => match req_tag {
                HTTP_CLIENT => {
                    // TODO: better error handling.
                    let req = Noun::cue(Atom::from(req)).unwrap();
                    http_client_tx.send(req).await.unwrap();
                }
                _ => todo!(),
            },
            Err(_) => todo!("handle error"),
        }
    }
}

/// Reads outgoing IO responses from the drivers and writes the responses to an output source.
async fn send_io_responses(mut writer: impl AsyncWriteExt + Unpin, mut resp_rx: Receiver<Vec<u8>>) {
    while let Some(mut resp) = resp_rx.recv().await {
        let len = u64::try_from(resp.len()).unwrap();
        if let Err(_) = writer.write_u64_le(len).await {
            todo!("handle error");
        }
        if let Err(_) = writer.write_all(&mut resp).await {
            todo!("handle error");
        }
        if let Err(_) = writer.flush().await {
            todo!("handle error");
        }
    }
}

/// Constructs a [tokio] runtime.
///
/// [tokio]: https://docs.rs/tokio/latest/tokio/index.html
fn runtime() -> Runtime {
    {
        #[cfg(feature = "multi-thread")]
        {
            runtime::Builder::new_multi_thread()
        }
        #[cfg(not(feature = "multi-thread"))]
        {
            runtime::Builder::new_current_thread()
        }
    }
    .enable_all()
    .build()
    .unwrap()
}

/// Asynchronously handles IO requests.
///
/// This is the library entry point.
///
/// Reads incoming IO requests from `stdin`, which are of the following form:
/// ```text
/// jammed request length (8  bytes, little endian)
/// request type tag      (1  byte)
/// jammed request        (>1 bytes)
/// ```
///
/// The jammed request is dispatched to the appropriate driver based off the request type tag in
/// the IO request. Once the driver handles the request, it writes the response to `stdout`.
///
/// The following drivers are currently supported:
/// - HTTP client.
pub fn run() {
    runtime().block_on(async {
        // TODO: decide if there's a better upper bound for number of unscheduled requests.
        const QUEUE_SIZE: usize = 32;

        // driver tasks -> output task
        let (resp_tx, resp_rx): Channel<Vec<u8>> = mpsc::channel(QUEUE_SIZE);
        let output_task = tokio::spawn(send_io_responses(io::stdout(), resp_rx));

        // scheduling task -> http client driver task
        let (http_client_tx, http_client_rx): Channel<Noun> = mpsc::channel(QUEUE_SIZE);
        let http_client_task = HttpClient::run(http_client_rx, resp_tx);

        // input task -> scheduling task
        let input_task = tokio::spawn(recv_io_requests(io::stdin(), http_client_tx));

        input_task.await.unwrap();
        http_client_task.await.unwrap();
        output_task.await.unwrap();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    macro_rules! async_test {
        ($async_block:block) => {
            runtime().block_on(async { $async_block });
        };
    }

    #[test]
    fn recv_io_requests() {
        async_test!({
            const REQ: [u8; 16] = [
                7, 0, 0, 0, 0, 0, 0, 0, // Length.
                0, // Tag.
                128, 7, 173, 140, 141, 237, 13, // (%jam hello)
            ];

            let reader = BufReader::new(&REQ[..]);
            let (req_tx, mut req_rx): Channel<Noun> = mpsc::channel(8);

            tokio::spawn(super::recv_io_requests(reader, req_tx));

            if let Noun::Atom(req) = req_rx.recv().await.expect("recv") {
                assert_eq!(req, "hello");
            } else {
                panic!("unexpected cell");
            }
        });
    }

    #[test]
    fn send_io_responses() {}
}
