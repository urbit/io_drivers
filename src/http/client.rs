use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Error as HyperError, Request as HyperRequest,
};
use noun::serdes::{Cue, Jam};
use tokio::sync::mpsc::{Receiver, Sender};

struct Request(HyperRequest<Body>);

impl Cue for Request {
    type Error = Error;

    fn cue(_jammed_val: Vec<u8>) -> Result<Self, Self::Error> {
        Err(Self::Error::Cue)
    }
}

struct Response(Parts, Bytes);

impl Jam for Response {
    type Error = Error;

    fn jam(self) -> Result<Vec<u8>, Self::Error> {
        Err(Self::Error::Jam)
    }
}

#[derive(Debug)]
enum Error {
    Cue,
    Hyper(HyperError),
    Jam,
}

impl From<HyperError> for Error {
    fn from(err: HyperError) -> Self {
        Self::Hyper(err)
    }
}

impl Jam for Error {
    type Error = Self;

    fn jam(self) -> Result<Vec<u8>, Self::Error> {
        Err(Self::Jam)
    }
}

/// Send an HTTP request and receive its response.
async fn send_request(client: Client<HttpConnector>, req: Vec<u8>) -> Result<Vec<u8>, Error> {
    let req = Request::cue(req)?;
    let (parts, body) = client.request(req.0).await?.into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await?;

    let resp = Response(parts, body).jam()?;
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
                Err(err) => err.jam().expect("failed to jam error"),
            };
            // TODO: better error handling.
            resp_tx_clone.send(resp).await.unwrap();
        });
    }
    println!("http client task exiting");
}
