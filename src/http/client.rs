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
    serdes::{Cue, Jam},
    types::{atom::Atom, cell::Cell, noun::Noun},
    Atom as _, Cell as _, FromNoun, IntoNoun, Noun as _,
};
use rustls::ClientConfig;
use std::{future::Future, mem::size_of};
use tokio::sync::mpsc::{Receiver, Sender};

type HyperClient = Client<HttpsConnector<HttpConnector>, Body>;

struct Request {
    req_num: u64,
    req: HyperRequest<Body>,
}

impl FromNoun<Atom, Cell, Noun> for Request {
    fn from_noun_ref(req_noun: &Noun) -> Result<Self, ()> {
        let (req_num, req_noun) = req_noun.as_cell()?.as_parts();
        let req_num = req_num.as_atom()?.as_u64()?;

        let mut req = HyperRequest::builder();

        let (method, req_noun) = req_noun.as_cell()?.as_parts();
        req = req.method(method.as_atom()?.as_str()?);

        let (uri, req_noun) = req_noun.as_cell()?.as_parts();
        req = req.uri(uri.as_atom()?.as_str()?);

        let (mut headers, body) = req_noun.as_cell()?.as_parts();

        while let Ok(cell) = headers.as_cell() {
            let header = cell.head();
            headers = cell.tail();

            let (key, val) = header.as_cell()?.as_parts();
            let (key, val) = (key.as_atom()?.as_str()?, val.as_atom()?.as_str()?);
            req = req.header(key, val);
        }

        let body = if let Ok(body) = body.as_cell() {
            let (_body_len, body) = body.as_parts();
            Body::from(String::from(body.as_atom()?.as_str()?))
        } else {
            Body::empty()
        };

        let req = req.body(body).map_err(|_| ())?;
        Ok(Self { req_num, req })
    }

    fn from_noun(_req: Noun) -> Result<Self, ()> {
        Err(())
    }
}

struct Response(Parts, Bytes);

impl IntoNoun<Atom, Cell, Noun> for Response {
    fn as_noun(&self) -> Result<Noun, ()> {
        todo!()
    }

    fn into_noun(self) -> Result<Noun, ()> {
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

impl IntoNoun<Atom, Cell, Noun> for Error {
    fn as_noun(&self) -> Result<Noun, ()> {
        todo!()
    }

    fn into_noun(self) -> Result<Noun, ()> {
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
