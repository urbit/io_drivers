use crate::{Endianness, RequestTag};
use bitstream_io::BitReader;
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Error as HyperError, Request as HyperRequest,
};
use hyper_rustls::{ConfigBuilderExt, HttpsConnector, HttpsConnectorBuilder};
use noun::{
    atom::{types::VecAtom as Atom, Atom as _},
    cell::{types::RcCell as Cell, Cell as _},
    convert::{FromNoun, IntoNoun},
    noun::{types::EnumNoun as Noun, Noun as _},
    serdes::{Cue, Jam},
};
use rustls::ClientConfig;
use std::{future::Future, mem::size_of, rc::Rc};
use tokio::sync::mpsc::{Receiver, Sender};

type HyperClient = Client<HttpsConnector<HttpConnector>, Body>;

struct Request {
    req_num: u64,
    req: HyperRequest<Body>,
}

impl FromNoun<Atom, Cell<Atom>, Noun<Atom, Cell<Atom>>> for Request {
    fn from_noun_ref(req: &Noun<Atom, Cell<Atom>>) -> Result<Self, ()> {
        let (req_num, method, uri, mut headers, body) = {
            let (req_num, req) = req.as_cell()?.as_parts();
            let (method, req) = req.as_cell()?.as_parts();
            let (uri, req) = req.as_cell()?.as_parts();
            let (headers, body) = req.as_cell()?.as_parts();
            (req_num, method, uri, headers, body)
        };
        let req_num = req_num.as_atom()?.as_u64()?;

        let mut req = HyperRequest::builder()
            .method(method.as_atom()?.as_str()?)
            .uri(uri.as_atom()?.as_str()?);

        while let Ok(cell) = headers.as_cell() {
            let (header, remaining_headers) = cell.as_parts();
            let header = header.as_cell()?;
            headers = remaining_headers;

            let (key, val) = header.as_parts();
            let (key, val) = (key.as_atom()?.as_str()?, val.as_atom()?.as_str()?);
            req = req.header(key, val);
        }

        let body = if let Ok(body) = body.as_cell() {
            let (_body_len, body) = body.as_parts();
            Body::from(body.as_atom()?.as_str()?.to_string())
        } else {
            Body::empty()
        };

        let req = req.body(body).map_err(|_| ())?;
        Ok(Self { req_num, req })
    }

    fn from_noun(_req: Noun<Atom, Cell<Atom>>) -> Result<Self, ()> {
        unimplemented!()
    }
}

struct Response {
    req_num: u64,
    parts: Parts,
    body: Bytes,
}

/// Need
/// - status as u32,
/// - headers as noun,
/// - body as noun,
impl IntoNoun<Atom, Cell<Atom>, Noun<Atom, Cell<Atom>>> for Response {
    fn to_noun(&self) -> Result<Noun<Atom, Cell<Atom>>, ()> {
        let req_num = Rc::new(Atom::from_u64(self.req_num).into_noun_unchecked());
        let status = Rc::new(Atom::from_u16(self.parts.status.as_u16()).into_noun_unchecked());

        let headers = {
            let null = Rc::new(Atom::from_u8(0).into_noun_unchecked());
            let mut headers = null;
            let mut cnt = 0;
            for (key, val) in &self.parts.headers {
                //eprintln!("[{}]: key={}, val={}", cnt, key.as_str(), val.to_str().unwrap());
                if cnt == 9 {
                    break;
                }
                if let Ok(val) = val.to_str() {
                    let key = Rc::new(Atom::from(key.as_str()).into_noun_unchecked());
                    let val = Rc::new(Atom::from(val).into_noun_unchecked());
                    let head = Rc::new(Cell::new(key, val).into_noun_unchecked());
                    let tail = headers;
                    headers = Rc::new(Cell::new(head, tail).into_noun_unchecked());
                } else {
                    todo!("handle ToStrError");
                }
                cnt += 1;
            }
            headers
        };

        let body = {
            let body = self.body.to_vec();
            let null = Rc::new(Atom::from_u8(0).into_noun_unchecked());
            if body.is_empty() {
                null
            } else {
                let body_len = Rc::new(Atom::from_usize(body.len()).into_noun_unchecked());
                let body = Rc::new(Atom::from(body).into_noun_unchecked());
                Rc::new(
                    Cell::new(
                        null,
                        Rc::new(Cell::new(body_len, body).into_noun_unchecked()),
                    )
                    .into_noun_unchecked(),
                )
            }
        };

        Ok(Cell::new(
            req_num,
            Rc::new(
                Cell::new(
                    status,
                    Rc::new(Cell::new(headers, body).into_noun_unchecked()),
                )
                .into_noun_unchecked(),
            ),
        )
        .into_noun_unchecked())
    }

    fn to_noun_unchecked(&self) -> Noun<Atom, Cell<Atom>> {
        todo!()
    }

    fn into_noun(self) -> Result<Noun<Atom, Cell<Atom>>, ()> {
        self.to_noun()
    }

    fn into_noun_unchecked(self) -> Noun<Atom, Cell<Atom>> {
        self.to_noun().expect("Response into Noun")
    }
}

#[derive(Debug)]
enum Error {
    //Cue,
    //FromNoun,
    Hyper(HyperError),
    //IntoNoun,
    //Jam,
}

impl From<HyperError> for Error {
    fn from(err: HyperError) -> Self {
        Self::Hyper(err)
    }
}

impl IntoNoun<Atom, Cell<Atom>, Noun<Atom, Cell<Atom>>> for Error {
    fn to_noun(&self) -> Result<Noun<Atom, Cell<Atom>>, ()> {
        todo!()
    }

    fn to_noun_unchecked(&self) -> Noun<Atom, Cell<Atom>> {
        todo!()
    }

    fn into_noun(self) -> Result<Noun<Atom, Cell<Atom>>, ()> {
        todo!()
    }

    fn into_noun_unchecked(self) -> Noun<Atom, Cell<Atom>> {
        todo!()
    }
}

/// Send an HTTP request and receive its response.
async fn send_http_request(
    client: HyperClient,
    req: Request,
    resp_tx: Sender<Vec<u8>>,
) -> Option<()> {
    // Send request and receive response.
    let resp = client.request(req.req).await.ok()?;
    let (parts, body) = resp.into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await.ok()?;

    let req_num = req.req_num;
    let resp = Response {
        req_num,
        parts,
        body,
    }
    .into_noun()
    .ok()?
    .jam()
    .ok()?;
    resp_tx.send(resp).await.ok()?;
    Some(())
}

/// This has to be synchronous because Noun is not Send.
fn handle_io_request(
    client: HyperClient,
    req: Vec<u8>,
    resp_tx: Sender<Vec<u8>>,
) -> Option<impl Future<Output = Option<()>>> {
    let (tag, req) = {
        // First byte is the request type, which should be skipped.
        const START: usize = size_of::<RequestTag>();
        let bitstream: BitReader<&[_], Endianness> = BitReader::new(&req[START..]);
        let noun = Noun::cue(bitstream).unwrap();
        noun.into_cell().unwrap().into_parts()
    };

    let tag = tag.as_atom().unwrap();
    if tag == "request" {
        let req = Request::from_noun_ref(&req).unwrap();
        Some(send_http_request(client, req, resp_tx))
    } else if tag == "cancel-request" {
        todo!("cancel request");
    } else {
        None
    }
}

/// HTTP client driver entry point.
pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client: HyperClient = {
        let tls = ClientConfig::builder()
            .with_safe_defaults()
            .with_native_roots()
            .with_no_client_auth();

        let https = HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();

        Client::builder().build(https)
    };

    while let Some(req) = req_rx.recv().await {
        let client_clone = client.clone();
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(async move { handle_io_request(client_clone, req, resp_tx_clone)?.await });
    }
}
