use crate::{mote, Driver, Status, QUEUE_SIZE};
use log::{debug, warn};
use noun::{atom::Atom, Noun};
use tokio::{
    io::{self, Stdin, Stdout},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

/// Types of requests that can be handled by the filesystem driver.
#[repr(u32)]
enum Tag {
    UpdateFilesystem = mote!('e', 'r', 'g', 'o'),
    CommitMountPoint = mote!('d', 'i', 'r', 'k'),
    DeleteMountPoint = mote!('o', 'g', 'r', 'e'),
    ListMountPoints = mote!('h', 'i', 'l', 'l'),
}

impl PartialEq<Atom> for Tag {
    fn eq(&self, other: &Atom) -> bool {
        if let Some(other) = other.as_u32() {
            self == &other
        } else {
            false
        }
    }
}

impl PartialEq<u32> for Tag {
    fn eq(&self, other: &u32) -> bool {
        self == other
    }
}

/// The filesystem driver.
pub struct Filesystem {}

impl Filesystem {}

/// Implements the [`Driver`] trait for the [`Filesystem`] driver.
macro_rules! impl_driver {
    ($input_src:ty, $output_sink:ty) => {
        impl Driver<$input_src, $output_sink> for Filesystem {
            fn new() -> Result<Self, Status> {
                Ok(Self {})
            }

            fn name() -> &'static str {
                "filesystem"
            }

            fn handle_requests(
                self,
                mut input_rx: Receiver<Noun>,
                _output_tx: Sender<Noun>,
            ) -> JoinHandle<Status> {
                let task = tokio::spawn(async move {
                    while let Some(req) = input_rx.recv().await {
                        if let Noun::Cell(req) = req {
                            let (tag, req) = req.into_parts();
                            if let Noun::Atom(tag) = &*tag {
                                todo!()
                            } else {
                                warn!(
                                    target: Self::name(),
                                    "ignoring request because the tag is a cell"
                                );
                            }
                        } else {
                            warn!(
                                target: Self::name(),
                                "ignoring request because it's an atom"
                            );
                        }
                    }
                    Status::Success
                });
                debug!(target: Self::name(), "spawned handling task");
                task
            }
        }
    };
}

impl_driver!(Stdin, Stdout);

/// Provides an FFI-friendly interface for running the filesystem driver with `stdin` as the input
/// source and `stdout` as the output sink.
#[no_mangle]
pub extern "C" fn filesystem_run() -> Status {
    match Filesystem::new() {
        Ok(driver) => driver.run::<QUEUE_SIZE>(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}
