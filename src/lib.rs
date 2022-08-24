#[cfg(feature = "file-system")]
pub mod fs;
#[cfg(feature = "http-client")]
pub mod http;

use log::{debug, error, info, warn};
use noun::{
    atom::Atom,
    convert,
    serdes::{Cue, Jam},
    Noun,
};
use std::{
    marker::{Send, Unpin},
    process::{ExitCode, Termination},
};
use tokio::{
    self,
    io::{AsyncReadExt, AsyncWriteExt, ErrorKind},
    runtime,
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

type Channel<T> = (Sender<T>, Receiver<T>);

/// The return status of a driver.
#[derive(Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Success = 0,
    /// Reading IO requests from the input source failed.
    BadSource,
    /// Reading/writing to/from a channel failed.
    BadChannel,
    /// Writing IO responses to the output sink failed.
    BadSink,
    /// Creating a Tokio runtime failed.
    NoRuntime,
    /// Creating a driver failed.
    NoDriver,
}

impl Termination for Status {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

/// A generic IO driver.
///
/// A driver is designed to run in its own process. It asynchronously receives IO requests from some
/// input source and sends the responses to those requests to some output source.
///
/// The form of an IO request is:
/// ```text
/// jammed_request length (8 bytes, little endian)
/// jammed request        (>1 byte)
/// ````
///
/// The form of an IO response is:
/// ```text
/// jammed_response length (8 bytes, little endian)
/// jammed response        (>1 byte)
/// ```
pub trait Driver<I, O>
where
    I: AsyncReadExt + Send + Unpin + 'static,
    O: AsyncWriteExt + Send + Unpin + 'static,
    Self: Sized,
{
    /// Initializes a new driver.
    fn new() -> Result<Self, Status>;

    /// Returns the name of the driver.
    fn name() -> &'static str;

    /// Spawns a blocking task to asynchronously handle IO requests.
    ///
    /// This is the driver entry point.
    ///
    /// Handles requests as long as the input source is open. Responses are sent to the output
    /// sink.
    fn run(self, input_src: I, output_sink: O) -> Status {
        let runtime = runtime::Builder::new_multi_thread().enable_all().build();
        if let Err(err) = runtime {
            error!(
                target: Self::name(),
                "could not create Tokio runtime: {}", err
            );
            return Status::NoRuntime;
        }
        runtime.unwrap().block_on(async {
            const QUEUE_SIZE: usize = 32;
            // Channel from input task to handling task.
            let (input_tx, input_rx): Channel<Noun> = mpsc::channel(QUEUE_SIZE);
            // Channel from handling task to output task.
            let (output_tx, output_rx): Channel<Noun> = mpsc::channel(QUEUE_SIZE);

            let input_task = Self::recv_requests(input_src, input_tx);
            let handling_task = self.handle_requests(input_rx, output_tx);
            let output_task = Self::send_responses(output_rx, output_sink);

            // TODO: handle errors.
            input_task.await.unwrap();
            handling_task.await.unwrap();
            output_task.await.unwrap();

            Status::Success
        })
    }

    /// Spawns a task to read incoming IO requests from an input sink.
    ///
    /// This task is referred to as the "input task".
    fn recv_requests(mut input_src: I, input_tx: Sender<Noun>) -> JoinHandle<Status> {
        let task = tokio::spawn(async move {
            loop {
                let req_len = match input_src.read_u64_le().await {
                    Ok(0) => {
                        info!(target: Self::name(), "encountered EOF");
                        return Status::Success;
                    }
                    Ok(req_len) => {
                        if let Ok(req_len) = usize::try_from(req_len) {
                            req_len
                        } else {
                            error!(
                                target: Self::name(),
                                "request length {} does not fit in usize", req_len
                            );
                            return Status::BadSource;
                        }
                    }
                    Err(err) => match err.kind() {
                        ErrorKind::UnexpectedEof => {
                            info!(target: Self::name(), "encountered EOF");
                            return Status::Success;
                        }
                        err => {
                            error!(
                                target: Self::name(),
                                "failed to read request length: {}", err
                            );
                            return Status::BadSource;
                        }
                    },
                };
                debug!(target: Self::name(), "request length = {}", req_len);

                let req = {
                    let mut req = Vec::with_capacity(req_len);
                    // Extend the length to match the capacity.
                    req.resize(req.capacity(), 0);
                    if let Err(err) = input_src.read_exact(&mut req).await {
                        error!(
                            target: Self::name(),
                            "failed to read request of length {}: {}", req_len, err
                        );
                        return Status::BadSource;
                    }
                    Atom::from(req)
                };
                debug!(target: Self::name(), "request = {}", req);

                match Noun::cue(req) {
                    Ok(req) => {
                        if let Err(_req) = input_tx.send(req).await {
                            error!(
                                target: Self::name(),
                                "failed to send request of length {} to handling task", req_len
                            );
                            return Status::BadChannel;
                        }
                    }
                    Err(err) => {
                        warn!(
                            target: Self::name(),
                            "failed to deserialize request of length {}: {}", req_len, err
                        );
                    }
                }
            }
        });
        debug!(target: Self::name(), "spawned input task");
        task
    }

    /// Spawns a task to handle IO requests received from the input task and sends the
    /// corresponding responses to the output task.
    ///
    /// This task is referred to as the "handling task".
    fn handle_requests(
        self,
        input_rx: Receiver<Noun>,
        output_tx: Sender<Noun>,
    ) -> JoinHandle<Status>;

    /// Spawns a task to write outgoing IO responses to an output sink.
    ///
    /// This task is referred to as the "output task".
    fn send_responses(
        mut output_rx: Receiver<Noun>,
        mut output_sink: O,
    ) -> JoinHandle<Status> {
        let task = tokio::spawn(async move {
            const FLUSH_RETRY_MAX: usize = 5;
            debug!(
                target: Self::name(),
                "max flush retry attempts = {}", FLUSH_RETRY_MAX
            );
            let mut flush_retry_cnt = 0;
            while let Some(resp) = output_rx.recv().await {
                let mut resp = resp.jam().into_vec();
                let resp_len = {
                    let resp_len = u64::try_from(resp.len());
                    if let Err(err) = resp_len {
                        warn!(
                            target: Self::name(),
                            "response length {} does not fit in u64: {}",
                            resp.len(),
                            err
                        );
                        continue;
                    }
                    resp_len.unwrap()
                };
                debug!(target: Self::name(), "response length = {}", resp_len);

                if let Err(err) = output_sink.write_u64_le(resp_len).await {
                    error!(
                        target: Self::name(),
                        "failed to write response length {}: {}", resp_len, err
                    );
                    return Status::BadSink;
                }

                if let Err(err) = output_sink.write_all(&mut resp).await {
                    error!(
                        target: Self::name(),
                        "failed to read response of length {}: {}", resp_len, err
                    );
                    return Status::BadSink;
                }
                debug!(target: Self::name(), "response = {}", Atom::from(resp));

                if let Err(err) = output_sink.flush().await {
                    warn!(target: Self::name(), "failed to flush output: {}", err);
                    if flush_retry_cnt == FLUSH_RETRY_MAX {
                        error!(
                            target: Self::name(),
                            "failing after {} of {} flush retries attempted",
                            flush_retry_cnt,
                            FLUSH_RETRY_MAX
                        );
                        return Status::BadSink;
                    } else {
                        flush_retry_cnt += 1;
                        info!(
                            target: Self::name(),
                            "{} of {} flush retries attempted", flush_retry_cnt, FLUSH_RETRY_MAX
                        );
                    }
                } else {
                    flush_retry_cnt = 0;
                }
                debug!(
                    target: Self::name(),
                    "flush retry count = {}", flush_retry_cnt
                );
            }
            Status::Success
        });
        debug!(target: Self::name(), "spawned output task");
        task
    }
}

/// Converts an atom into a string, returning a `convert::Error` if the operation failed.
///
/// This function exists purely for convenience.
fn atom_as_str(atom: &Atom) -> Result<&str, convert::Error> {
    atom.as_str().map_err(|_| convert::Error::AtomToStr)
}
