//! Tests the file system driver.
//!
//! The general pattern for each test is to launch the file system driver in a subprocess with
//! piped `stdin` and `stdout` via the crate's binary (defined in `src/main.rs`) and write file
//! system requests to the driver over the subprocess's `stdin` pipe and read responses to those
//! requests over the subprocess's `stdout` pipe.

use noun::{convert, Atom, Cell, Noun};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ChildStdin,
    thread,
    time::Duration,
};

mod common;

#[cfg(not(target_os = "windows"))]
const CWD: &'static str = "/tmp";
#[cfg(target_os = "windows")]
const CWD: &'static str = env!("TEMP");

/// Compares the contents of a change to an `expected_path` and
/// `expected_contents`, panicking if the change doesn't match `expected_path`
/// and `expected_contents`.
///
/// If a change adds/edits a file, it's of the form:
/// ```text
/// [
///   <path>
///   0
///   [[%text %plain 0] <byte_len> <bytes>]
/// ]
/// ```
///
/// if the change adds/edits a file or
///
/// If a change removes a file, it's of the form:
/// ```text
/// [<path> 0]
/// ```
fn assert_change(change: &Noun, expected_path: &[&str], expected_contents: Option<&str>) {
    if let Noun::Cell(change) = change {
        let path = convert!(change.head_ref() => Vec<&str>).expect("path to Vec");
        assert_eq!(path.len(), expected_path.len());
        for i in 0..path.len() {
            assert_eq!(path[i], expected_path[i]);
        }

        match change.tail_ref() {
            // Change removes a file.
            Noun::Atom(null) => {
                assert!(expected_contents.is_none());
                assert!(null.is_null())
            }
            // Change adds/edits a file.
            Noun::Cell(change) => {
                assert!(expected_contents.is_some());
                let expected_contents = expected_contents.unwrap();
                assert!(change.head_ref().is_null());
                if let Noun::Cell(change) = change.tail_ref() {
                    let [file_type, byte_len, bytes] =
                        change.to_array::<3>().expect("change to array");
                    let file_type = convert!(&*file_type => Vec<&str>).expect("file type to Vec");
                    assert_eq!(file_type.len(), 2);
                    assert_eq!(file_type[0], "text");
                    assert_eq!(file_type[1], "plain");
                    if let Noun::Atom(byte_len) = &*byte_len {
                        assert_eq!(
                            byte_len.as_usize().expect("byte_len to usize"),
                            expected_contents.len()
                        );
                    } else {
                        panic!("byte len is a cell");
                    }
                    if let Noun::Atom(bytes) = &*bytes {
                        assert_eq!(bytes.as_str().expect("bytes to str"), expected_contents);
                    } else {
                        panic!("bytes is a cell");
                    }
                } else {
                    panic!("change's tail's tail is an atom");
                }
            }
        }
    } else {
        panic!("change is an atom");
    }
}

/// Compares the contents of a [`File`] to a [`&str`], returning `true` if the [`File`] contents
/// and the [`&str`] are identical and `false` otherwise.
fn check_file_contents(path: &Path, expected: &str) -> bool {
    if let Ok(contents) = fs::read_to_string(path) {
        contents == expected
    } else {
        false
    }
}

/// Deletes a mount point from the file system by sending an `%ogre` request to the file systme
/// driver, returning `true` if the mount point was deleted and `false` otherwise.
fn delete_mount_point(mount_point: &str, input: &mut ChildStdin) -> bool {
    let req = Noun::from(Cell::from(["ogre", mount_point]));
    common::write_request(input, req);
    // Ensure the request gets processed before running the assertions.
    thread::sleep(Duration::from_millis(100));

    let path: PathBuf = [CWD, mount_point].iter().collect();
    !path.exists()
}

/// Sends `%dirk` requests to the file system driver.
#[test]
fn commit_mount_point() {
    let (mut driver, mut input, mut output) = common::spawn_driver(
        "fs",
        Some(Path::new(CWD)),
        Path::new("commit_mount_point.fs_tests.log"),
    );

    const MOUNT_POINT: &'static str = "garden";

    // Commit `example.txt`.
    {
        // Create `example.txt` by writing directly to the file system.
        let path: PathBuf = [CWD, MOUNT_POINT, "example.txt"].iter().collect();
        const CONTENTS: &'static str = "How strange it is to be anything at all.";
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        fs::write(&path, CONTENTS).expect("write");

        let req = Noun::from(Cell::from(["dirk", MOUNT_POINT]));
        common::write_request(&mut input, req);
        if let Noun::Cell(resp) = common::read_response(&mut output) {
            let [change, null] = resp.to_array::<2>().expect("response to array");
            assert_change(&*change, &["example", "txt"], Some(CONTENTS));
            assert!(null.is_null());
        } else {
            panic!("response is an atom");
        }
    }

    assert!(delete_mount_point(MOUNT_POINT, &mut input));
}

/// Sends `%ergo` requests to the file system driver.
#[test]
fn update_file_system() {
    let (mut driver, mut input, mut output) = common::spawn_driver(
        "fs",
        Some(Path::new(CWD)),
        Path::new("update_file_system.fs_tests.log"),
    );

    const MOUNT_POINT: &'static str = "base";

    // `gen/vats.hoon`.
    let file = convert!([&"gen", &"vats", &"hoon"].into_iter() => Noun).expect("path to Noun");

    // Create `gen/vats.hoon`.
    {
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("ergo")),
            // Mount point.
            Noun::from(Atom::from(MOUNT_POINT)),
            // Update `base/gen/vats.hoon`.
            Noun::from(Cell::from([
                file.clone(),
                Noun::null(),
                convert!([&"text", &"x-hoon"].into_iter() => Noun).expect("file type to Noun"),
                Noun::from(Atom::from(112u8)),
                Noun::from(Atom::from(vec![
                    0x2f, 0x2d, 0x20, 0x20, 0x2a, 0x68, 0x6f, 0x6f, 0x64, 0x0a, 0x3a, 0x2d, 0x20,
                    0x20, 0x25, 0x73, 0x61, 0x79, 0x0a, 0x7c, 0x3d, 0x20, 0x20, 0x24, 0x3a, 0x20,
                    0x20, 0x5b, 0x6e, 0x6f, 0x77, 0x3d, 0x40, 0x64, 0x61, 0x20, 0x65, 0x6e, 0x79,
                    0x3d, 0x40, 0x75, 0x76, 0x4a, 0x20, 0x62, 0x65, 0x63, 0x3d, 0x62, 0x65, 0x61,
                    0x6b, 0x5d, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x5b, 0x61,
                    0x72, 0x67, 0x3d, 0x7e, 0x20, 0x7e, 0x5d, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x3d,
                    0x3d, 0x0a, 0x5b, 0x25, 0x74, 0x61, 0x6e, 0x67, 0x20, 0x28, 0x72, 0x65, 0x70,
                    0x6f, 0x72, 0x74, 0x2d, 0x76, 0x61, 0x74, 0x73, 0x20, 0x70, 0x2e, 0x62, 0x65,
                    0x63, 0x20, 0x6e, 0x6f, 0x77, 0x29, 0x5d, 0xa,
                ])),
            ])),
            Noun::null(),
        ]));
        common::write_request(&mut input, req);
        // Ensure the request gets processed before running the assertions.
        thread::sleep(Duration::from_millis(100));

        let path: PathBuf = [CWD, MOUNT_POINT, "gen", "vats.hoon"].iter().collect();
        const CONTENTS: &'static str = concat!(
            "/-  *hood\n",
            ":-  %say\n",
            "|=  $:  [now=@da eny=@uvJ bec=beak]\n",
            "        [arg=~ ~]\n",
            "    ==\n",
            "[%tang (report-vats p.bec now)]\n",
        );
        assert!(check_file_contents(&path, CONTENTS));
    }

    // Delete `gen/vats.hoon`.
    {
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("ergo")),
            // Mount point.
            Noun::from(Atom::from(MOUNT_POINT)),
            // Delete `base/gen/vats.hoon`.
            Noun::from(Cell::from([file, Noun::null()])),
            Noun::null(),
        ]));
        common::write_request(&mut input, req);
        // Ensure the request gets processed before running the assertions.
        thread::sleep(Duration::from_millis(100));

        let path: PathBuf = [CWD, MOUNT_POINT, "gen", "vats.hoon"].iter().collect();
        assert!(!path.exists());
    }

    assert!(delete_mount_point(MOUNT_POINT, &mut input));
}
