#![allow(dead_code, unreachable_code)]

use crate::{atom_as_str, Driver, Status};
use log::{debug, warn};
use noun::{
    atom::Atom,
    cell::Cell,
    convert,
    marker::{Atomish, Cellish},
    Noun, Rc,
};
use std::{
    env,
    ffi::OsStr,
    fmt, fs,
    path::{self, Path, PathBuf},
};
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
}

impl TryFrom<Noun> for Request {
    type Error = convert::Error;

    fn try_from(req: Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(req) = req {
            let (tag, data) = req.into_parts();
            if let Noun::Atom(tag) = &*tag {
                // These tag names are terrible, but we unfortunately can't do anything about
                // it here because they're determined by the kernel.
                match atom_as_str(tag)? {
                    "ergo" => Ok(Self::UpdateFileSystem(UpdateFileSystem::try_from(&*data)?)),
                    _tag => Err(convert::Error::ImplType),
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
    /// The mount point to update.
    mount_point: PathComponent,

    /// Changes to apply to the file system.
    changes: Vec<Change>,
}

impl TryFrom<&Noun> for UpdateFileSystem {
    type Error = convert::Error;

    /// A properly structured noun is:
    ///
    /// ```text
    /// [<mount_point> <change_list>]
    /// ```
    ///
    /// where `<change_list>` is a null-terminated list of changes to make to the file system. See
    /// [`Change`] for the structure of a single change.
    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(data) = &*data {
            if let Noun::Atom(head) = &*data.head() {
                let mount_point = PathComponent::try_from(Knot(head))?;
                if let Noun::Cell(tail) = &*data.tail() {
                    let mut tail = tail.to_vec();
                    // Remove null terminator.
                    tail.pop();
                    let mut changes = Vec::new();
                    for change in tail {
                        changes.push(Change::try_from(&*change)?);
                    }
                    Ok(Self {
                        mount_point,
                        changes,
                    })
                } else {
                    Err(convert::Error::UnexpectedAtom)
                }
            } else {
                Err(convert::Error::UnexpectedCell)
            }
        } else {
            Err(convert::Error::UnexpectedAtom)
        }
    }
}

//==================================================================================================
// Driver
//==================================================================================================

/// The file system driver.
pub struct FileSystem;

impl FileSystem {
    fn update_file_system(&self, req: UpdateFileSystem) {
        let mount_point = match env::current_dir() {
            Ok(mut cwd) => {
                cwd.push(req.mount_point);
                cwd
            }
            Err(err) => {
                warn!(
                    target: Self::name(),
                    "failed to access current directory: {}", err
                );
                return;
            }
        };

        for change in req.changes {
            match change {
                Change::EditFile { path, bytes } => {
                    // TODO: track hashes of files and don't update file if the hash hasn't
                    // changed.
                    let path: PathBuf = [&mount_point, &path].iter().collect();
                    fs::write(path, bytes).expect("write file");
                }
                Change::RemoveFile { path } => {
                    let path: PathBuf = [&mount_point, &path].iter().collect();
                    fs::remove_file(path).expect("remove file");
                },
            }
        }
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
                        match Request::try_from(req) {
                            Ok(Request::UpdateFileSystem(req)) => self.update_file_system(req),
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
// Path Manipulation
//==================================================================================================

/// A `$knot`.
///
/// A `$knot` is simply an ASCII string.
struct Knot<A: Atomish>(A);

/// Attempts to create a [`Knot`] from a [`PathComponent`].
impl TryFrom<PathComponent> for Knot<Atom> {
    type Error = ();

    fn try_from(path_component: PathComponent) -> Result<Self, Self::Error> {
        // This is unlikely to ever occur because a [`PathComponent`] should only ever be created
        // using [`TryFromNoun<Knot<&Atom>>`], but we check when compiling in debug mode just to be
        // safe.
        #[cfg(debug_assertions)]
        if path_component.0.contains(path::MAIN_SEPARATOR) {
            return Err(());
        }

        let knot = if path_component.0.chars().nth(0) == Some('!') {
            &path_component.0[1..]
        } else {
            &path_component.0[..]
        };
        Ok(Knot(Atom::from(knot)))
    }
}

/// A null-terminated list of `$knot`.
struct KnotList<C: Cellish>(C);

impl TryFrom<&Path> for KnotList<Cell> {
    type Error = ();

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let mut path_components = Vec::new();
        if let Some(parent) = path.parent() {
            for path_component in parent.components() {
                let path_component =
                    PathComponent(path_component.as_os_str().to_str().ok_or(())?.to_string());
                let knot = Knot::try_from(path_component)?;
                path_components.push(Rc::<Noun>::from(knot.0));
            }
        }
        if let Some(stem) = path.file_stem() {
            let stem = Atom::try_from(stem)?;
            path_components.push(Rc::<Noun>::from(stem));
            if let Some(extension) = path.extension() {
                let extension = Atom::try_from(extension)?;
                path_components.push(Rc::<Noun>::from(extension));
            }
            path_components.push(Rc::<Noun>::from(Atom::null()));
            Ok(KnotList(Cell::from(path_components)))
        } else {
            Err(())
        }
    }
}

/// A component of a file system path.
///
/// A [`PathComponent`] is guaranteed to never be `.` or `..`.
#[derive(Clone, Eq, Hash, PartialEq)]
struct PathComponent(String);

/// Enables a [`PathComponent`] to be pushed onto a [`std::path::Path`] or [`std::path::PathBuf`].
impl AsRef<Path> for PathComponent {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl fmt::Display for PathComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<Knot<&Atom>> for PathComponent {
    type Error = convert::Error;

    fn try_from(knot: Knot<&Atom>) -> Result<Self, Self::Error> {
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

impl TryFrom<&Noun> for PathComponent {
    type Error = convert::Error;

    fn try_from(noun: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Atom(atom) = noun {
            Self::try_from(Knot(atom))
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

impl TryFrom<&OsStr> for PathComponent {
    type Error = ();

    fn try_from(os_str: &OsStr) -> Result<Self, Self::Error> {
        match os_str.to_str() {
            // TODO: escape with `!` if `os_str` is ``, `.`, or `..`.
            Some(".") | Some("..") | None => Err(()),
            Some(string) => Ok(Self(string.to_string())),
        }
    }
}

impl TryFrom<KnotList<&Cell>> for PathBuf {
    type Error = convert::Error;

    fn try_from(knot_list: KnotList<&Cell>) -> Result<Self, Self::Error> {
        let mut path = PathBuf::new();
        let mut knot_list = knot_list.0.to_vec();
        // Remove null terminator.
        knot_list.pop();
        for knot in knot_list {
            if let Noun::Atom(knot) = &*knot {
                // TODO: handle file_name.extension case.
                let path_component = PathComponent::try_from(Knot(knot))?;
                path.push(path_component);
            } else {
                return Err(convert::Error::UnexpectedCell);
            }
        }
        Ok(path)
    }
}

//==================================================================================================
// File System Entries
//==================================================================================================

/// A file system mount point.
///
/// All mount points reside within the root directory of a ship (i.e. the pier directory).
struct MountPoint(PathBuf);

/// A change to the file system.
enum Change {
    /// Edit a file in place.
    EditFile {
        /// Mount-point-relative path to the file.
        path: PathBuf,

        /// New contents of the file.
        bytes: Vec<u8>,
    },

    /// Remove a file from the file system.
    RemoveFile {
        /// Mount-point-relative path to the file.
        path: PathBuf,
    },
}

impl TryFrom<&Noun> for Change {
    type Error = convert::Error;

    /// A properly structured noun is one of:
    ///
    /// ```text
    /// [<path_list> 0]
    /// [<path_list> 0 <file_type_list> <byte_count> <bytes>]
    /// ```
    ///
    /// The former structure removes a file at `<path_list>`, whereas the latter structure edits a
    /// file of type `<file_type_list>` at `<path_list>`, replacing the previous file contents with
    /// `<bytes>`.
    ///
    /// As a concrete example, writing `|=  a=@  +(a)` (a 13-byte change) to
    /// `<pier>/base/gen/example.hoon` yields:
    /// ```text
    /// [
    ///   [%gen %example %hoon 0]
    ///   0
    ///   [%text %x-hoon 0]
    ///   14
    ///   0xa2961282b2020403d6120203d7c
    /// ]
    /// ```
    /// Note that `14` is the length of the change to `example.hoon` plus one (for the record
    /// separator i.e. ASCII `30`) and `0xa2961282b2020403d6120203d7c` is `|=  a=@  +(a)<RS>`
    /// represented as an atom (where `<RS>` is the record separator).
    ///
    /// Removing `<pier>/base/gen/example.hoon` yields:
    /// ```text
    /// [
    ///   [%gen %example %hoon 0]
    ///   0
    /// ]
    /// ```
    fn try_from(noun: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(cell) = noun {
            if let Noun::Cell(path) = &*cell.head() {
                let path = PathBuf::try_from(KnotList(path))?;
                match &*cell.tail() {
                    Noun::Atom(change) => {
                        if change.is_null() {
                            Ok(Change::RemoveFile { path })
                        } else {
                            Err(convert::Error::ImplType)
                        }
                    }
                    Noun::Cell(change) => {
                        let [null, _file_type_list, byte_len, bytes] =
                            change.to_array::<4>().ok_or(convert::Error::ImplType)?;
                        if let Noun::Atom(null) = &*null {
                            if let Noun::Atom(byte_len) = &*byte_len {
                                if let Noun::Atom(bytes) = &*bytes {
                                    if null.is_null() {
                                        let bytes = bytes.to_vec();
                                        debug_assert_eq!(
                                            byte_len.as_usize().expect("atom to usize"),
                                            bytes.len()
                                        );
                                        Ok(Change::EditFile { path, bytes })
                                    } else {
                                        Err(convert::Error::ImplType)
                                    }
                                } else {
                                    Err(convert::Error::UnexpectedCell)
                                }
                            } else {
                                Err(convert::Error::UnexpectedCell)
                            }
                        } else {
                            Err(convert::Error::UnexpectedCell)
                        }
                    }
                }
            } else {
                Err(convert::Error::UnexpectedAtom)
            }
        } else {
            Err(convert::Error::UnexpectedAtom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a file system path rooted at the current OS's temporary directory.
    macro_rules! path {
        ($($path_component:expr),+ $(,)?) => {{
            let mut path = std::path::PathBuf::new();
            #[cfg(target_os = "windows")]
            {
                path.push(env!("TEMP"));
            }
            #[cfg(not(target_os = "windows"))]
            {
                path.push("/tmp");
            }
            $(
                path.push($path_component);
            )+
            path
        }}
    }

    #[test]
    fn convert_knot() {
        macro_rules! test {
            // `Knot` -> `PathComponent`: expect success.
            (knot: $knot:literal, path_component: $path_component:literal) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                let path_component =
                    PathComponent::try_from(knot).expect("path component from knot");
                assert_eq!(path_component.0, $path_component);
            };
            // `Knot` -> `PathComponent`: expect failure.
            (knot: $knot:expr) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                assert!(PathComponent::try_from(knot).is_err());
            };
            // `PathComponent` -> `Knot`: expect success.
            (path_component: $path_component:literal, knot: $knot:literal) => {
                let path_component = PathComponent($path_component.to_string());
                let knot = Knot::try_from(path_component).expect("knot from path component");
                assert_eq!(knot.0, $knot);
            };
            // `PathComponent` -> `Knot`: expect failure.
            (path_component: $path_component:expr) => {
                let path_component = PathComponent($path_component.to_string());
                assert!(Knot::try_from(path_component).is_err());
            };
        }

        {
            test!(knot: "hello", path_component: "hello");
            test!(knot: "wow this is a long component", path_component: "wow this is a long component");
            test!(knot: "", path_component: "!");
            test!(knot: ".", path_component: "!.");
            test!(knot: "..", path_component: "!..");
            test!(knot: "!bu4hao3yi4si", path_component: "!!bu4hao3yi4si");
        }

        {
            test!(knot: format!("{}at-the-beginning", path::MAIN_SEPARATOR));
            test!(knot: format!("at-the-end{}", path::MAIN_SEPARATOR));
            test!(knot: format!("in{}between", path::MAIN_SEPARATOR));
        }

        {
            test!(path_component: "goodbye", knot: "goodbye");
            test!(path_component: "a_little_longer", knot: "a_little_longer");
            test!(path_component: "!", knot: "");
            test!(path_component: "!.", knot: ".");
            test!(path_component: "!..", knot: "..");
            test!(path_component: "!!double-down", knot: "!double-down");
        }

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
    }

    #[test]
    fn convert_knot_list() {
        macro_rules! test {
            // `KnotList` -> `Path`: expect success.
            (knot_list: $knot_list:expr, path: $path:literal) => {
                let cell = Cell::from($knot_list);
                let knot_list = KnotList(&cell);
                let path = PathBuf::try_from(knot_list).expect("path from knot list");
                assert_eq!(path, path::Path::new($path));
            };
            // `KnotList` -> `Path`: expect failure.
            (knot_list: $knot_list:expr) => {
                let cell = Cell::from($knot_list);
                let knot_list = KnotList(&cell);
                assert!(PathBuf::try_from(knot_list).is_err());
            };
            // `Path` -> `KnotList`: expect success.
            (path: $path:literal, knot_list: $knot_list:expr) => {
                let path = PathBuf::from($path);
                let knot_list = KnotList::try_from(path.as_path()).expect("knot list from path");
                assert_eq!(knot_list.0, Cell::from($knot_list))
            };
            // `Path` -> `KnotList`: expect failure.
            (path: $path:expr) => {
                let path = PathBuf::from($path);
                assert!(KnotList::try_from(path.as_path()).is_err());
            };
        }

        // `KnotList` -> `Path`: expect success.
        {
            test!(knot_list: ["hello", "goodbye", ""], path: "hello/goodbye");
            test!(knot_list: ["some", ".", "path", ""], path: "some/!./path");
            test!(knot_list: ["..", "!", "", "jian3", "fei2", ""], path: "!../!!/!/jian3/fei2");
        }

        // `KnotList` -> `Path`: expect failure.
        {
            test!(knot_list: [&format!("{}uh-oh", path::MAIN_SEPARATOR), "gan4ma2"]);
        }

        // `Path` -> `KnotList`: expect success.
        {
            test!(path: "la/dee/da", knot_list: ["la", "dee", "da", ""]);
            test!(path: "a/b/c.d", knot_list: ["a", "b", "c", "d", ""]);
            test!(path: "some/!!escaped/path", knot_list: ["some", "!escaped", "path", ""]);
            test!(path: "!./!../!/more/components", knot_list: [".", "..", "", "more", "components", ""]);
        }

        // `Path` -> `KnotList`: expect failure.
        {
            test!(
                path: format!(
                    "{}the{}usual{}",
                    path::MAIN_SEPARATOR,
                    path::MAIN_SEPARATOR,
                    path::MAIN_SEPARATOR
                )
            );
        }
    }
}
