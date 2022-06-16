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
    Cue,
    FromNoun,
    Hyper(HyperError),
    IntoNoun,
    Jam,
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
async fn send_request(client: Client<HttpConnector>, req: Vec<u8>) -> Result<Vec<u8>, Error> {
    // TODO: better error handling.
    let req_noun = Noun::cue(req).map_err(|_| Error::Cue)?;
    // TODO: better error handling.
    let req = Request::from_noun(req_noun).map_err(|_| Error::FromNoun)?;
    let (parts, body) = client.request(req.0).await?.into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await?;

    // TODO: better error handling.
    let resp = Response(parts, body)
        .into_noun()
        .map_err(|_| Error::IntoNoun)?
        .jam()
        .map_err(|_| Error::Jam)?;
    Ok(resp)
}

/// HTTP client driver entry point.
pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {
        let client_clone = client.clone();
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(async move {
            let resp = match send_request(client_clone, req).await {
                Ok(resp) => resp,
                Err(err) => err.into_noun().expect("into noun").jam().expect("jam"),
            };
            // TODO: better error handling.
            resp_tx_clone.send(resp).await.unwrap();
        });
    }
    println!("http client task exiting");
}
