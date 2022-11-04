//! Tests the HTTP client driver.
//!
//! The general pattern for each test is to launch the HTTP client driver in a subprocess with piped
//! `stdin` and `stdout` via the crate's binary (defined in `src/main.rs`) and write HTTP client
//! requests to the driver over the subprocess's `stdin` pipe and read responses to those requests
//! over the subprocess's `stdout` pipe.

use noun::{convert, Atom, Cell, Noun};
use std::{path::Path, sync::mpsc, thread, time::Duration};

mod common;

/// Sends `%request` requests to the HTTP client driver.
#[test]
fn send_request() {
    let mut driver = common::spawn_driver(
        "http-client",
        Path::new("send_request.http_client_tests.log"),
    );

    let mut input = driver.0.stdin.take().unwrap();
    let mut output = driver.0.stdout.take().unwrap();

    // This HTTP request can be replicated from the command line:
    //
    // ```
    // $ curl -i -X GET https://archlinux.org
    // ```
    {
        let req_num = 87714;
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("request")),
            // Request number.
            Noun::from(Atom::from(req_num)),
            // HTTP method.
            Noun::from(Atom::from("GET")),
            // HTTP URI.
            Noun::from(Atom::from("https://archlinux.org")),
            // HTTP headers.
            Noun::null(),
            // HTTP body.
            Noun::null(),
        ]));

        common::write_request(&mut input, req);
        if let Noun::Cell(resp) = common::read_response(&mut output) {
            let [num, status, headers, _body] = resp.to_array::<4>().expect("response to array");
            assert!(common::check_u64(&num, req_num));
            assert!(common::check_u64(&status, 200));

            let headers = convert!(&*headers => HashMap<&str, &str>).expect("headers to HashMap");

            // We can't check the value of these headers because they aren't deterministic.
            assert!(headers.contains_key("cache-control"));
            assert!(headers.contains_key("content-length"));
            assert!(headers.contains_key("date"));
            assert!(headers.contains_key("strict-transport-security"));
            assert!(headers.contains_key("vary"));
            assert!(headers.contains_key("x-frame-options"));

            assert_eq!(
                headers.get("content-security-policy"),
                Some(&"form-action 'self'; script-src 'self'; img-src 'self' data:; default-src 'self'; base-uri 'none'; frame-ancestors 'none'"),
            );
            assert_eq!(
                headers.get("content-type"),
                Some(&"text/html; charset=utf-8")
            );
            assert_eq!(
                headers.get("cross-origin-opener-policy"),
                Some(&"same-origin")
            );
            assert_eq!(headers.get("referrer-policy"), Some(&"strict-origin"));
            assert_eq!(headers.get("server"), Some(&"nginx"));
            assert_eq!(headers.get("x-content-type-options"), Some(&"nosniff"));
        } else {
            panic!("response is an atom");
        }
    }

    // This HTTP request can be replicated from the command line:
    //
    // ```
    // $ curl -i -X POST -H 'Content-Type: application/json' \
    //      -d '[{"params":["0x1cb206cf43349cd6569b74aea264b3301d388aa19b083094b09ba428f925d1a5"],"id":"tx by hash","jsonrpc":"2.0","method":"eth_getTransactionByHash"}]' \
    //      http://eth-mainnet.urbit.org:85450
    // ```
    {
        let req_num = 62;
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("request")),
            // Request number.
            Noun::from(Atom::from(req_num)),
            // HTTP method.
            Noun::from(Atom::from("POST")),
            // HTTP URI.
            Noun::from(Atom::from("http://eth-mainnet.urbit.org:8545")),
            // HTTP headers.
            Noun::from(Cell::from([
                Noun::from(Cell::from(["Content-Type", "application/json"])),
                Noun::null(),
            ])),
            // HTTP body.
            Noun::from(Cell::from([
                Noun::null(),
                Noun::from(Atom::from(153u8)),
                Noun::from(Atom::from(
                    r#"[{"params":["0x1cb206cf43349cd6569b74aea264b3301d388aa19b083094b09ba428f925d1a5"],"id":"tx by hash","jsonrpc":"2.0","method":"eth_getTransactionByHash"}]"#,
                )),
            ])),
        ]));

        common::write_request(&mut input, req);
        if let Noun::Cell(resp) = common::read_response(&mut output) {
            let [num, status, headers, body] = resp.to_array::<4>().expect("response to array");
            assert!(common::check_u64(&num, req_num));
            assert!(common::check_u64(&status, 200));

            let headers = convert!(&*headers => HashMap<&str, &str>).expect("headers to HashMap");
            // We can't check the value of these headers because they aren't deterministic.
            assert!(headers.contains_key("x-cached"));
            assert!(headers.contains_key("date"));

            // We can't check the value of these headers because each header occurs multiple times.
            assert!(headers.contains_key("vary"));

            assert_eq!(headers.get("connection"), Some(&"keep-alive"));
            assert_eq!(headers.get("content-type"), Some(&"application/json"));
            assert_eq!(headers.get("server"), Some(&"nginx/1.14.0 (Ubuntu)"));
            assert_eq!(headers.get("transfer-encoding"), Some(&"chunked"));

            if let Noun::Cell(body) = &*body {
                let [_null, body_len, _body] = body.to_array::<3>().expect("body to array");
                assert!(common::check_u64(&body_len, 0x28c75));
            } else {
                panic!("body is an atom");
            }
        } else {
            panic!("response is an atom");
        }
    }

    // This HTTP request can be replicated from the command line:
    //
    // ```
    // $ curl -i -X PUT https://urbit.org
    // ```
    {
        let req_num = u64::MAX;
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("request")),
            // Request number.
            Noun::from(Atom::from(req_num)),
            // HTTP method.
            Noun::from(Atom::from("PUT")),
            // HTTP URI.
            Noun::from(Atom::from("https://urbit.org")),
            // HTTP headers.
            Noun::null(),
            // HTTP body.
            Noun::null(),
        ]));

        common::write_request(&mut input, req);
        if let Noun::Cell(resp) = common::read_response(&mut output) {
            let [num, status, _headers, _body] = resp.to_array::<4>().expect("response to array");
            assert!(common::check_u64(&num, req_num));
            assert!(common::check_u64(&status, 405));
        } else {
            panic!("response is an atom");
        }
    }
}

#[test]
fn cancel_request() {
    let mut driver = common::spawn_driver(
        "http-client",
        Path::new("cancel_request.http_client_tests.log"),
    );

    let mut input = driver.0.stdin.take().unwrap();
    let mut output = driver.0.stdout.take().unwrap();

    {
        let req_num = 1443u16;
        let req = Noun::from(Cell::from([
            // Tag.
            Noun::from(Atom::from("request")),
            // Request number.
            Noun::from(Atom::from(req_num)),
            // HTTP method.
            Noun::from(Atom::from("GET")),
            // HTTP URI.
            Noun::from(Atom::from(
                "https://bootstrap.urbit.org/props/1.10/brass.pill",
            )),
            // HTTP headers.
            Noun::null(),
            // HTTP body.
            Noun::null(),
        ]));

        let cancel_req = Noun::from(Cell::from([
            Noun::from(Atom::from("cancel-request")),
            Noun::from(Atom::from(req_num)),
        ]));
        common::write_request(&mut input, req);
        common::write_request(&mut input, cancel_req);

        let (done_tx, done_rx) = mpsc::channel();
        // Spawn a thread to read the response and then notify the main thread.
        thread::spawn(move || {
            let _ = common::read_response(&mut output);
            done_tx.send(()).expect("send");
        });

        // Conclude that we successfully cancelled the request if we still haven't heard from the
        // response thread in 5s.
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Timeout),
        );

        // The thread we spawned can't be joined since it's stuck attempting to read a response
        // that will never come, so we leave it under the assumption that the entire process is
        // cleaned up shortly after anyway.
    }
}
