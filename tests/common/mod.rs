//! Utilities shared by integration tests.

use noun::{
    serdes::{Cue, Jam},
    Atom, Noun,
};
use std::{
    env,
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

/// IO driver subprocess.
pub(crate) struct DriverProcess(pub(crate) Child);

impl Drop for DriverProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

/// Spawns an IO driver in a subprocess with piped `stdin` and `stdout`.
pub(crate) fn spawn_driver(
    driver: &'static str,
    dir: Option<&Path>,
    log_file: &Path,
) -> (DriverProcess, ChildStdin, ChildStdout) {
    // Absolute path to the binary defined by `src/main.rs`.
    const BINARY: &'static str = env!("CARGO_BIN_EXE_io_drivers");

    const LOG_VAR: &'static str = "URBIT_IO_DRIVERS_LOG";

    let cwd = env::current_dir().unwrap();
    let dir = dir.unwrap_or(&cwd);

    let mut driver = DriverProcess(
        Command::new(BINARY)
            .arg(driver)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env(LOG_VAR, log_file)
            .spawn()
            .expect("spawn io_drivers process"),
    );
    let input = driver.0.stdin.take().unwrap();
    let output = driver.0.stdout.take().unwrap();
    (driver, input, output)
}

/// Writes a request to a driver's input source.
pub(crate) fn write_request(input: &mut ChildStdin, req: Noun) {
    let req = req.jam().into_vec();

    // Write the little-endian request length.
    let len = req.len().to_le_bytes();
    input.write_all(&len[..]).expect("write request length");

    // Write the request.
    input.write_all(&req[..]).expect("write request");
    input.flush().expect("flush input");
}

/// Reads a response from a driver's output sink.
pub(crate) fn read_response(output: &mut ChildStdout) -> Noun {
    // Read the little-endian response length.
    let mut len: [u8; 8] = [0; 8];
    output
        .read_exact(&mut len[..])
        .expect("read response length");
    let len = usize::try_from(u64::from_le_bytes(len)).expect("u64 to usize");

    // Read the response.
    let mut resp = Vec::with_capacity(len);
    // Extend the length to match the capacity.
    resp.resize(resp.capacity(), 0);
    output.read_exact(&mut resp[..]).expect("read response");

    Cue::cue(Atom::from(resp)).expect("cue response")
}

/// Compares a [`Noun`] to a `u64`, returning `true` if they represent the same value and `false`
/// otherwise.
pub(crate) fn check_u64(actual: &Noun, expected: u64) -> bool {
    if let Noun::Atom(actual) = actual {
        if let Some(actual) = actual.as_u64() {
            actual == expected
        } else {
            false
        }
    } else {
        false
    }
}
