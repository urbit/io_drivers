use crate::{Request, Response};
use hyper::{
    body::{self, Bytes},
    client::{Client, HttpConnector},
    http::response::Parts,
    Body, Error as HyperError, Request as HyperRequest,
};
use tokio::sync::mpsc::{Receiver, Sender};

impl Request for HyperRequest<Body> {
    type Error = Error;

    fn deserialize(_req: Vec<u8>) -> Result<Self, Self::Error> {
        Err(Self::Error::Deserialization)
    }
}

type HyperResponse = (Parts, Bytes);

impl Response for HyperResponse {
    fn serialize(self) -> Vec<u8> {
        todo!()
    }
}

/// HTTP client error.
pub enum Error {
    Deserialization,
    Hyper(HyperError),
}

impl From<HyperError> for Error {
    fn from(err: HyperError) -> Self {
        Self::Hyper(err)
    }
}

/// Send an HTTP request and receive its response.
async fn send_request(
    client: Client<HttpConnector>,
    req: Vec<u8>,
    resp_tx: Sender<Vec<u8>>,
) -> Result<(), Error> {
    let req = HyperRequest::deserialize(req)?;
    let (parts, body) = client.request(req).await?.into_parts();

    // Wait for the entire response body to come in.
    let body = body::to_bytes(body).await?;

    let resp = (parts, body).serialize();
    resp_tx.send(resp).await.unwrap();
    Ok(())
}

/// HTTP client driver entry point.
pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {
        let client_clone = client.clone();
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(async move {
            match send_request(client_clone, req, resp_tx_clone).await {
                Err(_) => todo!("send error response"),
                _ => (),
            }
        });
    }
    println!("http client task exiting");
}
