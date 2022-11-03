# Urbit IO drivers

[![Cargo build](https://github.com/urbit/io_drivers/actions/workflows/cargo-build.yml/badge.svg)](https://github.com/urbit/io_drivers/actions/workflows/cargo-build.yml)
[![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE.txt)

This repository defines the interface for the [Urbit][urbit] runtime's IO
subsystem along with default implementations for that interface. The IO
subsystem consists of a collection of IO drivers, one driver per type of IO. For
more information, consult the documentation (see below).

### Build

Ensure you have an up-to-date Rust toolchain installed on your machine. If you
need Rust installation instructions, head to [rust-lang.org][rust].

To build, run:
```console
$ cargo build --release
```

If you want a debug build, run:
```console
$ cargo build
```

To build and run the test suite, run:
```console
$ cargo test
```

The test suite includes integration tests, which execute the binary defined by
`src/main.rs` in a subprocess. Logging output from the binary when executed as a
subprocess can be captured by defining the `URBIT_IO_DRIVERS_LOG` environment
variable. For example:
```console
$ URBIT_IO_DRIVERS_LOG=test.log cargo test
```
Note that each `#[test]` test defined in files in `tests/` will spawn a unique
version of the binary, which means that logging output will likely be
interleaved.

To build and view the documentation, run:
```console
$ cargo doc --open
```


[ames]: https://developers.urbit.org/reference/arvo/ames/ames
[rust]: https://www.rust-lang.org/tools/install
[urbit]: https://urbit.org
