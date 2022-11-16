//! Tests the file system driver.
//!
//! The general pattern for each test is to launch the file system driver in a subprocess with
//! piped `stdin` and `stdout` via the crate's binary (defined in `src/main.rs`) and write file
//! system requests to the driver over the subprocess's `stdin` pipe and read responses to those
//! requests over the subprocess's `stdout` pipe.

use noun::{Atom, Cell, Noun};
use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

mod common;

#[cfg(not(target_os = "windows"))]
const CWD: &'static str = "/tmp";
#[cfg(target_os = "windows")]
const CWD: &'static str = env!("TEMP");

/// Compares the contents of a [`File`] to a [`&str`], returning `true` if the [`File`] contents
/// and the [`&str`] are identical and `false` otherwise.
fn check_file_contents(path: &Path, expected: &str) -> bool {
    if let Ok(contents) = fs::read_to_string(path) {
        contents == expected
    } else {
        false
    }
}

/// Sends `%ergo` requests to the file system driver.
#[test]
fn update_file_system() {
    let mut driver = common::spawn_driver(
        "fs",
        Some(Path::new(CWD)),
        Path::new("update_file_system.fs_tests.log"),
    );

    let mut input = driver.0.stdin.take().unwrap();
    let mut output = driver.0.stdout.take().unwrap();

    const TAG: &'static str = "ergo";
    const MOUNT_POINT: &'static str = "base";

    // Create `gen/vats.hoon`.
    {
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from(TAG)),
            // Mount point.
            Noun::from(Atom::from(MOUNT_POINT)),
            // Update `base/gen/vats.hoon`.
            Noun::from(Cell::from([
                Noun::from(Cell::from([
                    Noun::from(Atom::from("gen")),
                    Noun::from(Atom::from("vats")),
                    Noun::from(Atom::from("hoon")),
                    Noun::null(),
                ])),
                Noun::null(),
                Noun::from(Cell::from([
                    Noun::from(Atom::from("text")),
                    Noun::from(Atom::from("x-hoon")),
                    Noun::null(),
                ])),
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
            Noun::from(Atom::from(TAG)),
            // Mount point.
            Noun::from(Atom::from(MOUNT_POINT)),
            // Delete `base/gen/vats.hoon`.
            Noun::from(Cell::from([
                Noun::from(Cell::from([
                    Noun::from(Atom::from("gen")),
                    Noun::from(Atom::from("vats")),
                    Noun::from(Atom::from("hoon")),
                    Noun::null(),
                ])),
                Noun::null(),
            ])),
            Noun::null(),
        ]));
        common::write_request(&mut input, req);
        // Ensure the request gets processed before running the assertions.
        thread::sleep(Duration::from_millis(100));

        let path: PathBuf = [CWD, MOUNT_POINT, "gen", "vats.hoon"].iter().collect();
        assert!(!path.exists());
    }
}
