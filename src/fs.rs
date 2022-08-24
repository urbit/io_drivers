use crate::{atom_as_str, Driver, Status};
use log::debug;
use noun::{
    atom::Atom,
    cell::Cell,
    convert::{self, TryFromNoun, TryIntoNoun},
    marker::Nounish,
    Noun,
};
use std::path;
use tokio::{
    io::{self, Stdin, Stdout},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

//==================================================================================================
// Request types
//==================================================================================================

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
                // data.tail() is `can`, which is a null-terminated list of pairs
                // each pair appears to [<path within mount point> <file type>]
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

//==================================================================================================
// Driver
//==================================================================================================

/// The file system driver.
// Seems like the driver needs to maintain a list of mount points.
pub struct FileSystem {}

impl FileSystem {
    fn update_file_system(&self, _req: UpdateFileSystem) {
        todo!()
    }

    fn commit_mount_point(&self, _req: CommitMountPoint) {
        todo!()
    }

    fn delete_mount_point(&self, _req: DeleteMountPoint) {
        todo!()
    }

    fn list_mount_points(&self, _req: ListMountPoints) {
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
        Ok(driver) => driver.run(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}

//==================================================================================================
// Miscellaneous
//==================================================================================================

/// A `$knot`.
///
/// A `$knot` is simply an ASCII string.
struct Knot<'a>(&'a Atom);

impl<'a> Nounish for Knot<'a> {}

/// A component of a file system path.
struct PathComponent(String);

impl TryFromNoun<Knot<'_>> for PathComponent {
    fn try_from_noun(knot: Knot) -> Result<Self, convert::Error> {
        let knot = atom_as_str(knot.0)?;
        // A path component should not have a path separator in it.
        if knot.contains(path::MAIN_SEPARATOR) {
            return Err(convert::Error::ImplType);
        }
        // The empty knot (`%$`), `.` knot, `..` knot, and any knots beginning with `!`
        // must be escaped by prepending a `!` to the path component.
        let path_component =
            if knot.is_empty() || knot == "." || knot == ".." || knot.chars().nth(0) == Some('!') {
                format!("!{}", knot)
            } else {
                knot.to_string()
            };
        Ok(Self(path_component))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_knot() -> Result<(), convert::Error> {
        macro_rules! test {
            (knot: $knot:literal, path_component: $path_component:literal) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                let path_component = PathComponent::try_from_noun(knot)?;
                assert_eq!(path_component.0, $path_component);
            };
            (knot: $knot:expr) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                assert!(PathComponent::try_from_noun(knot).is_err());
            };
        }

        test!(knot: "hello", path_component: "hello");
        test!(knot: "wow this is a long component", path_component: "wow this is a long component");
        test!(knot: "", path_component: "!");
        test!(knot: ".", path_component: "!.");
        test!(knot: "..", path_component: "!..");
        test!(knot: "!bu4hao3yi4si", path_component: "!!bu4hao3yi4si");
        test!(knot: format!("{}at-the-beginning", path::MAIN_SEPARATOR));
        test!(knot: format!("at-the-end{}", path::MAIN_SEPARATOR));
        test!(knot: format!("in{}between", path::MAIN_SEPARATOR));

        Ok(())
    }
}
