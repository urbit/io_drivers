use crate::{atom_as_str, Driver, Status, QUEUE_SIZE};
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

/// Requests that can be handled by the file system driver.
enum Request {
    UpdateFileSystem(UpdateFileSystem),
    CommitMountPoint(CommitMountPoint),
    DeleteMountPoint(DeleteMountPoint),
    ListMountPoints(ListMountPoints),
}

impl TryFromNoun<Noun> for Request {
    fn try_from_noun(req: Noun) -> Result<Self, convert::Error> {
        if let Noun::Cell(req) = req {
            let (tag, data) = req.into_parts();
            if let Noun::Atom(tag) = &*tag {
                // These tag names are terrible, but we unfortunatley can't do anything about
                // it here because they're determined by the kernel.
                match atom_as_str(tag)? {
                    // Update the file system.
                    "ergo" => Ok(Self::UpdateFileSystem(UpdateFileSystem::try_from_noun(
                        &*data,
                    )?)),
                    _ => todo!(),
                }
            } else {
                Err(convert::Error::UnexpectedCell)
            }
        } else {
            Err(convert::Error::UnexpectedAtom)
        }
    }
}

/// A request to update the file system.
struct UpdateFileSystem {
    mount_point: String,
}

impl TryFromNoun<&Noun> for UpdateFileSystem {
    fn try_from_noun(data: &Noun) -> Result<Self, convert::Error> {
        if let Noun::Cell(data) = &*data {
            if let Noun::Atom(mount_point) = &*data.head() {
                Ok(Self {
                    mount_point: atom_as_str(mount_point)?.to_string(),
                })
            } else {
                Err(convert::Error::UnexpectedCell)
            }
        } else {
            Err(convert::Error::UnexpectedAtom)
        }
    }
}

/// A request to commit a mount point.
struct CommitMountPoint {}

/// A request to delete a mount point.
struct DeleteMountPoint {}

/// A request to list the mount points.
struct ListMountPoints {}

/// The file system driver.
pub struct FileSystem {}

impl FileSystem {
    fn update_file_system(&self, req: UpdateFileSystem) {
        todo!()
    }

    fn commit_mount_point(&self, req: CommitMountPoint) {
        todo!()
    }

    fn delete_mount_point(&self, req: DeleteMountPoint) {
        todo!()
    }

    fn list_mount_points(&self, req: ListMountPoints) {
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
                "file-system"
            }

            fn handle_requests(
                self,
                mut input_rx: Receiver<Noun>,
                _output_tx: Sender<Noun>,
            ) -> JoinHandle<Status> {
                let task = tokio::spawn(async move {
                    while let Some(req) = input_rx.recv().await {
                        match Request::try_from_noun(req) {
                            Ok(Request::UpdateFileSystem(req)) => self.update_file_system(req),
                            Ok(Request::CommitMountPoint(req)) => self.commit_mount_point(req),
                            Ok(Request::DeleteMountPoint(req)) => self.delete_mount_point(req),
                            Ok(Request::ListMountPoints(req)) => self.list_mount_points(req),
                            _ => todo!(),
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

/// Provides an FFI-friendly interface for running the file system driver with `stdin` as the input
/// source and `stdout` as the output sink.
#[no_mangle]
pub extern "C" fn file_system_run() -> Status {
    match FileSystem::new() {
        Ok(driver) => driver.run::<QUEUE_SIZE>(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}
