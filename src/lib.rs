mod http;

use crate::http::client::HttpClient;
use log::{debug, error, info, warn};
use noun::{
    atom::Atom,
    serdes::{Cue, Jam},
    Noun,
};
use std::{
    fmt::{Display, Error, Formatter},
    marker::{Send, Unpin},
    process::{ExitCode, Termination},
};
use tokio::{
    self,
    io::{self, AsyncReadExt, AsyncWriteExt, ErrorKind},
    runtime::{self, Runtime},
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

type Channel<T> = (Sender<T>, Receiver<T>);

/// The return status of the crate.
#[derive(Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Success = 0,
    /// Reading IO requests from stdin failed.
    ReadFailed = 1,
    /// Writing IO responses to stdout failed.
    WriteFailed = 2,
    /// The HTTP client driver failed.
    HttpClientFailed = 3,
}

impl Status {
    fn success(&self) -> bool {
        *self == Status::Success
    }
}

impl Termination for Status {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

/// Tag identifying a driver.
#[derive(Eq, PartialEq)]
#[repr(u8)]
pub enum DriverTag {
    HttpClient = 0,
}

impl Display for DriverTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Self::HttpClient => write!(f, "HTTP client"),
        }
    }
}

impl TryFrom<u8> for DriverTag {
    type Error = ();

    fn try_from(val: u8) -> Result<Self, ()> {
        match val {
            0 => Ok(Self::HttpClient),
            _ => Err(())
        }
    }
}

/// A generic IO driver.
trait Driver: Sized {
    /// Initializes a new driver.
    fn new() -> Self;

    /// Spawns a task to asynchronously handle IO requests.
    ///
    /// This is the driver entry point.
    ///
    /// Handles requests as long as the input channel is open and sends the responses to the output
    /// channel.
    fn run(self, req_rx: Receiver<Noun>, resp_tx: Sender<Noun>) -> JoinHandle<Status>;
}

/// Reads incoming IO requests from an input source.
fn recv_io_requests(
    mut reader: impl AsyncReadExt + Send + Unpin + 'static,
    http_client_tx: Sender<Noun>,
) -> JoinHandle<Status> {
    let task = tokio::spawn(async move {
        loop {
            let req_len = match reader.read_u64_le().await {
                Ok(0) => {
                    info!(target: "io-drivers:input", "encountered EOF");
                    return Status::Success;
                }
                Ok(req_len) => {
                    if let Ok(req_len) = usize::try_from(req_len) {
                        req_len
                    } else {
                        error!(target: "io-drivers:input", "request length {} does not fit in usize", req_len);
                        return Status::ReadFailed;
                    }
                }
                Err(err) => match err.kind() {
                    ErrorKind::UnexpectedEof => {
                        info!(target: "io-drivers:input", "encountered EOF");
                        return Status::Success;
                    }
                    err => {
                        error!(target: "io-drivers:input", "failed to read request length: {}", err);
                        return Status::ReadFailed;
                    }
                },
            };
            debug!(target: "io-drivers:input", "request length = {}", req_len);

            let driver_tag = match reader.read_u8().await {
                Ok(driver_tag) => driver_tag,
                Err(err) => {
                    error!(target: "io-drivers:input", "failed to read driver tag: {}", err);
                    return Status::ReadFailed;
                }
            };
            debug!(target: "io-drivers:input", "driver tag = {}", driver_tag);

            let req = {
                let mut req = Vec::with_capacity(req_len);
                req.resize(req.capacity(), 0);
                if let Err(err) = reader.read_exact(&mut req).await {
                    error!(target: "io-drivers:input", "failed to read jammed request of length {}: {}", req_len, err);
                    return Status::ReadFailed;
                }
                Atom::from(req)
            };
            debug!(target: "io-drivers:input", "request = {}", req);

            match Noun::cue(req) {
                Ok(req) => match DriverTag::try_from(driver_tag) {
                    Ok(DriverTag::HttpClient) => {
                        if let Err(_req) = http_client_tx.send(req).await {
                            error!(target: "io-drivers:input", "failed to send {} request of length {} to HTTP client driver", driver_tag, req_len);
                            return Status::HttpClientFailed;
                        }
                    }
                    _ => warn!(target: "io-drivers:input", "unknown driver tag {}", driver_tag),
                },
                Err(err) => {
                    warn!(target: "io-drivers:input", "failed to deserialize {} request of length {}: {}", driver_tag, req_len, err)
                }
            }
        }
    });
    debug!(target: "io-drivers:input", "spawned input task");
    task
}

/// Reads outgoing IO responses from the drivers and writes the responses to an output source.
fn send_io_responses(
    mut writer: impl AsyncWriteExt + Send + Unpin + 'static,
    mut resp_rx: Receiver<Noun>,
) -> JoinHandle<Status> {
    let task = tokio::spawn(async move {
        let mut flush_retry_cnt = 0;
        const FLUSH_RETRY_MAX: usize = 5;
        debug!(target: "io-drivers:output", "max flush retry attempts = {}", FLUSH_RETRY_MAX);
        while let Some(resp) = resp_rx.recv().await {
            let mut resp = resp.jam().into_vec();
            let resp_len = u64::try_from(resp.len());
            if let Err(err) = resp_len {
                warn!(target: "io-drivers:output", "response length {} does not fit in u64: {}", resp.len(), err);
                continue;
            }
            let resp_len = resp_len.unwrap();
            debug!(target: "io-drivers:output", "response length = {}", resp_len);

            if let Err(err) = writer.write_u64_le(resp_len).await {
                error!(target: "io-drivers:output", "failed to write response length {}: {}", resp_len, err);
                return Status::WriteFailed;
            }

            if let Err(err) = writer.write_all(&mut resp).await {
                error!(target: "io-drivers:output", "failed to write jammed response {}: {}", Atom::from(resp), err);
                return Status::WriteFailed;
            }
            debug!(target: "io-drivers:output", "response = {}", Atom::from(resp));

            if let Err(err) = writer.flush().await {
                warn!(target: "io-drivers:output", "failed to flush output: {}", err);
                if flush_retry_cnt == FLUSH_RETRY_MAX {
                    error!(target: "io-drivers:output", "failing after {} of {} flush retries attempted", flush_retry_cnt, FLUSH_RETRY_MAX);
                    return Status::WriteFailed;
                } else {
                    flush_retry_cnt += 1;
                    info!(target: "io-drivers:output", "{} of {} flush retries remaining", FLUSH_RETRY_MAX - flush_retry_cnt, FLUSH_RETRY_MAX);
                }
            } else {
                flush_retry_cnt = 0;
            }
            debug!(target: "io-drivers:output", "flush retry count = {}", flush_retry_cnt);
        }
        Status::Success
    });
    debug!(target: "io-drivers:output", "spawned output task");
    task
}

/// Constructs a [tokio] runtime.
///
/// [tokio]: https://docs.rs/tokio/latest/tokio/index.html
fn runtime() -> Runtime {
    {
        #[cfg(feature = "multi-thread")]
        {
            let runtime = runtime::Builder::new_multi_thread();
            debug!(target: "io-drivers:init", "created multi-threaded tokio runtime");
            runtime
        }
        #[cfg(not(feature = "multi-thread"))]
        {
            let runtime = runtime::Builder::new_current_thread();
            debug!(target: "io-drivers:init", "created single-threaded tokio runtime");
            runtime
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
/// driver tag            (1  byte)
/// jammed request        (>1 bytes)
/// ```
///
/// The jammed request is dispatched to the appropriate driver based off the driver tag in the IO
/// request. Once the driver handles the request, it writes the response to `stdout`.
///
/// The following drivers are currently supported:
/// - HTTP client.
#[no_mangle]
pub extern "C" fn run() -> Status {
    runtime().block_on(async {
        // TODO: decide if there's a better upper bound for number of unscheduled requests.
        const QUEUE_SIZE: usize = 32;

        // driver tasks -> output task
        let (resp_tx, resp_rx): Channel<Noun> = mpsc::channel(QUEUE_SIZE);
        let output_task = send_io_responses(io::stdout(), resp_rx);

        // scheduling task -> http client driver task
        let (http_client_tx, http_client_rx): Channel<Noun> = mpsc::channel(QUEUE_SIZE);
        let http_client_task = HttpClient::new().run(http_client_rx, resp_tx);

        // input task -> scheduling task
        let input_task = recv_io_requests(io::stdin(), http_client_tx);

        let input_status = if let Ok(status) = input_task.await {
            status
        } else {
            Status::ReadFailed
        };

        let http_client_status = if let Ok(status) = http_client_task.await {
            status
        } else {
            Status::HttpClientFailed
        };

        let output_status = if let Ok(status) = output_task.await {
            status
        } else {
            Status::WriteFailed
        };

        if !output_status.success() {
            output_status
        } else if !input_status.success() {
            input_status
        } else if !http_client_status.success() {
            http_client_status
        } else {
            Status::Success
        }
    })
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
                7,
                0,
                0,
                0,
                0,
                0,
                0,
                0,           // Length of (jam %hello).
                DriverTag::HttpClient as u8, // Tag.
                128,
                7,
                173,
                140,
                141,
                237,
                13, // (%jam hello)
            ];

            let reader = BufReader::new(&REQ[..]);
            let (req_tx, mut req_rx): Channel<Noun> = mpsc::channel(8);

            super::recv_io_requests(reader, req_tx);

            if let Noun::Atom(req) = req_rx.recv().await.expect("recv") {
                assert_eq!(req, "hello");
            } else {
                panic!("unexpected cell");
            }
        });
    }
}
