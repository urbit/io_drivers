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
use std::{future::Future, mem::size_of};
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

    fn from_noun(req_noun: Noun<Atom, Cell<Atom>>) -> Result<Self, ()> {
        unimplemented!()
    }
}

struct Response(Parts, Bytes);

impl IntoNoun<Atom, Cell<Atom>, Noun<Atom, Cell<Atom>>> for Response {
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
    let (resp_parts, resp_body) = {
        let resp = client.request(req.req).await.map_err(|_| ())?;
        let (parts, body) = resp.into_parts();

        // Wait for the entire response body to come in.
        let body = body::to_bytes(body).await.map_err(|_| ())?;
        (parts, body)
    };

    let resp_noun = Response(resp_parts, resp_body).into_noun()?;

    let resp = resp_noun.jam()?;
    Ok(resp)
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
