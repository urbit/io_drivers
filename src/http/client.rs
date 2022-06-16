use crate::Endianness;
use bitstream_io::{BitReader, BitWriter};
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Error as HyperError, Request as HyperRequest,
};
use noun::{r#enum::Noun, Cue, FromNoun, IntoNoun, Jam};
use tokio::sync::mpsc::{Receiver, Sender};

struct Request(HyperRequest<Body>);

impl FromNoun for Request {
    type Error = Error;
    type Noun = Noun;

    fn from_noun(_noun: Self::Noun) -> Result<Self, Self::Error> {
        todo!()
    }
}

struct Response(Parts, Bytes);

impl IntoNoun for Response {
    type Error = Error;
    type Noun = Noun;

    fn into_noun(self) -> Result<Self::Noun, Self::Error> {
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

impl IntoNoun for Error {
    type Error = Self;
    type Noun = Noun;

    fn into_noun(self) -> Result<Self::Noun, Self::Error> {
        todo!()
    }
}

/// Send an HTTP request and receive its response.
async fn send_request(client: Client<HttpConnector>, req: Vec<u8>) -> Vec<u8> {
    let bitstream: BitReader<&[_], Endianness> = BitReader::new(&req[..]);

    let req_noun = Noun::cue(bitstream);
    if let Err(_) = req_noun {
        todo!("handle error");
    }

    let req = Request::from_noun(req_noun.unwrap());
    if let Err(_) = req {
        todo!("handle error");
    }

    let resp = client.request(req.unwrap().0).await;
    if let Err(_) = resp {
        todo!("handle error");
    }
    let (parts, body) = resp.unwrap().into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await;
    if let Err(_) = body {
        todo!("handle error");
    }

    let resp_noun = Response(parts, body.unwrap()).into_noun();
    if let Err(_) = resp_noun {
        todo!("handle error");
    }

    let resp = Vec::new();
    let mut bitstream: BitWriter<Vec<_>, Endianness> = BitWriter::new(resp);
    let resp_noun = resp_noun.unwrap().jam(&mut bitstream);
    if let Err(_) = resp_noun {
        todo!("handle error");
    }

    bitstream.into_writer()
}

/// HTTP client driver entry point.
pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {
        let client_clone = client.clone();
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(async move {
            let resp = send_request(client_clone, req).await;
            // TODO: better error handling.
            resp_tx_clone.send(resp).await.unwrap();
        });
    }
    println!("http client task exiting");
}
