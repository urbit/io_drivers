mod http;

use std::marker::Unpin;
use tokio::{
    self,
    io::{self, AsyncReadExt, AsyncWriteExt, ErrorKind},
    sync::mpsc::{self, Receiver, Sender},
};

type Channel<T> = (Sender<T>, Receiver<T>);

/// Endianness of requests.
type Endianness = bitstream_io::LittleEndian;

type RequestTag = u8;

/// Tag identifying the type of a serialized IO request.
const HTTP_CLIENT: RequestTag = 0;

/// Read incoming IO requests from some input source.
async fn recv_io_requests(mut reader: impl AsyncReadExt + Unpin, req_tx: Sender<Vec<u8>>) {
    loop {
        let req_len = match reader.read_u64_le().await {
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
                // TODO: better error handling.
                req_tx.send(req).await.unwrap();
            }
            Err(_) => todo!("handle error"),
        }
    }
}

/// Schedule IO requests with the appropriate driver.
async fn schedule_io_requests(mut req_rx: Receiver<Vec<u8>>, http_client_tx: Sender<Vec<u8>>) {
    while let Some(req) = req_rx.recv().await {
        match req[0] {
            HTTP_CLIENT => {
                // TODO: better error handling.
                http_client_tx.send(req).await.unwrap();
            }
            _ => {
                println!("req={}", String::from_utf8(req).unwrap());
                //todo!("unknown request type");
            }
        }
    }
}

/// Read outgoing IO responses from the drivers and write the responses to some output source.
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

/// Library entry point.
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    // TODO: decide if there's a better upper bound for number of unscheduled requests.
    const QUEUE_SIZE: usize = 32;

    // driver tasks -> output task
    let (resp_tx, resp_rx): Channel<Vec<u8>> = mpsc::channel(QUEUE_SIZE);
    let output_task = tokio::spawn(send_io_responses(io::stdout(), resp_rx));

    // scheduling task -> http client driver task
    let (http_client_tx, http_client_rx): Channel<Vec<u8>> = mpsc::channel(QUEUE_SIZE);
    let http_client_task = tokio::spawn(http::client::run(http_client_rx, resp_tx));

    // input task -> scheduling task
    let (req_tx, req_rx): Channel<Vec<u8>> = mpsc::channel(QUEUE_SIZE);
    let scheduling_task = tokio::spawn(schedule_io_requests(req_rx, http_client_tx));
    let input_task = tokio::spawn(recv_io_requests(io::stdin(), req_tx));

    input_task.await.unwrap();
    scheduling_task.await.unwrap();
    http_client_task.await.unwrap();
    output_task.await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    macro_rules! async_test {
        ($async_block:block) => {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async { $async_block });
        };
    }

    #[test]
    fn recv_io_requests() {
        async_test!({
            const REQ: [u8; 13] = [
                5, 0, 0, 0, 0, 0, 0, 0, // Payload.
                b'h', b'e', b'l', b'l', b'o',
            ];
            let reader = BufReader::new(&REQ[..]);
            let (req_tx, mut req_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(8);

            tokio::spawn(super::recv_io_requests(reader, req_tx));

            let req = req_rx.recv().await.unwrap();
            let req = String::from_utf8(req).unwrap();
            assert_eq!(req, "hello");
        });
    }

    #[test]
    fn send_io_responses() {
    }
}
