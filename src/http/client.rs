use crate::{Request, Response};
use hyper::{
    body::{self, Bytes},
    client::Client,
    http::response::Parts,
    Body, Request as HyperRequest,
};
use tokio::sync::mpsc::{Receiver, Sender};

impl Request for HyperRequest<Body> {
    type Error = ();

    fn deserialize(_req: Vec<u8>) -> Result<Self, Self::Error> {
        todo!()
    }
}

type HyperResponse = (Parts, Bytes);

impl Response for HyperResponse {
    fn serialize(self) -> Vec<u8> {
        todo!()
    }
}

pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {
        let req = HyperRequest::deserialize(req);
        if let Err(_) = req {
            todo!("handle parse error");
        }
        let req = req.unwrap();

        let resp = client.request(req).await;
        if let Err(_) = resp {
            todo!("handle request error");
        }
        let (parts, body) = resp.unwrap().into_parts();

        // Wait for the entire response body to come in.
        let body = body::to_bytes(body).await;
        if let Err(_) = body {
            todo!("handle body error");
        }
        let body = body.unwrap();

        let resp = (parts, body).serialize();
        resp_tx.send(resp).await.unwrap();
    }
    println!("http client task exiting");
}
