#![allow(dead_code)]

use crate::atom_as_str;
use log::{info, warn};
use noun::{atom::Atom, cell::Cell, convert, marker::Atomish, Noun, Rc};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    env, fmt, fs,
    hash::Hasher,
    io,
    path::{self, Path, PathBuf},
};
use tokio::sync::mpsc::Sender;

//==================================================================================================
// Request Types
//==================================================================================================

/// Requests that can be handled by the file system driver.
enum Request {
    /// A request to commit a mount point.
    CommitMountPoint(CommitMountPoint),

    /// A request to delete a mount point.
    DeleteMountPoint(DeleteMountPoint),

    /// A request to scan a list of mount points.
    ScanMountPoints(ScanMountPoints),

    /// A request to update the file system from a list of changes.
    UpdateFileSystem(UpdateFileSystem),
}

impl_try_from_noun_for_request!(
    Request,
    "dirk" => CommitMountPoint,
    "ogre" => DeleteMountPoint,
    "hill" => ScanMountPoints,
    "ergo" => UpdateFileSystem,
);

/// Implements `TryFrom<&Noun>` for a struct consisting of a single field `mount_point` of type
/// [`PathComponent`] or `mount_points` of type [`Vec<PathComponent>`].
macro_rules! impl_try_from_noun {
    // Implements `TryFrom<&Noun>` for a struct with a single field named `mount_point` of type
    // `PathComponent`. A properly structured noun is:
    //
    // ```text
    // <mount_point>
    // ```
    //
    // where `<mount_point>` is a mount point name.
    ($type:ty { mount_point }) => {
        impl TryFrom<&Noun> for $type {
            type Error = convert::Error;

            fn try_from(data: &Noun) -> Result<Self, Self::Error> {
                let knot = Knot::try_from(data)?;
                let mount_point = PathComponent::try_from(knot)?;
                Ok(Self { mount_point })
            }
        }
    };
    // Implements `TryFrom<&Noun>` for a struct with a single field named `mount_points` of type
    // `Vec<PathComponent>`. A properly structured noun is:
    //
    // ```text
    // <mount_point_list>
    // ```
    //
    // where `<mount_point_list>` is a null-terminated list of mount point names.
    ($type:ty { mount_points }) => {
        impl TryFrom<&Noun> for $type {
            type Error = convert::Error;
            fn try_from(data: &Noun) -> Result<Self, Self::Error> {
                let knots = KnotList::try_from(data)?;
                let mut mount_points = Vec::new();
                for knot in knots.0 {
                    mount_points.push(PathComponent::try_from(knot)?);
                }
                Ok(Self { mount_points })
            }
        }
    };
}

/// A request to commit a mount point.
struct CommitMountPoint {
    /// The name of the mount point to commit.
    mount_point: PathComponent,
}

impl_try_from_noun!(CommitMountPoint { mount_point });

/// A request to delete a mount point.
struct DeleteMountPoint {
    /// The name of the mount point to delete.
    mount_point: PathComponent,
}

impl_try_from_noun!(DeleteMountPoint { mount_point });

/// A request to scan a list of mount points.
struct ScanMountPoints {
    /// The names of the mount points to scan.
    mount_points: Vec<PathComponent>,
}

impl_try_from_noun!(ScanMountPoints { mount_points });

/// A request to update the file system from a list of changes.
struct UpdateFileSystem {
    /// The name of the mount point to update.
    mount_point: PathComponent,

    /// The chnages to apply to the mount point.
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
    /// where `<mount_point>` is the name of the mount point and `<change_list>` is a
    /// null-terminated list of changes to make to the file system. See [`Change`] for the
    /// structure of a single change.
    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(data) = data {
            let knot = Knot::try_from(data.head_ref())?;
            let mount_point = PathComponent::try_from(knot)?;
            let changes = ChangeList::try_from(data.tail_ref())?.0;
            Ok(Self {
                mount_point,
                changes,
            })
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
                Ok((mut mount_point, old_entries)) => {
                    let mut changes = Vec::new();
                    let null = Rc::new(Noun::null());
                    for (path, old_hash) in &mut mount_point.entries {
                        match fs::read(path) {
                            Ok(bytes) => {
                                let new_hash = Hash::from(&bytes[..]);
                                // If the hash didn't change, skip this entry.
                                if Some(&new_hash) != old_hash.as_ref() {
                                    match path.strip_prefix(&mount_point.path) {
                                        // Append
                                        //
                                        // [
                                        //   <path>
                                        //   0
                                        //   [[%text %plain 0] <byte_len> <bytes>]
                                        // ]
                                        //
                                        // to the list of changes.
                                        Ok(path) => {
                                            if let Ok(path) = KnotList::try_from(path) {
                                                let change = Cell::from([
                                                    Rc::<Noun>::from(Noun::from(path)),
                                                    null.clone(),
                                                    Rc::<Noun>::from(Cell::from([
                                                        Noun::from(Cell::from([
                                                            Atom::from("text"),
                                                            Atom::from("plain"),
                                                            Atom::null(),
                                                        ])),
                                                        Noun::from(Atom::from(bytes.len())),
                                                        Noun::from(Atom::from(bytes)),
                                                    ])),
                                                ]);
                                                changes.push(change);
                                                // TODO: verify this does what's expected.
                                                *old_hash = Some(new_hash);
                                            } else {
                                                warn!(
                                                    target: Self::name(),
                                                    "failed to convert {} into a list of knots",
                                                    path.display()
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            warn!(
                                                target: Self::name(),
                                                "failed to strip {} from {}: {}",
                                                mount_point.path.display(),
                                                path.display(),
                                                err
                                            );
                                        }
                                    }
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

                    for (path, _hash) in old_entries {
                        match path.strip_prefix(&mount_point.path) {
                            // Append [<path> 0] to the list of changes.
                            Ok(path) => {
                                if let Ok(path) = KnotList::try_from(path) {
                                    let path = Noun::from(path);
                                    let change = Cell::from([Rc::<Noun>::from(path), null.clone()]);
                                    changes.push(change);
                                } else {
                                    warn!(
                                        target: Self::name(),
                                        "failed to convert {} into a list of knots",
                                        path.display()
                                    );
                                }
                            }
                            Err(err) => {
                                warn!(
                                    target: Self::name(),
                                    "failed to strip {} from {}: {}",
                                    mount_point.path.display(),
                                    path.display(),
                                    err
                                );
                            }
                        }
                    }

                    let _changes = convert!(changes.into_iter() => Noun);
                    // TODO: send changes over _output_tx
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
#[derive(Debug, Eq, Hash, PartialEq)]
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

impl From<Knot<Atom>> for Noun {
    fn from(knot: Knot<Atom>) -> Self {
        Self::from(knot.0)
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

impl TryFrom<&Path> for KnotList<Atom> {
    type Error = ();

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let mut knots = Vec::new();
        if let Some(parent) = path.parent() {
            for dir in parent.components() {
                let dir = Atom::try_from(dir.as_os_str())?;
                knots.push(Knot(dir));
            }
        }
        if let Some(file_stem) = path.file_stem() {
            let file_stem = Atom::try_from(file_stem)?;
            knots.push(Knot(file_stem));
        }
        if let Some(file_extension) = path.extension() {
            let file_extension = Atom::try_from(file_extension)?;
            knots.push(Knot(file_extension));
        }
        Ok(Self(knots))
    }
}

impl From<KnotList<Atom>> for Noun {
    fn from(knots: KnotList<Atom>) -> Self {
        // This is safe to unwrap because the conversion from `Knot<Atom>` to `Noun` will never
        // fail.
        convert!(knots.0.into_iter() => Noun).unwrap()
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
///
/// TODO: handle single file mount points.
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
#[derive(Debug, Eq, PartialEq)]
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
    /// `<path_list>` is a null-terminated list identifying the mount-point-relative path to a
    /// file.
    ///
    /// As a concrete example, writing `|=  a=@  +(a)` (a 13-byte change) to
    /// `<pier>/base/gen/example.hoon` yields:
    ///
    /// ```text
    /// [
    ///     [%gen %example %hoon 0]
    ///     0
    ///     [%text %x-hoon 0]
    ///     14
    ///     0xa2961282b2020403d6120203d7c
    /// ]
    /// ```
    ///
    /// Note that `14` is the length of the chnage to `example.hoon` plus one (for the record
    /// separator i.e. ASCII `30`) and `0xa2961282b2020403d6120203d7c` is `|=  a=@  +(a)<RS>`
    /// represented as an atom (where `<RS>` is the record separator).
    ///
    /// Removing `<pier>/base/gen/example.hoon` yields:
    ///
    /// ```text
    /// [
    ///     [%gen %example %hoon 0]
    ///     0
    /// ]
    /// ```
    fn try_from(noun: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(noun) = noun {
            let path = PathBuf::try_from(KnotList::try_from(noun.head_ref())?)?;
            match noun.tail_ref() {
                Noun::Atom(tail) => {
                    if tail.is_null() {
                        Ok(Self::RemoveFile { path })
                    } else {
                        Err(convert::Error::ExpectedNull)
                    }
                }
                Noun::Cell(tail) => {
                    let [null, _file_type_list, byte_len, bytes] =
                        tail.to_array::<4>().ok_or(convert::Error::ImplType)?;
                    if null.is_null() {
                        if let Noun::Atom(byte_len) = &*byte_len {
                            if let Noun::Atom(bytes) = &*bytes {
                                let bytes = bytes.to_vec();
                                debug_assert_eq!(
                                    byte_len.as_usize().expect("Atom to usize"),
                                    bytes.len()
                                );
                                Ok(Self::EditFile { path, bytes })
                            } else {
                                Err(convert::Error::UnexpectedCell)
                            }
                        } else {
                            Err(convert::Error::UnexpectedCell)
                        }
                    } else {
                        Err(convert::Error::ExpectedNull)
                    }
                }
            }
        } else {
            Err(convert::Error::UnexpectedAtom)
        }
    }
}

/// A list of changes to the file system.
struct ChangeList(Vec<Change>);

impl TryFrom<&Noun> for ChangeList {
    type Error = convert::Error;

    fn try_from(noun: &Noun) -> Result<Self, Self::Error> {
        Ok(Self(convert!(noun => Vec<Change>)?))
    }
}

//==================================================================================================
// Tests
//==================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_noun_to_mount_point {
        ($type:ty) => {
            macro_rules! test {
                // Noun -> $type: expect success.
                (Noun: $atom:expr, PathComponent: $path_component:literal) => {
                    let noun = Noun::from($atom);
                    let req = <$type>::try_from(&noun).expect("Noun to $type");
                    assert_eq!(
                        req.mount_point,
                        PathComponent(String::from($path_component))
                    );
                };
                // Noun -> $type: expect failure.
                (Noun: $noun:expr) => {
                    let noun = Noun::from($noun);
                    assert!(<$type>::try_from(&noun).is_err());
                };
            }

            // Noun -> $type: expect success.
            {
                test!(Noun: Atom::from("mount-point-name"), PathComponent: "mount-point-name");
                test!(Noun: Atom::from(""), PathComponent: "!");
                test!(Noun: Atom::from("."), PathComponent: "!.");
                test!(Noun: Atom::from(".."), PathComponent: "!..");
                test!(Noun: Atom::from("!base"), PathComponent: "!!base");
            }

            // Noun -> $type: expect failure.
            {
                test!(Noun: Atom::from(" "));
                test!(Noun: Atom::from(format!("has{}separator", path::MAIN_SEPARATOR)));
                test!(Noun: Cell::from([Atom::from("mount-point"), Atom::null()]));
            }
        };
    }

    #[test]
    fn convert_change() {
        // Noun -> Change: expect success.
        {
            {
                let noun = Noun::from(Cell::from([
                    Noun::from(Cell::from([
                        Atom::from("gen"),
                        Atom::from("example"),
                        Atom::from("hoon"),
                        Atom::null(),
                    ])),
                    Noun::null(),
                ]));
                let change = Change::try_from(&noun).expect("Noun to Change");
                assert_eq!(
                    change,
                    Change::RemoveFile {
                        path: PathBuf::from("gen/example.hoon")
                    }
                );
            }

            {
                let noun = Noun::from(Cell::from([
                    Noun::from(Cell::from([
                        Atom::from("gen"),
                        Atom::from("example"),
                        Atom::from("hoon"),
                        Atom::null(),
                    ])),
                    Noun::null(),
                    Noun::from(Cell::from([
                        Atom::from("text"),
                        Atom::from("x-hoon"),
                        Atom::null(),
                    ])),
                    Noun::from(Atom::from(14u8)),
                    Noun::from(Atom::from(0xa2961282b2020403d6120203d7cu128)),
                ]));
                let change = Change::try_from(&noun).expect("Noun to Change");
                assert_eq!(
                    change,
                    Change::EditFile {
                        path: PathBuf::from("gen/example.hoon"),
                        bytes: Atom::from(0xa2961282b2020403d6120203d7cu128).into_vec(),
                    }
                );
            }
        }
    }

    #[test]
    fn convert_commit_mount_point_request() {
        test_noun_to_mount_point!(CommitMountPoint);
    }

    #[test]
    fn convert_delete_mount_point_request() {
        test_noun_to_mount_point!(DeleteMountPoint);
    }

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
                let noun = Noun::from(Atom::null());
                test!(Noun: noun, PathBuf: "");
            }

            {
                let noun = Noun::from(Cell::from([
                    Atom::from("only-a-single-component"),
                    Atom::null(),
                ]));
                test!(Noun: noun, PathBuf: "only-a-single-component");
            }

            {
                let noun = Noun::from(Cell::from([
                    Atom::from("fs"),
                    Atom::from("rs"),
                    Atom::null(),
                ]));
                test!(Noun: noun, PathBuf: "fs.rs");
            }

            {
                let noun = Noun::from(Cell::from([
                    Atom::from("this"),
                    Atom::from("is"),
                    Atom::from("a"),
                    Atom::from("path"),
                    Atom::from("file"),
                    Atom::from("extension"),
                    Atom::null(),
                ]));
                test!(Noun: noun, PathBuf: "this/is/a/path/file.extension");
            }

            {
                let noun = Noun::from(Cell::from([Atom::from(""), Atom::null()]));
                test!(Noun: noun, PathBuf: "!");
            }

            {
                let noun = Noun::from(Cell::from([Atom::from("."), Atom::null()]));
                test!(Noun: noun, PathBuf: "!.");
            }

            {
                let noun = Noun::from(Cell::from([Atom::from(".."), Atom::null()]));
                test!(Noun: noun, PathBuf: "!..");
            }

            {
                let noun = Noun::from(Cell::from([Atom::from("!"), Atom::null()]));
                test!(Noun: noun, PathBuf: "!!");
            }

            {
                let noun = Noun::from(Cell::from([Atom::from("!escaped"), Atom::null()]));
                test!(Noun: noun, PathBuf: "!!escaped");
            }

            {
                let noun = Noun::from(Cell::from([
                    Atom::from(".."),
                    Atom::from("."),
                    Atom::from(""),
                    Atom::from("!file"),
                    Atom::from("!extension"),
                    Atom::null(),
                ]));
                test!(Noun: noun, PathBuf: "!../!./!/!!file.!!extension");
            }
        }

        // Noun -> KnotList: expect failure.
        {
            {
                let noun = Noun::from(Atom::from(107u8));
                test!(Noun: noun, KnotList);
            }

            {
                let noun = Noun::from(Cell::from(["missing", "null", "terminator"]));
                test!(Noun: noun, KnotList);
            }
        }

        // Noun -> KnotList -> PathBuf: expect failure.
        {
            {
                let noun = Noun::from(Cell::from([Atom::from("has a space"), Atom::null()]));
                test!(Noun: noun, PathBuf);
            }
        }
    }

    #[test]
    fn convert_scan_mount_points_request() {
        macro_rules! test {
            // Noun -> ScanMountPoints: expect success.
            (Noun: $noun:expr, Vec<PathComponent>: $path_components:expr) => {
                let noun = Noun::from($noun);
                let req = ScanMountPoints::try_from(&noun).expect("Noun to ScanMountPoints");
                assert_eq!(req.mount_points, $path_components);
            };
            // Noun -> ScanMountPoints: expect failure.
            (Noun: $noun:expr) => {
                let noun = Noun::from($noun);
                assert!(ScanMountPoints::try_from(&noun).is_err());
            };
        }

        // Noun -> ScanMountPoints: expect success.
        {
            test!(Noun: Atom::null(), Vec<PathComponent>: vec![]);

            {
                let cell = Cell::from([
                    Atom::from("a"),
                    Atom::from("b"),
                    Atom::from("c"),
                    Atom::null(),
                ]);
                let path_components = vec![
                    PathComponent(String::from("a")),
                    PathComponent(String::from("b")),
                    PathComponent(String::from("c")),
                ];
                test!(Noun: cell, Vec<PathComponent>: path_components);
            }
        }

        // Noun -> ScanMountPoints: expect failure.
        {
            {
                let cell = Cell::from([
                    Noun::from(Cell::from(["unexpected", "cell"])),
                    Noun::from(Atom::null()),
                ]);
                test!(Noun: cell);
            }

            {
                let cell = Cell::from([
                    Atom::from("missing"),
                    Atom::from("null"),
                    Atom::from("terminator"),
                ]);
                test!(Noun: cell);
            }
        }
    }

    #[test]
    fn convert_update_file_system_request() {
        // Noun -> UpdateFileSystem: expect success.
        {
            {
                let noun = Noun::from(Cell::from([
                    Noun::from(Atom::from("mount-point")),
                    Noun::from(Cell::from([
                        Noun::from(Cell::from([
                            Atom::from("gen"),
                            Atom::from("foo"),
                            Atom::from("hoon"),
                            Atom::null(),
                        ])),
                        Noun::null(),
                    ])),
                    Noun::from(Cell::from([
                        Noun::from(Cell::from([
                            Atom::from("gen"),
                            Atom::from("bar"),
                            Atom::from("hoon"),
                            Atom::null(),
                        ])),
                        Noun::null(),
                        Noun::from(Cell::from([
                            Atom::from("text"),
                            Atom::from("x-hoon"),
                            Atom::null(),
                        ])),
                        Noun::from(Atom::from(14u8)),
                        Noun::from(Atom::from(0xa2961282b2020403d6120203d7cu128)),
                    ])),
                    Noun::null(),
                ]));
                let req = UpdateFileSystem::try_from(&noun).expect("Noun to UpdateFileSystem");
                assert_eq!(req.mount_point, PathComponent(String::from("mount-point")));
                assert_eq!(req.changes.len(), 2);
                assert_eq!(
                    req.changes[0],
                    Change::RemoveFile {
                        path: PathBuf::from("gen/foo.hoon")
                    }
                );
                assert_eq!(
                    req.changes[1],
                    Change::EditFile {
                        path: PathBuf::from("gen/bar.hoon"),
                        bytes: Atom::from(0xa2961282b2020403d6120203d7cu128).into_vec()
                    }
                );
            }
        }
    }
}
