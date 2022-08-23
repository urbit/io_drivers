use crate::{Driver, Status, QUEUE_SIZE};
use log::{debug, warn};
use noun::{
    convert::{self, TryFromNoun},
    Noun,
};
use tokio::{
    io::{self, Stdin, Stdout},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

/// Types of requests that can be handled by the filesystem driver.
#[repr(u32)]
enum Tag {
    UpdateFileSystem,
    CommitMountPoint,
    DeleteMountPoint,
    ListMountPoints,
}

impl TryFromNoun<&Noun> for Tag {
    fn try_from_noun(noun: &Noun) -> Result<Self, convert::Error> {
        if let Noun::Atom(atom) = noun {
            if let Ok(atom) = atom.as_str() {
                // These tag names are terrible, but we unfortunately can't do anything about it
                // here because they're determined by the kernel.
                match atom {
                    "ergo" => Ok(Self::UpdateFileSystem),
                    "dirk" => Ok(Self::CommitMountPoint),
                    "ogre" => Ok(Self::DeleteMountPoint),
                    "hill" => Ok(Self::ListMountPoints),
                    _ => Err(convert::Error::ImplType),
                }
            } else {
                Err(convert::Error::AtomToStr)
            }
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

/// The filesystem driver.
pub struct FileSystem {}

impl FileSystem {
    fn update_file_system(&self) {
        todo!()
    }

    fn commit_mount_point(&self) {
        todo!()
    }

    fn delete_mount_point(&self) {
        todo!()
    }

    fn list_mount_points(&self) {
        todo!()
    }
}

/// Implements the [`Driver`] trait for the [`FileSystem`] driver.
macro_rules! impl_driver {
    ($input_src:ty, $output_sink:ty) => {
        impl Driver<$input_src, $output_sink> for FileSystem {
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
                            match Tag::try_from_noun(&*tag) {
                                Ok(Tag::UpdateFileSystem) => self.update_file_system(),
                                Ok(Tag::CommitMountPoint) => self.commit_mount_point(),
                                Ok(Tag::DeleteMountPoint) => self.delete_mount_point(),
                                Ok(Tag::ListMountPoints) => self.list_mount_points(),
                                _ => {
                                    warn!(
                                        target: Self::name(),
                                        "ignoring request with unknown tag %{}", tag
                                    );
                                }
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
    match FileSystem::new() {
        Ok(driver) => driver.run::<QUEUE_SIZE>(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}
