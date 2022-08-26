use crate::{atom_as_str, Driver, Status};
use log::{debug, info, warn};
use noun::{
    atom::Atom,
    cell::Cell,
    convert,
    marker::{Atomish, Cellish},
    Noun, Rc,
};
use std::{
    collections::HashMap,
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
    CommitMountPoint(CommitMountPoint),
    DeleteMountPoint(DeleteMountPoint),
    ScanMountPoints(ScanMountPoints),
}

impl TryFrom<Noun> for Request {
    type Error = convert::Error;

    fn try_from(req: Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(req) = req {
            let (tag, data) = req.into_parts();
            if let Noun::Atom(tag) = &*tag {
                // These tag names are terrible, but we unfortunatley can't do anything about
                // it here because they're determined by the kernel.
                match atom_as_str(tag)? {
                    "ergo" => Ok(Self::UpdateFileSystem(UpdateFileSystem::try_from(&*data)?)),
                    "hill" => Ok(Self::ScanMountPoints(ScanMountPoints::try_from(&*data)?)),
                    "ogre" => Ok(Self::DeleteMountPoint(DeleteMountPoint::try_from(&*data)?)),
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
    /// The mount point to update.
    mount_point: PathComponent,
}

impl TryFrom<&Noun> for UpdateFileSystem {
    type Error = convert::Error;

    /// Attempts to create a [`UpdateFileSystem`] request from the tail of a noun that was tagged
    /// with `%ergo`, where `%ergo` is a poor choice of tag name for an "update file system"
    /// request.
    ///
    /// A properly structured noun is TODO.
    /// ```text
    ///   .
    ///  / \
    /// mp  TODO
    /// ```
    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(data) = &*data {
            if let Noun::Atom(knot) = &*data.head() {
                let mount_point = PathComponent::try_from(Knot(knot))?;
                // data.tail() is `can`, which is a null-terminated list of pairs
                // each pair appears to [<path within mount point> <file type>]
                Ok(Self { mount_point })
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
struct DeleteMountPoint {
    /// The mount point to delete.
    mount_point: PathComponent,
}

impl TryFrom<&Noun> for DeleteMountPoint {
    type Error = convert::Error;

    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Atom(knot) = &*data {
            let mount_point = PathComponent::try_from(Knot(knot))?;
            Ok(Self { mount_point })
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

/// A request to scan a list of mount points.
struct ScanMountPoints {
    /// The names of the mount points to scan.
    mount_points: Vec<PathComponent>,
}

impl TryFrom<&Noun> for ScanMountPoints {
    type Error = convert::Error;

    /// Attempts to create a [`ScanMountPoints`] request from the tail of a noun that was tagged
    /// with `%hill`, where `%hill` is a poor choice of tag name for a "scan mount points" request.
    ///
    /// A properly structured noun is a null-terminated list of mount points, each of which is an
    /// atom. For example:
    /// ```text
    ///    .
    ///   / \
    /// mp   .
    ///     / \
    ///    mp  .
    ///       / \
    ///      mp  0
    /// ```
    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        let mut mount_points = Vec::new();
        if let Noun::Cell(data) = data {
            let data = data.to_vec();
            // Skip the null terminator at the end of the list.
            for knot in &data[0..data.len() - 1] {
                if let Noun::Atom(knot) = &**knot {
                    let mount_point = PathComponent::try_from(Knot(knot))?;
                    mount_points.push(mount_point);
                } else {
                    return Err(convert::Error::UnexpectedCell);
                }
            }
        }
        Ok(Self { mount_points })
    }
}

//==================================================================================================
// Driver
//==================================================================================================

/// The file system driver.
pub struct FileSystem {
    /// The root of file system tree managed by the driver.
    ///
    /// This is the pier directory.
    root_dir: PathBuf,

    /// A map from mount point name to mount point.
    mount_points: HashMap<PathComponent, MountPoint>,
}

impl FileSystem {
    fn update_file_system(&self, _req: UpdateFileSystem) {
        todo!()
    }

    fn commit_mount_point(&self, _req: CommitMountPoint) {
        todo!()
    }

    fn delete_mount_point(&mut self, req: DeleteMountPoint) {
        // TODO: explain how implicit `drop()` calls delete the mount point.
        if let Some(_mount_point) = self.mount_points.remove(&req.mount_point) {
            info!(
                target: Self::name(),
                "deleting mount point %{}", req.mount_point
            );
        } else {
            warn!(
                target: Self::name(),
                "mount point %{} is not in the active set of mount points", req.mount_point
            );
        }
    }

    fn scan_mount_points(&mut self, req: ScanMountPoints) {
        for mount_point_name in req.mount_points {
            if !self.mount_points.contains_key(&mount_point_name) {
                match MountPoint::new(mount_point_name.clone(), &mut self.root_dir) {
                    Ok(mount_point) => {
                        self.mount_points.insert(mount_point_name, mount_point);
                    }
                    Err(err) => {
                        warn!(
                            target: Self::name(),
                            "failed to scan %{} mount point: {}", mount_point_name, err
                        );
                    }
                }
            }
        }
    }
}

/// Implements the [`Driver`] trait for the [`FileSystem`] driver.
macro_rules! impl_driver {
    ($input_src:ty, $output_sink:ty) => {
        impl Driver<$input_src, $output_sink> for FileSystem {
            fn new() -> Result<Self, Status> {
                Ok(Self {
                    root_dir: todo!(),
                    mount_points: HashMap::new(),
                })
            }

            fn name() -> &'static str {
                "file-system"
            }

            fn handle_requests(
                mut self,
                mut input_rx: Receiver<Noun>,
                _output_tx: Sender<Noun>,
            ) -> JoinHandle<Status> {
                let task = tokio::spawn(async move {
                    while let Some(req) = input_rx.recv().await {
                        match Request::try_from(req) {
                            Ok(Request::UpdateFileSystem(req)) => self.update_file_system(req),
                            Ok(Request::CommitMountPoint(req)) => self.commit_mount_point(req),
                            Ok(Request::DeleteMountPoint(req)) => self.delete_mount_point(req),
                            Ok(Request::ScanMountPoints(req)) => self.scan_mount_points(req),
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

/// A list of `$knot`.
struct KnotList<C: Cellish>(C);

/// Attempts to create a [`KnotList`] from a [`Path`].
impl TryFrom<&Path> for KnotList<Cell> {
    type Error = ();

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let mut path_components = Vec::new();
        for path_component in path.components() {
            let path_component =
                PathComponent(path_component.as_os_str().to_str().ok_or(())?.to_string());
            let knot = Knot::try_from(path_component)?;
            path_components.push(Rc::<Noun>::from(knot.0));
        }
        // TODO: determine if `Atom::null()` should be pushed onto `path_components`.
        Ok(KnotList(Cell::from(path_components)))
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

/// Attempts to create a [`PathComponent`] from an [`Knot`].
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

/// Attempts to create a [`PathComponent`] from an [`&OsStr`].
impl TryFrom<&OsStr> for PathComponent {
    type Error = ();

    fn try_from(os_str: &OsStr) -> Result<Self, Self::Error> {
        match os_str.to_str() {
            Some(".") | Some("..") | None => Err(()),
            Some(string) => Ok(Self(string.to_string())),
        }
    }
}

/// Attempts to create a [`PathBuf`] from a [`KnotList`].
impl TryFrom<KnotList<&Cell>> for PathBuf {
    type Error = convert::Error;

    fn try_from(knot_list: KnotList<&Cell>) -> Result<Self, Self::Error> {
        let mut path = PathBuf::new();
        // TODO: determine if is `knot_list` null-terminated.
        for knot in knot_list.0.to_vec() {
            if let Noun::Atom(knot) = &*knot {
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

/// Collects the entries of a directory that are valid [`PathComponent`]s.
///
/// `.` and `..` are omitted from the map of returned entries because they are not valid
/// [`PathComponent`]s.
fn read_dir(path: &Path) -> io::Result<HashMap<PathComponent, Entry>> {
    let mut entries = HashMap::new();
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if let Some(name) = path.file_name() {
            if let Ok(name) = PathComponent::try_from(name) {
                let entry = if path.is_dir() {
                    Entry::Directory(Directory {
                        name: name.clone(),
                        children: HashMap::new(),
                    })
                } else if path.is_file() {
                    Entry::File(File { name: name.clone() })
                } else if path.is_symlink() {
                    todo!()
                } else {
                    continue;
                };
                entries.insert(name, entry);
            }
        }
    }
    Ok(entries)
}

/// A file system mount point.
///
/// All mount points reside within the root directory of a ship (i.e. the pier directory).
struct MountPoint {
    /// The name of the mount point.
    name: PathComponent,

    /// The topmost files and directories within the mount point.
    children: Option<HashMap<PathComponent, Entry>>,
}

impl MountPoint {
    /// Creates a new mount point.
    fn new(name: PathComponent, parent_dir: &mut PathBuf) -> io::Result<Self> {
        let path = parent_dir;
        path.push(&name);
        let res = if path.is_dir() {
            Ok(Self {
                name,
                children: None,
            })
        } else if path.is_file() {
            todo!()
        } else if path.is_symlink() {
            todo!()
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("cannot determine file type of {}", name),
            ))
        };
        path.pop();
        res
    }
}

/// A file system entry monitored by the driver.
enum Entry {
    Directory(Directory),
    File(File),
}

/// A directory monitored by the driver.
struct Directory {
    /// The name of the directory.
    name: PathComponent,

    /// The files and directories within the directory.
    children: HashMap<PathComponent, Entry>,
}

/// A file monitored by the driver.
struct File {
    /// The name of the file.
    name: PathComponent,
}

#[cfg(test)]
mod tests {
    use super::*;

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

        {
            test!(knot_list: ["hello", "goodbye"], path: "hello/goodbye");
            test!(knot_list: ["some", ".", "path"], path: "some/!./path");
            test!(knot_list: ["..", "!", "", "jian3", "fei2"], path: "!../!!/!/jian3/fei2");
        }

        {
            test!(knot_list: [&format!("{}uh-oh", path::MAIN_SEPARATOR), "gan4ma2"]);
        }

        {
            test!(path: "la/dee/da", knot_list: ["la", "dee", "da"]);
            test!(path: "some/!!escaped/path", knot_list: ["some", "!escaped", "path"]);
            test!(path: "!./!../!/more/components", knot_list: [".", "..", "", "more", "components"]);
        }

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
