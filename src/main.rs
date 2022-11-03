use io_drivers::{http::client::http_client_run, Status};
use std::env;

fn main() -> Status {
    let mut args = env::args();
    if args.len() != 2 {
        return Status::NoDriver;
    }

    let driver = args.nth(1).unwrap_or(String::from("unknown"));
    env_logger::init();
    match &driver[..] {
        "http-client" => http_client_run(),
        _ => Status::NoDriver,
    }
}
