use crate::Driver;
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Request as HyperRequest,
};
use hyper_rustls::{ConfigBuilderExt, HttpsConnector, HttpsConnectorBuilder};
use noun::{
    atom::Atom,
    cell::Cell,
    convert::{self, IntoNoun, TryFromNoun, TryIntoNoun},
    Noun, Rc,
};
use rustls::ClientConfig;
use std::sync::Arc;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

struct Request {
    req_num: u64,
    req: HyperRequest<Body>,
}

impl TryFromNoun<Rc<Noun>> for Request {
    fn try_from_noun(req: Rc<Noun>) -> Result<Self, convert::Error> {
        fn atom_as_str(atom: &Atom) -> Result<&str, convert::Error> {
            atom.as_str().map_err(|_| convert::Error::AtomToStr)
        }

        if let Noun::Cell(req) = &*req {
            let [req_num, method, uri, headers, body] =
                req.as_list::<5>().ok_or(convert::Error::MissingValue)?;
            if let (Noun::Atom(req_num), Noun::Atom(method), Noun::Atom(uri), mut headers, body) =
                (&*req_num, &*method, &*uri, headers, body)
            {
                let req_num = req_num.as_u64().ok_or(convert::Error::AtomToUint)?;

                let mut req = HyperRequest::builder()
                    .method(atom_as_str(method)?)
                    .uri(atom_as_str(uri)?);

                while let Noun::Cell(cell) = &*headers {
                    let header = cell.head();
                    if let Noun::Cell(header) = &*header {
                        if let (Noun::Atom(key), Noun::Atom(val)) =
                            (&*header.head(), &*header.tail())
                        {
                            req = req.header(atom_as_str(key)?, atom_as_str(val)?);
                        } else {
                            return Err(convert::Error::UnexpectedCell);
                        }
                    } else {
                        return Err(convert::Error::UnexpectedAtom);
                    }
                    headers = cell.tail();
                }

                let (body_len, body) = match &*body {
                    Noun::Atom(_) => (0, Body::empty()),
                    Noun::Cell(body) => {
                        let [_null, body_len, body] =
                            body.as_list::<3>().ok_or(convert::Error::MissingValue)?;

                        if let (Noun::Atom(body_len), Noun::Atom(body)) = (&*body_len, &*body) {
                            let body_len = body_len.as_u64().ok_or(convert::Error::AtomToUint)?;
                            let body = Body::from(atom_as_str(body)?.to_string());
                            (body_len, body)
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

struct Response {
    req_num: u64,
    parts: Parts,
    body: Bytes,
}

impl TryIntoNoun<Noun> for Response {
    type Error = ();

    fn try_into_noun(self) -> Result<Noun, ()> {
        let req_num = Atom::from(self.req_num).into_rc_noun();
        let status = Atom::from(self.parts.status.as_u16()).into_rc_noun();
        let null = Atom::null().into_rc_noun();

        let headers = {
            let mut headers_cell = null.clone();
            let headers = &self.parts.headers;
            for key in headers.keys().map(|k| k.as_str()) {
                let vals = headers.get_all(key);
                let key = Atom::from(key).into_rc_noun();
                for val in vals {
                    let val = match val.to_str() {
                        Ok(val) => Atom::from(val).into_rc_noun(),
                        Err(_) => todo!("handle ToStrError"),
                    };
                    headers_cell =
                        Cell::from([Cell::from([key.clone(), val]).into_rc_noun(), headers_cell])
                            .into_rc_noun();
                }
            }
            headers_cell
        };

        let body = {
            let body = self.body.to_vec();
            if body.is_empty() {
                null
            } else {
                let body_len = Atom::from(body.len());
                let body = Atom::from(body);
                Cell::from([null, Cell::from([body_len, body]).into_rc_noun()]).into_rc_noun()
            }
        };

        Ok(Cell::from([req_num, status, headers, body]).into_noun())
    }
}

pub struct HttpClient {
    hyper: Client<HttpsConnector<HttpConnector>, Body>,
}

impl HttpClient {
    /// Handle an HTTP request, returning `None` if an error occurred or if no response is needed.
    async fn handle_http_request(&self, req: Noun) -> Option<Noun> {
        let (tag, req) = if let Noun::Cell(req) = req {
            req.into_parts()
        } else {
            return None;
        };

        if let Noun::Atom(tag) = &*tag {
            let req = Request::try_from_noun(req).unwrap();
            match tag.as_str() {
                Ok("request") => {
                    let resp = self.hyper.request(req.req).await.ok()?;
                    let (parts, body) = resp.into_parts();

                    let body = body::to_bytes(body).await.ok()?;

                    Response {
                        req_num: req.req_num,
                        parts,
                        body,
                    }
                    .try_into_noun()
                    .ok()
                }
                Ok("cancel-request") => todo!("cancel request"),
                _ => todo!("handle error"),
            }
        } else {
            None
        }
    }
}

impl Driver for HttpClient {
    fn run(mut req_rx: Receiver<Noun>, resp_tx: Sender<Noun>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let driver = {
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
                Arc::new(Self { hyper })
            };

            while let Some(req) = req_rx.recv().await {
                let driver_clone = driver.clone();
                let resp_tx_clone = resp_tx.clone();
                tokio::spawn(async move {
                    if let Some(resp) = driver_clone.handle_http_request(req).await {
                        resp_tx_clone.send(resp).await.expect("send");
                    }
                });
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::http::response;

    #[test]
    fn response_into_noun() {
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

            let resp = Response {
                req_num,
                parts,
                body,
            };

            let noun = resp.try_into_noun().expect("to noun");
            let expected = Cell::from([
                Atom::from(req_num).into_noun(),
                Atom::from(200u8).into_noun(),
                Cell::from([
                    Cell::from([Atom::from("server"), Atom::from("nginx/1.14.0 (Ubuntu)")])
                        .into_noun(),
                    Cell::from([
                        Atom::from("date"),
                        Atom::from("Fri, 08 Jul 2022 16:43:50 GMT"),
                    ])
                    .into_noun(),
                    Cell::from([Atom::from("content-type"), Atom::from("application/json")])
                        .into_noun(),
                    Cell::from([Atom::from("content-length"), Atom::from("14645")]).into_noun(),
                    Cell::from([Atom::from("connection"), Atom::from("keep-alive")]).into_noun(),
                    Cell::from([Atom::from("vary"), Atom::from("Accept-Encoding")]).into_noun(),
                    Cell::from([Atom::from("vary"), Atom::from("Origin")]).into_noun(),
                    Cell::from([Atom::from("x-cached"), Atom::from("HIT")]).into_noun(),
                    Atom::from(0u8).into_noun(),
                ])
                .into_noun(),
                Cell::from([
                    Atom::from(0u8),
                    Atom::from(59u8),
                    Atom::from(r#"[{"jsonrpc":"2.0","id":"block number","result":"0xe67461"}]"#),
                ])
                .into_noun(),
            ])
            .into_noun();

            // If this test starts failing, it may be because the headers are in a different
            // (though still correct order).
            assert_eq!(noun, expected);
        }
    }
}
