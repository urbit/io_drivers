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
    UpdateFileSystem = mote!('e', 'r', 'g', 'o'),
    CommitMountPoint = mote!('d', 'i', 'r', 'k'),
    DeleteMountPoint = mote!('o', 'g', 'r', 'e'),
    ListMountPoints = mote!('h', 'i', 'l', 'l'),
}

impl TryFrom<&Atom> for Tag {
    type Error = ();

    fn try_from(atom: &Atom) -> Result<Self, Self::Error> {
        if let Some(atom) = atom.as_u32() {
            if atom == Self::UpdateFileSystem as u32 {
                Ok(Self::UpdateFileSystem)
            } else if atom == Self::CommitMountPoint as u32 {
                Ok(Self::CommitMountPoint)
            } else if atom == Self::DeleteMountPoint as u32 {
                Ok(Self::DeleteMountPoint)
            } else if atom == Self::ListMountPoints as u32 {
                Ok(Self::ListMountPoints)
            } else {
                Err(())
            }
        } else {
            Err(())
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
                            if let Noun::Atom(tag) = &*tag {
                                match Tag::try_from(tag) {
                                    Ok(Tag::UpdateFileSystem) => self.update_file_system(),
                                    Ok(Tag::CommitMountPoint) => self.commit_mount_point(),
                                    Ok(Tag::DeleteMountPoint) => self.delete_mount_point(),
                                    Ok(Tag::ListMountPoints) => self.list_mount_points(),
                                    _ => {
                                        if let Ok(tag) = tag.as_str() {
                                            warn!(
                                                target: Self::name(),
                                                "ignoring request with unknown tag %{}", tag
                                            );
                                        } else {
                                            warn!(
                                                target: Self::name(),
                                                "ignoring request with unknown tag %{}", tag
                                            );
                                        }
                                    }
                                }
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
    match FileSystem::new() {
        Ok(driver) => driver.run::<QUEUE_SIZE>(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}
