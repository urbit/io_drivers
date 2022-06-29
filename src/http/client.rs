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
            let req = req.as_cell()?;
            let (req_num, req) = (req.head(), req.tail());

            let req = req.as_cell()?;
            let (method, req) = (req.head(), req.tail());

            let req = req.as_cell()?;
            let (uri, req) = (req.head(), req.tail());

            let req = req.as_cell()?;
            let (headers, body) = (req.head(), req.tail());

            (req_num, method, uri, headers, body)
        };
        let req_num = req_num.as_atom()?.as_u64()?;

        let mut req = HyperRequest::builder()
            .method(method.as_atom()?.as_str()?)
            .uri(uri.as_atom()?.as_str()?);

        while let Ok(cell) = headers.as_cell() {
            let header = cell.head().as_cell()?;
            headers = cell.tail();

            let (key, val) = (header.head(), header.tail());
            let (key, val) = (key.as_atom()?.as_str()?, val.as_atom()?.as_str()?);
            req = req.header(key, val);
        }

        let body = if let Ok(body) = body.as_cell() {
            let (_body_len, body) = (body.head(), body.tail());
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
    parts: Parts,
    body: Bytes,
}

/// Need
/// - status as u32,
/// - headers as noun,
/// - body as noun,
impl IntoNoun<Atom, Cell<Atom>, Noun<Atom, Cell<Atom>>> for Response {
    fn to_noun(&self) -> Result<Noun<Atom, Cell<Atom>>, ()> {
        let status = Rc::new(Atom::from_u16(self.parts.status.as_u16()).into_noun_unchecked());

        let headers = {
            let null = Rc::new(Atom::from_u8(0).into_noun_unchecked());
            let mut headers_cell = null;
            let headers = &self.parts.headers;
            for key in headers.keys().map(|k| k.as_str()) {
                let vals = headers.get_all(key);
                let key = Rc::new(Atom::from(key).into_noun_unchecked());
                for val in vals {
                    let val = match val.to_str() {
                        Ok(val) => Rc::new(Atom::from(val).into_noun_unchecked()),
                        Err(_) => todo!("handle ToStrError"),
                    };
                    let head = Rc::new(Cell::new(key.clone(), val).into_noun_unchecked());
                    let tail = headers_cell.clone();
                    headers_cell = Rc::new(Cell::new(head, tail).into_noun_unchecked());
                }
            }
            headers_cell
        };
        let body = Rc::new(Atom::from(self.body.to_vec()).into_noun_unchecked());

        Ok(Cell::new(
            status,
            Rc::new(Cell::new(headers, body).into_noun_unchecked()),
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
async fn send_http_request(client: HyperClient, req: Request) -> Result<Vec<u8>, ()> {
    // Send request and receive response.
    let resp = client.request(req.req).await.map_err(|_| ())?;
    let (parts, body) = resp.into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await.map_err(|_| ())?;

    Response { parts, body }.into_noun()?.jam()
}

/// This has to be synchronous because Noun is not Send.
fn handle_io_request(
    client: HyperClient,
    req: Vec<u8>,
    _resp_tx: Sender<Vec<u8>>,
) -> Result<impl Future<Output = Result<Vec<u8>, ()>>, ()> {
    let (tag, req_noun) = {
        // First byte is the request type, which should be skipped.
        let start = size_of::<RequestTag>();
        let bitstream: BitReader<&[_], Endianness> = BitReader::new(&req[start..]);
        let noun = Noun::cue(bitstream)?;
        noun.into_cell().map_err(|_| ())?.into_parts()
    };

    let tag = tag.as_atom()?;
    if tag == "request" {
        let req = Request::from_noun_ref(&req_noun)?;
        return Ok(send_http_request(client, req));
    } else if tag == "cancel-request" {
        todo!("cancel request");
    } else {
        return Err(());
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
