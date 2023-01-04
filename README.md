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
subprocess is captured in `tests/<test_fn_name>.<test_file_name>.log`. For
example, the logging output from the binary when running the `send_request()`
test in `tests/http_client_tests.rs` ends up in
`tests/send_request.http_client_tests.log`.

To build and view the documentation, run:
```console
$ cargo doc --open
```


[ames]: https://developers.urbit.org/reference/arvo/ames/ames
[rust]: https://www.rust-lang.org/tools/install
[urbit]: https://urbit.org
