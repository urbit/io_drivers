use crate::{atom_as_str, Driver, Status};
use log::debug;
use noun::{
    atom::Atom,
    cell::Cell,
    convert::{self, TryFromNoun, TryIntoNoun},
    marker::{Atomish, Cellish, Nounish},
    Noun,
};
use std::path::{self, PathBuf};
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
struct Knot<A: Atomish>(A);

impl Nounish for Knot<&Atom> {}

impl Nounish for Knot<Atom> {}

/// A list of `$knot`.
struct KnotList<C: Cellish>(C);

impl Nounish for KnotList<&Cell> {}

impl Nounish for KnotList<Cell> {}

/// A component of a file system path.
struct PathComponent(String);

/// Enables a [`PathComponent`] to be pushed onto a [`std::path::Path`] or [`std::path::PathBuf`].
impl AsRef<path::Path> for PathComponent {
    fn as_ref(&self) -> &path::Path {
        self.0.as_ref()
    }
}

impl TryFromNoun<Knot<&Atom>> for PathComponent {
    fn try_from_noun(knot: Knot<&Atom>) -> Result<Self, convert::Error> {
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

impl TryIntoNoun<Knot<Atom>> for PathComponent {
    type Error = ();

    fn try_into_noun(self) -> Result<Knot<Atom>, Self::Error> {
        // This is unlikely to ever occur because a [`PathComponent`] should only ever be created
        // using [`TryFromNoun<Knot<&Atom>>`], but we check when compiling in debug mode just to be
        // safe.
        #[cfg(debug_assertions)]
        if self.0.contains(path::MAIN_SEPARATOR) {
            return Err(());
        }

        let knot = if self.0.chars().nth(0) == Some('!') {
            &self.0[1..]
        } else {
            &self.0[..]
        };
        Ok(Knot(Atom::from(knot)))
    }
}

/// A file system path.
struct Path(PathBuf);

impl TryFromNoun<KnotList<&Cell>> for Path {
    fn try_from_noun(knot_list: KnotList<&Cell>) -> Result<Self, convert::Error> {
        let mut path = PathBuf::new();
        for knot in knot_list.0.to_vec() {
            if let Noun::Atom(knot) = &*knot {
                let path_component = PathComponent::try_from_noun(Knot(knot))?;
                path.push(path_component);
            } else {
                return Err(convert::Error::UnexpectedCell);
            }
        }
        Ok(Self(path))
    }
}

impl TryIntoNoun<KnotList<Cell>> for Path {
    type Error = ();

    fn try_into_noun(self) -> Result<KnotList<Cell>, Self::Error> {
        let mut path_components = Vec::new();
        for path_component in self.0.components() {
            let path_component =
                PathComponent(path_component.as_os_str().to_str().ok_or(())?.to_string());
            let knot = path_component.try_into_noun()?;
            path_components.push(knot.0.into_rc_noun());
        }
        Ok(KnotList(Cell::from(path_components)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_knot() -> Result<(), convert::Error> {
        macro_rules! test {
            // Expect success.
            (knot: $knot:literal, path_component: $path_component:literal) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                let path_component = PathComponent::try_from_noun(knot)?;
                assert_eq!(path_component.0, $path_component);
            };
            // Expect failure.
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

    #[test]
    fn try_into_knot() -> Result<(), ()> {
        macro_rules! test {
            // Expect success.
            (path_component: $path_component:literal, knot: $knot:literal) => {
                let path_component = PathComponent($path_component.to_string());
                let knot = path_component.try_into_noun()?;
                assert_eq!(knot.0, $knot);
            };
            // Expect failure.
            (path_component: $path_component:expr) => {
                let path_component = PathComponent($path_component.to_string());
                assert!(path_component.try_into_noun().is_err());
            };
        }

        test!(path_component: "goodbye", knot: "goodbye");
        test!(path_component: "a_little_longer", knot: "a_little_longer");
        test!(path_component: "!", knot: "");
        test!(path_component: "!.", knot: ".");
        test!(path_component: "!..", knot: "..");
        test!(path_component: "!!double-down", knot: "!double-down");

        #[cfg(debug_assertions)]
        {
            test!(path_component: format!("{}start", path::MAIN_SEPARATOR));
            test!(path_component: format!("end{}", path::MAIN_SEPARATOR));
            test!(
                path_component:
                    format!(
                        "neither{}start{}nor{}end",
                        path::MAIN_SEPARATOR,
                        path::MAIN_SEPARATOR,
                        path::MAIN_SEPARATOR
                    )
            );
        }

        Ok(())
    }

    #[test]
    fn try_from_knot_list() -> Result<(), convert::Error> {
        macro_rules! test {
            // Expect success.
            (knot_list: $knot_list:expr, path: $path:literal) => {
                let cell = Cell::from($knot_list);
                let knot_list = KnotList(&cell);
                let path = Path::try_from_noun(knot_list)?;
                assert_eq!(path.0, path::Path::new($path));
            };
            // Expect failure.
            (knot_list: $knot_list:expr) => {
                let cell = Cell::from($knot_list);
                let knot_list = KnotList(&cell);
                assert!(Path::try_from_noun(knot_list).is_err());
            };
        }

        test!(knot_list: ["hello", "goodbye"], path: "hello/goodbye");
        test!(knot_list: ["some", ".", "path"], path: "some/!./path");
        test!(knot_list: ["..", "!", "", "jian3", "fei2"], path: "!../!!/!/jian3/fei2");
        test!(knot_list: [&format!("{}uh-oh", path::MAIN_SEPARATOR), "gan4ma2"]);

        Ok(())
    }

    #[test]
    fn try_into_knot_list() -> Result<(), ()> {
        macro_rules! test {
            // Expect success.
            (path: $path:literal, knot_list: $knot_list:expr) => {
                let path = Path(PathBuf::from($path));
                let knot_list = path.try_into_noun()?;
                assert_eq!(knot_list.0, Cell::from($knot_list))
            };
            // Expect failure.
            (path: $path:expr) => {
                let path = Path(PathBuf::from($path));
                assert!(path.try_into_noun().is_err());
            };
        }

        test!(path: "la/dee/da", knot_list: ["la", "dee", "da"]);
        test!(path: "some/!!escaped/path", knot_list: ["some", "!escaped", "path"]);
        test!(path: "!./!../!/more/components", knot_list: [".", "..", "", "more", "components"]);
        test!(
            path: format!(
                "{}the{}usual{}",
                path::MAIN_SEPARATOR,
                path::MAIN_SEPARATOR,
                path::MAIN_SEPARATOR
            )
        );

        Ok(())
    }
}
