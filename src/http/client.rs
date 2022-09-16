use crate::{atom_as_str, Driver, Status};
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    header,
    http::response::Parts,
    Body, Request as HyperRequest,
};
use hyper_rustls::{ConfigBuilderExt, HttpsConnector, HttpsConnectorBuilder};
use log::{debug, info, warn};
use noun::{atom::Atom, cell::Cell, convert, Noun, Rc};
use rustls::ClientConfig;
use std::collections::HashMap;
use tokio::{
    io::{self, Stdin, Stdout},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

//==================================================================================================
// Request Types
//==================================================================================================

/// Requests that can be handled by the HTTP client driver.
enum Request {
    SendRequest(SendRequest),
    CancelRequest(CancelRequest),
}

impl_try_from_noun_for_request!(
    Request,
    "request" => SendRequest,
    "cancel-request" => CancelRequest,
);

/// A request to send an HTTP request.
#[derive(Debug)]
struct SendRequest {
    req_num: u64,
    req: HyperRequest<Body>,
}

impl TryFrom<&Noun> for SendRequest {
    type Error = convert::Error;

    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Cell(data) = data {
            let [req_num, method, uri, headers, body] =
                data.to_array::<5>().ok_or(convert::Error::MissingValue)?;
            if let (Noun::Atom(req_num), Noun::Atom(method), Noun::Atom(uri), headers, body) =
                (&*req_num, &*method, &*uri, headers, body)
            {
                let req_num = req_num.as_u64().ok_or(convert::Error::AtomToUint)?;

                let mut req = HyperRequest::builder()
                    .method(atom_as_str(method)?)
                    .uri(atom_as_str(uri)?);

                for (key, val) in convert!(&*headers => HashMap<&str, &str>)? {
                    req = req.header(key, val);
                }

                let (body_len, body) = match &*body {
                    Noun::Atom(_) => (0, Body::empty()),
                    Noun::Cell(body) => {
                        let [_null, body_len, body] =
                            body.to_array::<3>().ok_or(convert::Error::MissingValue)?;

                        if let (Noun::Atom(body_len), Noun::Atom(body)) = (&*body_len, &*body) {
                            let body_len = body_len.as_u64().ok_or(convert::Error::AtomToUint)?;
                            // Ensure trailing null bytes are retained.
                            let mut body = String::from(atom_as_str(body)?);
                            let expected_len = usize::try_from(body_len)
                                .map_err(|_| convert::Error::AtomToUint)?;
                            while body.len() < expected_len {
                                body.push('\0');
                            }
                            (body_len, Body::from(body))
                        } else {
                            return Err(convert::Error::UnexpectedCell);
                        }
                    }
                };

                let host = {
                    let uri = req.uri_ref().ok_or(convert::Error::MissingValue)?;
                    match (uri.host(), uri.port()) {
                        (Some(host), Some(port)) => format!("{}:{}", host, port),
                        (Some(host), None) => String::from(host),
                        _ => return Err(convert::Error::MissingValue),
                    }
                };
                let req = req
                    .header("Content-Length", body_len)
                    .header("Host", host)
                    .body(body)
                    .map_err(|_| convert::Error::ImplType)?;

                Ok(Self { req_num, req })
            } else {
                Err(convert::Error::UnexpectedCell)
            }
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

/// A request to cancel an inflight HTTP request.
#[derive(Debug)]
struct CancelRequest {
    req_num: u64,
}

impl TryFrom<&Noun> for CancelRequest {
    type Error = convert::Error;

    fn try_from(data: &Noun) -> Result<Self, Self::Error> {
        if let Noun::Atom(req_num) = data {
            Ok(Self {
                req_num: req_num.as_u64().ok_or(convert::Error::AtomToUint)?,
            })
        } else {
            Err(convert::Error::UnexpectedCell)
        }
    }
}

//==================================================================================================
// Driver
//==================================================================================================

/// The HTTP client driver.
pub struct HttpClient {
    hyper: Client<HttpsConnector<HttpConnector>, Body>,
    /// Map from request number to request task. Must only be accessed from a single task.
    inflight_req: HashMap<u64, JoinHandle<()>>,
}

impl HttpClient {
    /// Sends an HTTP request, writing the reponse to the output channel.
    fn send_request(&mut self, req: SendRequest, output_tx: Sender<Noun>) {
        debug!(target: Self::name(), "request = {:?}", req);

        let req_num = req.req_num;
        debug!(target: Self::name(), "request number = {}", req_num);
        let task = {
            let hyper = self.hyper.clone();
            let task = tokio::spawn(async move {
                let resp = match hyper.request(req.req).await {
                    Ok(resp) => resp,
                    Err(err) => {
                        warn!(
                            target: Self::name(),
                            "failed to send request #{}: {}", req_num, err
                        );
                        return;
                    }
                };
                debug!(
                    target: Self::name(),
                    "response to request #{} = {:?}", req_num, resp
                );

                let (parts, body) = resp.into_parts();

                let body = match body::to_bytes(body).await {
                    Ok(body) => body,
                    Err(err) => {
                        warn!(
                            target: Self::name(),
                            "failed to receive entire body of request #{}: {}", req_num, err
                        );
                        return;
                    }
                };
                debug!(
                    target: Self::name(),
                    "response body to request #{} = {:?}", req_num, body
                );

                info!(
                    target: Self::name(),
                    "received status {} in response to request #{}",
                    parts.status.as_u16(),
                    req_num
                );

                let resp = {
                    let resp = HyperResponse {
                        req_num: req.req_num,
                        parts,
                        body,
                    };
                    match Noun::try_from(resp) {
                        Ok(resp) => resp,
                        Err(err) => {
                            warn!(
                                target: Self::name(),
                                "failed to convert response to request #{} into noun: {}",
                                req_num,
                                err
                            );
                            return;
                        }
                    }
                };
                if let Err(_resp) = output_tx.send(resp).await {
                    warn!(
                        target: Self::name(),
                        "failed to send response to request #{} to output task", req_num
                    );
                } else {
                    info!(
                        target: Self::name(),
                        "sent response to request #{} to output task", req_num
                    );
                }
            });
            debug!("spawned task to handle request #{}", req_num);
            task
        };
        self.inflight_req.insert(req_num, task);
    }

    /// Cancels an inflight HTTP request.
    fn cancel_request(&mut self, req: CancelRequest) {
        if let Some(task) = self.inflight_req.remove(&req.req_num) {
            task.abort();
            info!(
                target: Self::name(),
                "aborted task for request #{}", req.req_num
            );
        } else {
            warn!(
                target: Self::name(),
                "no task for request #{} found in request cache", req.req_num
            );
        }
    }
}

/// Implements the [`Driver`] trait for the [`HttpClient`] driver.
macro_rules! impl_driver {
    ($input_src:ty, $output_sink:ty) => {
        impl Driver<$input_src, $output_sink> for HttpClient {
            fn new() -> Result<Self, Status> {
                let tls = ClientConfig::builder()
                    .with_safe_defaults()
                    .with_native_roots()
                    .with_no_client_auth();

                let https = HttpsConnectorBuilder::new()
                    .with_tls_config(tls)
                    .https_or_http()
                    .enable_http1()
                    .build();

                let hyper = Client::builder().build(https);
                let inflight_req = HashMap::new();
                debug!(target: Self::name(), "initialized driver");
                Ok(Self {
                    hyper,
                    inflight_req,
                })
            }

            fn name() -> &'static str {
                "http-client"
            }

            fn handle_requests(
                mut self,
                mut input_rx: Receiver<Noun>,
                output_tx: Sender<Noun>,
            ) -> JoinHandle<Status> {
                let task = tokio::spawn(async move {
                    while let Some(req) = input_rx.recv().await {
                        match Request::try_from(req) {
                            Ok(Request::SendRequest(req)) => {
                                self.send_request(req, output_tx.clone())
                            }
                            Ok(Request::CancelRequest(req)) => self.cancel_request(req),
                            _ => {
                                warn!(target: Self::name(), "skipping unidentifiable request");
                            }
                        }
                    }
                    for (req_num, task) in self.inflight_req {
                        if let Err(err) = task.await {
                            warn!(
                                target: Self::name(),
                                "request #{} task failed to complete successfully: {}",
                                req_num,
                                err
                            );
                        } else {
                            info!(
                                target: Self::name(),
                                "request #{} task completed successfully", req_num
                            );
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

/// Provides an FFI-friendly interface for running the HTTP client driver with `stdin` as the input
/// source and `stdout` as the output sink.
#[no_mangle]
pub extern "C" fn http_client_run() -> Status {
    match HttpClient::new() {
        Ok(driver) => driver.run(io::stdin(), io::stdout()),
        Err(status) => status,
    }
}

//==================================================================================================
// Miscellaneous
//==================================================================================================

/// A response to an HTTP request.
#[derive(Debug)]
struct HyperResponse {
    req_num: u64,
    parts: Parts,
    body: Bytes,
}

impl TryFrom<HyperResponse> for Noun {
    type Error = header::ToStrError;

    fn try_from(resp: HyperResponse) -> Result<Self, Self::Error> {
        let req_num = Rc::<Noun>::from(Atom::from(resp.req_num));
        let status = Rc::<Noun>::from(Atom::from(resp.parts.status.as_u16()));
        let null = Rc::<Noun>::from(Atom::null());

        let headers = {
            let mut headers_cell = null.clone();
            let headers = &resp.parts.headers;
            for key in headers.keys().map(|k| k.as_str()) {
                let vals = headers.get_all(key);
                let key = Rc::<Noun>::from(Atom::from(key));
                for val in vals {
                    let val = Rc::<Noun>::from(Atom::from(val.to_str()?));
                    headers_cell = Rc::<Noun>::from(Cell::from([
                        Rc::<Noun>::from(Cell::from([key.clone(), val])),
                        headers_cell,
                    ]));
                }
            }
            headers_cell
        };

        let body = {
            let body = resp.body.to_vec();
            if body.is_empty() {
                null
            } else {
                let body_len = Atom::from(body.len());
                let body = Atom::from(body);
                Rc::<Noun>::from(Cell::from([
                    null,
                    Rc::<Noun>::from(Cell::from([body_len, body])),
                ]))
            }
        };

        Ok(Noun::from(Cell::from([req_num, status, headers, body])))
    }
}

//==================================================================================================
// Tests
//==================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::http::response;

    #[test]
    fn noun_from_response() {
        // [
        //   107
        //   [
        //     200
        //     [%x-cached 'HIT']
        //     [%vary 'Origin']
        //     [[%vary 'Origin'] 'Accept-Encoding']
        //     [%connection %keep-alive]
        //     [%content-length 14645]
        //     [%content-type 'application/json']
        //     [%date 'Fri, 08 Jul 2022 16:43:50 GMT']
        //     [%server 'nginx/1.14.0 (Ubuntu)']
        //     0
        //   ]
        //   [0 59 '[{"jsonrpc":"2.0","id":"block number","result":"0xe67461"}]']
        // ]
        {
            let req_num = 107u64;
            let (parts, _body) = response::Builder::new()
                .status(200)
                .header("x-cached", "HIT")
                .header("vary", "Origin")
                .header("vary", "Accept-Encoding")
                .header("connection", "keep-alive")
                .header("content-length", "14645")
                .header("content-type", "application/json")
                .header("date", "Fri, 08 Jul 2022 16:43:50 GMT")
                .header("server", "nginx/1.14.0 (Ubuntu)")
                .body(())
                .expect("build response")
                .into_parts();
            let body =
                Bytes::from(r#"[{"jsonrpc":"2.0","id":"block number","result":"0xe67461"}]"#);

            let resp = HyperResponse {
                req_num,
                parts,
                body,
            };

            let noun = Noun::try_from(resp).expect("noun from response");
            let expected = Noun::from(Cell::from([
                Noun::from(Atom::from(req_num)),
                Noun::from(Atom::from(200u8)),
                Noun::from(Cell::from([
                    Noun::from(Cell::from([
                        Atom::from("server"),
                        Atom::from("nginx/1.14.0 (Ubuntu)"),
                    ])),
                    Noun::from(Cell::from([
                        Atom::from("date"),
                        Atom::from("Fri, 08 Jul 2022 16:43:50 GMT"),
                    ])),
                    Noun::from(Cell::from([
                        Atom::from("content-type"),
                        Atom::from("application/json"),
                    ])),
                    Noun::from(Cell::from([
                        Atom::from("content-length"),
                        Atom::from("14645"),
                    ])),
                    Noun::from(Cell::from([
                        Atom::from("connection"),
                        Atom::from("keep-alive"),
                    ])),
                    Noun::from(Cell::from([
                        Atom::from("vary"),
                        Atom::from("Accept-Encoding"),
                    ])),
                    Noun::from(Cell::from([Atom::from("vary"), Atom::from("Origin")])),
                    Noun::from(Cell::from([Atom::from("x-cached"), Atom::from("HIT")])),
                    Noun::from(Atom::from(0u8)),
                ])),
                Noun::from(Cell::from([
                    Atom::from(0u8),
                    Atom::from(59u8),
                    Atom::from(r#"[{"jsonrpc":"2.0","id":"block number","result":"0xe67461"}]"#),
                ])),
            ]));

            // If this test starts failing, it may be because the headers are in a different
            // (though still correct order).
            assert_eq!(noun, expected);
        }
    }
}
