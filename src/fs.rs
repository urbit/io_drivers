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
}

impl TryFrom<&Noun> for UpdateFileSystem {
    type Error = convert::Error;

    /// Attempts to create a [`UpdateFileSystem`] request from the tail of a noun that was tagged
    /// with `%ergo`, where `%ergo` is a poor choice of tag name for an "update file system"
    /// request.
    ///
    /// A properly structured noun is a pair consisting of a mount point (`mp`) and a
    /// null-terminated list of changes (`cl`) to that mount point:
    /// ```text
    /// [mp cl]
    /// ```
    ///
    /// Each element in the list of changes is a pair consisting of a mount-point-relative path to
    /// the file being changed (represented as a null-terminated list) and a "unit" (i.e. `Option`
    /// type) detailing the change:
    ///
    /// ```text
    /// [<path_list> 0 <file_type_list> <byte_cnt> <bytes>]
    /// ```
    ///
    /// To illustrate, if `|=  a=@  +(a)` (a 13-byte change) is written to
    /// `<pier>/base/gen/example.hoon`, then the noun representing the request (assuming the tag
    /// `%ergo` tag has already been removed) is:
    /// ```text
    /// [
    ///   %base
    ///   [
    ///     [%gen %example %hoon 0]
    ///     [0 [%text %x-hoon 0] 14 0xa2961282b2020403d6120203d7c]
    ///   ]
    ///   0
    /// ]
    /// ```
    /// Note that `14` is the length of the change to `example.hoon` plus one (for the record
    /// separator i.e. ASCII `30`) and `0xa2961282b2020403d6120203d7c` is `|=  a=@  +(a)<RS>`
    /// represented as an atom (where `<RS>` is the record separator).
    ///
    /// If `<pier>/base/gen/example.hoon` is removed, then the noun representing the request is:
    /// ```text
    /// [
    ///     %base
    ///     [
    ///         [%gen %example %hoon 0]
    ///         0
    ///     ]
    ///     0
    /// ]
    /// ```
    /// Note that the "unit" detailing the change is `0`, which indicates that the file should be
    /// removed.
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

/// A file system mount point.
///
/// All mount points reside within the root directory of a ship (i.e. the pier directory).
struct MountPoint(PathBuf);

/// A file monitored by the driver.
struct File {
    /// The path to the file.
    path: PathBuf,

    /// The hash of the contents of the file after the last update.
    /// If `None`, no update has been performed yet.
    hash: Option<u64>,

    /// `true` if the file was modified since the last update.
    is_modified: bool,
}

impl File {
    /// Initializes a new [`File`].
    ///
    /// This method does not affect the underlying file system.
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            hash: None,
            is_modified: false,
        }
    }

    /// Reads the contents of a file into an atom, returning `None` if the file didn't change since
    /// the last update.
    fn read(&mut self) -> Option<io::Result<Atom>> {
        if self.hash.is_none() || self.is_modified {
            self.is_modified = false;
            let atom = match fs::read(&self.path) {
                Ok(bytes) => Atom::from(bytes),
                Err(err) => return Some(Err(err)),
            };
            let (new_hash, old_hash) = (atom.hash(), self.hash);
            if old_hash != Some(new_hash) {
                self.hash = Some(new_hash);
                Some(Ok(atom))
            } else {
                debug!(
                    "{} should have changed but the old and new hashes match",
                    self.path.display()
                );
                None
            }
        } else {
            debug!(
                "{} has not changed since the last update",
                self.path.display()
            );
            None
        }
    }

    /// Deletes a file from the file system.
    ///
    /// This is an alias for [`fs::remove_file`]`(&self.path)`.
    fn remove(self) -> io::Result<()> {
        fs::remove_file(&self.path)
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

    #[test]
    fn remove_file() {
        {
            let path = path!("what-are-the-odds-this-already-exists.txt");
            assert!(fs::File::create(&path).is_ok());
            let file = File::new(path.clone());
            file.remove().expect("remove file");
            let res = fs::File::open(&path);
            assert!(res.is_err());
            assert_eq!(res.unwrap_err().kind(), io::ErrorKind::NotFound);
        }
    }
}
