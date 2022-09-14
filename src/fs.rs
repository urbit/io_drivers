#![allow(dead_code)]

use log::{info, warn};
use noun::{cell::Cell, Noun};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    env, fmt, fs,
    hash::Hasher,
    io,
    path::{Path, PathBuf},
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
