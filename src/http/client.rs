use crate::{Endianness, RequestTag};
use bitstream_io::{BitReader, BitWriter};
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Error as HyperError, Request as HyperRequest,
};
use noun::{
    serdes::{Cue, Jam},
    types::{atom::Atom, cell::Cell, noun::Noun},
    Cell as _, FromNoun, IntoNoun, Noun as _,
};
use std::mem::size_of;
use tokio::sync::mpsc::{Receiver, Sender};

struct Request(HyperRequest<Body>);

impl FromNoun<Atom, Cell, Noun> for Request {
    fn from_noun(_noun: Noun) -> Result<Self, ()> {
        todo!()
    }
}

struct Response(Parts, Bytes);

impl IntoNoun<Atom, Cell, Noun> for Response {
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
    fn into_noun(self) -> Result<Noun, ()> {
        todo!()
    }
}

/// Send an HTTP request and receive its response.
async fn send_http_request(client: Client<HttpConnector>, req: Noun) -> Result<Vec<u8>, ()> {
    // Parse request.
    let req = Request::from_noun(req)?;

    // Send request and receive response.
    let (resp_parts, resp_body) = {
        let resp = client.request(req.0).await.map_err(|_| ())?;
        let (parts, body) = resp.into_parts();

        // Wait for the entire response body to come in.
        let body = body::to_bytes(body).await.map_err(|_| ())?;
        (parts, body)
    };

    let resp_noun = Response(resp_parts, resp_body).into_noun()?;

    let resp = Vec::new();
    let mut bitstream: BitWriter<Vec<_>, Endianness> = BitWriter::new(resp);
    resp_noun.jam(&mut bitstream)?;

    let resp = bitstream.into_writer();
    Ok(resp)
}

async fn handle_io_request(
    _client: Client<HttpConnector>,
    req: Vec<u8>,
    _resp_tx: Sender<Vec<u8>>,
) -> Result<(), ()> {
    let (tag, req_noun) = {
        // First byte is the request type, which should be skipped.
        let start = size_of::<RequestTag>();
        let bitstream: BitReader<&[_], Endianness> = BitReader::new(&req[start..]);

        let req_noun = Noun::cue(bitstream)?;
        req_noun.into_cell().map_err(|_| ())?.into_parts()
    };

    let tag = tag.as_atom()?;
    if tag == "request" {
        println!("rust: send HTTP request");
    } else if tag == "cancel-request" {
        println!("rust: cancel HTTP request");
    } else {
        todo!("handle unknown type");
    }
    //resp_tx.send(resp).await.unwrap();
    Ok(())
}

/// HTTP client driver entry point.
pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {
        let client_clone = client.clone();
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(handle_io_request(client_clone, req, resp_tx_clone));
    }
}
