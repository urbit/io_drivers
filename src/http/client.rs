use hyper::client::Client;
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run(mut req_rx: Receiver<Vec<u8>>, resp_tx: Sender<Vec<u8>>) {
    let client = Client::new();

    while let Some(req) = req_rx.recv().await {}
}
