#![allow(dead_code)]

use crate::atom_as_str;
use log::{info, warn};
use noun::{atom::Atom, cell::Cell, convert, marker::Atomish, Noun};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    env, fmt, fs,
    hash::Hasher,
    io,
    path::{self, Path, PathBuf},
};
use tokio::sync::mpsc::Sender;

//==================================================================================================
// Request types
//==================================================================================================

/// Requests that can be handled by the file system driver.
enum Request {
    DeleteMountPoint(DeleteMountPoint),
    ScanMountPoints(ScanMountPoints),
}

/// A request to commit a mount point.
struct CommitMountPoint {
    /// The name of the mount point to commit.
    mount_point: PathComponent,
}

/// A request to delete a mount point.
struct DeleteMountPoint {
    /// The name of the mount point to delete.
    mount_point: PathComponent,
}

/// A request to scan a list of mount points.
struct ScanMountPoints {
    /// The names of the mount points to scan.
    mount_points: Vec<PathComponent>,
}

/// A request to update the file system from a list of changes.
struct UpdateFileSystem {
    /// The name of the mount point to update.
    mount_point: PathComponent,

    /// The chnages to apply to the mount point.
    changes: Vec<Change>,
}

//==================================================================================================
// Driver
//==================================================================================================

/// The file system driver.
pub struct FileSystem {
    /// The list of actively mounted mount points.
    mount_points: HashMap<PathComponent, MountPoint>,
}

impl FileSystem {
    const fn name() -> &'static str {
        "file-system"
    }

    /// Handles a [`CommitMountPoint`] request.
    fn commit_mount_point(&mut self, req: CommitMountPoint, _output_tx: Sender<Noun>) {
        if let Some(mount_point) = self.mount_points.remove(&req.mount_point) {
            match mount_point.scan() {
                Ok((mount_point, old_entries)) => {
                    let changes = Vec::new();
                    for (path, old_hash) in &mount_point.entries {
                        match fs::read(path) {
                            Ok(bytes) => {
                                let new_hash = Hash::from(&bytes[..]);
                                if Some(&new_hash) != old_hash.as_ref() {
                                    // append cell
                                    // [
                                    //   <path>
                                    //   ~
                                    //   [[%text %plain ~] <byte_len> <bytes>]
                                    // ]
                                    // to list of changes
                                    todo!();
                                }
                            }
                            Err(err) => {
                                warn!(
                                    target: Self::name(),
                                    "failed to read {}: {}",
                                    path.display(),
                                    err
                                );
                            }
                        }
                    }

                    for (_path, _hash) in old_entries {
                        // append cell [[path ~] ~] to list of changes
                        todo!();
                    }

                    let _changes = Noun::from(Cell::from(changes));
                    self.mount_points.insert(req.mount_point, mount_point);
                }
                Err((mount_point, err)) => {
                    warn!(
                        target: Self::name(),
                        "failed to scan {}: {}",
                        mount_point.path.display(),
                        err
                    );
                    self.mount_points.insert(req.mount_point, mount_point);
                }
            }
        } else {
            info!("mount point {} is not actively mounted", req.mount_point);
        }
    }

    /// Handles a [`DeleteMountPoint`] request.
    fn delete_mount_point(&mut self, req: DeleteMountPoint) {
        if let Some(mount_point) = self.mount_points.remove(&req.mount_point) {
            let path = &mount_point.path;
            if let Err(err) = fs::remove_dir_all(path) {
                warn!(
                    target: Self::name(),
                    "failed to remove {}: {}",
                    path.display(),
                    err
                );
            }
        } else {
            info!("mount point {} is not actively mounted", req.mount_point);
        }
    }

    /// Handles a [`ScanMountPoints`] request.
    fn scan_mount_points(&mut self, req: ScanMountPoints) {
        for name in req.mount_points {
            if let Some(mount_point) = self.mount_points.remove(&name) {
                match mount_point.scan() {
                    Ok((mount_point, _old_entries)) => {
                        self.mount_points.insert(name, mount_point);
                    }
                    Err((mount_point, err)) => {
                        warn!(
                            target: Self::name(),
                            "failed to scan {}: {}",
                            mount_point.path.display(),
                            err
                        );
                        self.mount_points.insert(name, mount_point);
                    }
                }
            } else {
                info!(
                    target: Self::name(),
                    "mount point {} is not actively mounted", name
                );
            }
        }
    }

    /// Handles an [`UpdateFileSystem`] request.
    fn update_file_system(&mut self, req: UpdateFileSystem) {
        if let Some(mount_point) = self.mount_points.get_mut(&req.mount_point) {
            for change in req.changes {
                match change {
                    Change::EditFile { path, bytes } => {
                        let path: PathBuf = [&mount_point.path, &path].iter().collect();
                        let new_hash = Hash::from(&bytes[..]);
                        if let Some(Some(old_hash)) = mount_point.entries.get(&path) {
                            // Don't update the file if the hash hasn't changed.
                            if new_hash == *old_hash {
                                continue;
                            }
                        }
                        if let Err(err) = fs::write(&path, bytes) {
                            warn!(
                                target: Self::name(),
                                "failed to update {}: {}",
                                path.display(),
                                err
                            );
                        } else {
                            mount_point.entries.insert(path, Some(new_hash));
                        }
                    }
                    Change::RemoveFile { path } => {
                        let path: PathBuf = [&mount_point.path, &path].iter().collect();
                        if let Err(err) = fs::remove_file(&path) {
                            warn!(
                                target: Self::name(),
                                "failed to remove {}: {}",
                                path.display(),
                                err
                            );
                        } else {
                            mount_point.entries.remove(&path);
                        }
                    }
                }
            }
        } else {
            info!(
                target: Self::name(),
                "mount point {} is not actively mounted", req.mount_point
            );
        }
    }
}

//==================================================================================================
// Path Manipulation
//==================================================================================================

/// A single component of a file system path.
///
/// A [`PathComponent`] must only be created by converting a [`Knot`] with `try_from()`, which
/// ensures that [`Knot`]s that cause issues as file system paths are properly escaped. As a result
/// of this requirement, a [`PathComponent`] is guaranteed to never be:
/// - the empty string,
/// - `.`,
/// - `..`, or
/// - `!<some_chars>`
/// because each is escaped to yield (respectively):
/// - `!`,
/// - `!.`,
/// - `!..`, and
/// - `!!<some_chars>`.
#[derive(Eq, Hash, PartialEq)]
struct PathComponent(String);

/// Enables a [`PathComponent`] to be pushed onto a [`Path`].
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
        // A path component should not have spaces or path separators in it.
        if !knot.contains(" ") && !knot.contains(path::MAIN_SEPARATOR) {
            if knot.is_empty() || knot == "." || knot == ".." || knot.starts_with("!") {
                Ok(Self(format!("!{}", knot)))
            } else {
                Ok(Self(String::from(knot)))
            }
        } else {
            Err(convert::Error::ImplType)
        }
    }
}

/// A Hoon `$knot`.
///
/// A `$knot` is simply an ASCII string.
struct Knot<A: Atomish>(A);

impl From<PathComponent> for Knot<Atom> {
    fn from(path_component: PathComponent) -> Self {
        debug_assert!(!path_component.0.contains(path::MAIN_SEPARATOR));

        let knot = if path_component.0.chars().nth(0) == Some('!') {
            &path_component.0[1..]
        } else {
            &path_component.0[..]
        };
        Knot(Atom::from(knot))
    }
}

impl<'a> TryFrom<&'a Noun> for Knot<&'a Atom> {
    type Error = convert::Error;

    fn try_from(noun: &'a Noun) -> Result<Self, Self::Error> {
        if let Noun::Atom(atom) = noun {
            if atom_as_str(atom)?.is_ascii() {
                Ok(Self(atom))
            } else {
                Err(convert::Error::ImplType)
            }
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

/// A  list of [`Knot`]s.
///
/// A list of [`Knot`]s can take three forms:
/// - an empty list, which is interpreted as an empty file system path;
/// - a list of length 1, which is interpreted as the file system path `<file_name>`; or
/// - a list of length more than 1, which is interpreted as the file system path
///   `.../<file_name>.<file_extension>`.
///
/// Note that in the third case, `...` represents zero or more directory names and that the last two
/// elements of the list are the file name and file extension.
struct KnotList<A: Atomish>(Vec<Knot<A>>);

impl<'a> TryFrom<&'a Noun> for KnotList<&'a Atom> {
    type Error = convert::Error;

    fn try_from(noun: &'a Noun) -> Result<Self, Self::Error> {
        match noun {
            Noun::Atom(atom) => {
                if atom.is_null() {
                    Ok(Self(Vec::new()))
                } else {
                    Err(convert::Error::UnexpectedAtom)
                }
            }
            mut noun => {
                let mut knots = Vec::new();
                while let Noun::Cell(cell) = &*noun {
                    knots.push(Knot::try_from(cell.head_ref())?);
                    noun = cell.tail_ref();
                }
                // The list of knots should be null-terminated.
                if noun.is_null() {
                    Ok(Self(knots))
                } else {
                    Err(convert::Error::ImplType)
                }
            }
        }
    }
}

impl TryFrom<KnotList<&Atom>> for PathBuf {
    type Error = convert::Error;

    fn try_from(knots: KnotList<&Atom>) -> Result<Self, Self::Error> {
        match knots.0.len() {
            0 => Ok(PathBuf::new()),
            1 => {
                let mut path = PathBuf::new();
                // There's only a single knot, but this syntax for taking ownership of `knot` is
                // cleaner than alternatives.
                for knot in knots.0 {
                    path.push(PathComponent::try_from(knot)?);
                }
                Ok(path)
            }
            n => {
                let mut path = PathBuf::new();
                let mut file_name = None;
                for (i, knot) in knots.0.into_iter().enumerate() {
                    match i {
                        // `knot` is the file name.
                        m if m == n - 2 => {
                            file_name = Some(PathComponent::try_from(knot)?);
                        }
                        // `knot` is the file extension.
                        m if m == n - 1 => {
                            let file_extension = PathComponent::try_from(knot)?;
                            path.push(format!("{}.{}", file_name.take().unwrap(), file_extension));
                        }
                        // `knot` is a directory name.
                        _ => {
                            path.push(PathComponent::try_from(knot)?);
                        }
                    }
                }
                Ok(path)
            }
        }
    }
}

//==================================================================================================
// File System Entries
//==================================================================================================

/// A file system mount point.
struct MountPoint {
    /// The absolute path to the mount point.
    path: PathBuf,

    /// The file system entries that exist within the mount point.
    ///
    /// This is a map from the absolute path to a file system entry to the hash of the entry's
    /// contents.
    entries: HashMap<PathBuf, Option<Hash>>,
}

impl MountPoint {
    /// Creates a new mount point relative to the current working directory.
    fn new(name: PathComponent) -> io::Result<Self> {
        let path = {
            let mut path = env::current_dir()?;
            path.push(name);
            path
        };
        Ok(Self {
            path,
            entries: HashMap::new(),
        })
    }

    /// Scans a mount point.
    ///
    /// On success, `scan()` returns a pair consisting of the up-to-date mount point and the set of
    /// entries that were removed from the file system since the last call to `scan()`.
    ///
    /// On failure, `scan()` returns a pair consisting of the original mount point and the
    /// [`io::Error`] that prevented the mount point from being updated.
    fn scan(mut self) -> Result<(Self, HashMap<PathBuf, Option<Hash>>), (Self, io::Error)> {
        /// Recursively scans a directory, adding all discovered files to a map from absolute
        /// path to hash of the file contents.
        fn scan_dir(dir: &Path, entries: &mut HashMap<PathBuf, Option<Hash>>) -> io::Result<()> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                let file_type = entry.file_type()?;
                if file_type.is_dir() {
                    scan_dir(&path, entries)?;
                } else if file_type.is_file() && !entries.contains_key(&path) {
                    entries.insert(path, None);
                }
                // Ignore symlinks.
            }
            Ok(())
        }

        let (entries, old_entries) = self
            .entries
            .into_iter()
            .partition(|(entry, _hash)| entry.exists());

        self.entries = entries;
        if let Err(err) = scan_dir(&self.path, &mut self.entries) {
            Err((self, err))
        } else {
            Ok((self, old_entries))
        }
    }
}

/// A hash of a file system entry.
#[derive(Eq, PartialEq)]
struct Hash(u64);

impl From<&[u8]> for Hash {
    fn from(bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write(&bytes);
        Self(hasher.finish())
    }
}

/// A change to the file system.
enum Change {
    /// A change that edits a file in place.
    EditFile {
        /// Mount-point-relative path to the file.
        path: PathBuf,

        /// The new contents of the file.
        bytes: Vec<u8>,
    },

    /// A change that removes a file from the file system.
    RemoveFile {
        /// Mount-point-relative path to the file.
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use noun::{atom, cell};

    #[test]
    fn convert_knot() {
        macro_rules! test {
            // Knot -> PathComponent: expect success.
            (Knot: $knot:literal, PathComponent: $path_component:literal) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                let path_component = PathComponent::try_from(knot).expect("Knot to PathComponent");
                assert_eq!(path_component.0, $path_component);
            };
            // Knot -> PathComponent: expect failure.
            (Knot: $knot:expr) => {
                let atom = Atom::from($knot);
                let knot = Knot(&atom);
                assert!(PathComponent::try_from(knot).is_err());
            };
            // PathComponent -> Knot: expect success.
            (PathComponent: $path_component:literal, Knot: $knot:literal) => {
                let path_component = PathComponent(String::from($path_component));
                assert_eq!(Knot::from(path_component).0, $knot);
            };
        }

        {
            // Knot -> PathComponent: expect success.
            test!(Knot: "hello", PathComponent: "hello");
            test!(Knot: "goodbye!", PathComponent: "goodbye!");
            test!(Knot: "", PathComponent: "!");
            test!(Knot: ".", PathComponent: "!.");
            test!(Knot: "..", PathComponent: "!..");
            test!(Knot: "!", PathComponent: "!!");
            test!(Knot: "!water-bottle", PathComponent: "!!water-bottle");
        }

        {
            // Knot -> PathComponent: expect failure.
            test!(Knot: "this has spaces in it");
            test!(Knot: format!("{}at-the-beginning", path::MAIN_SEPARATOR));
            test!(Knot: format!("at-the-end{}", path::MAIN_SEPARATOR));
            test!(Knot: format!("in{}between", path::MAIN_SEPARATOR));
        }

        {
            // PathComponent -> Knot: expect success.
            test!(PathComponent: "goodbye", Knot: "goodbye");
            test!(PathComponent: "a_little_longer", Knot: "a_little_longer");
            test!(PathComponent: "!", Knot: "");
            test!(PathComponent: "!.", Knot: ".");
            test!(PathComponent: "!..", Knot: "..");
            test!(PathComponent: "!!double-down", Knot: "!double-down");
        }
    }

    #[test]
    fn convert_knot_list() {
        macro_rules! test {
            // Noun -> KnotList -> PathBuf: expect success.
            (Noun: $noun:expr, PathBuf: $path:literal) => {
                let knots = KnotList::try_from(&$noun).expect("Noun to KnotList");
                let path = PathBuf::try_from(knots).expect("KnotList to PathBuf");
                assert_eq!(path, Path::new($path));
            };
            // Noun -> KnotList: expect failure.
            (Noun: $noun:expr, KnotList) => {
                assert!(KnotList::try_from(&$noun).is_err());
            };
            // Noun -> KnotList -> PathBuf: expect failure.
            (Noun: $noun:expr, PathBuf) => {
                let knots = KnotList::try_from(&$noun).expect("Noun to KnotList");
                assert!(PathBuf::try_from(knots).is_err());
            };
        }

        // Noun -> KnotList -> PathBuf: expect success.
        {
            {
                let noun = Noun::from(atom!());
                test!(Noun: noun, PathBuf: "");
            }

            {
                let noun = Noun::from(cell![atom!("only-a-single-component"), atom!(),]);
                test!(Noun: noun, PathBuf: "only-a-single-component");
            }

            {
                let noun = Noun::from(cell![atom!("fs"), atom!("rs"), atom!()]);
                test!(Noun: noun, PathBuf: "fs.rs");
            }

            {
                let noun = Noun::from(cell![
                    atom!("this"),
                    atom!("is"),
                    atom!("a"),
                    atom!("path"),
                    atom!("file"),
                    atom!("extension"),
                    atom!(),
                ]);
                test!(Noun: noun, PathBuf: "this/is/a/path/file.extension");
            }

            {
                let noun = Noun::from(cell![atom!(""), atom!()]);
                test!(Noun: noun, PathBuf: "!");
            }

            {
                let noun = Noun::from(cell![atom!("."), atom!()]);
                test!(Noun: noun, PathBuf: "!.");
            }

            {
                let noun = Noun::from(cell![atom!(".."), atom!()]);
                test!(Noun: noun, PathBuf: "!..");
            }

            {
                let noun = Noun::from(cell![atom!("!"), atom!()]);
                test!(Noun: noun, PathBuf: "!!");
            }

            {
                let noun = Noun::from(cell![atom!("!escaped"), atom!()]);
                test!(Noun: noun, PathBuf: "!!escaped");
            }

            {
                let noun = Noun::from(cell![
                    atom!(".."),
                    atom!("."),
                    atom!(""),
                    atom!("!file"),
                    atom!("!extension"),
                    atom!()
                ]);
                test!(Noun: noun, PathBuf: "!../!./!/!!file.!!extension");
            }
        }

        // Noun -> KnotList: expect failure.
        {
            {
                let noun = Noun::from(atom!(107u8));
                test!(Noun: noun, KnotList);
            }

            {
                let noun = Noun::from(cell!["missing", "null", "terminator"]);
                test!(Noun: noun, KnotList);
            }
        }

        // Noun -> KnotList -> PathBuf: expect failure.
        {
            {
                let noun = Noun::from(cell![atom!("has a space"), atom!()]);
                test!(Noun: noun, PathBuf);
            }
        }
    }
}
