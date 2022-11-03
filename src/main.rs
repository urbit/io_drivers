use io_drivers::{http::client::http_client_run, Status};
use simplelog::{Config, LevelFilter, WriteLogger};
use std::{env, fs::File};

/// File to write all logging statements to.
const LOG_FILE: &'static str = "io_drivers.log";

fn main() -> Status {
    let mut args = env::args();
    if args.len() != 2 {
        return Status::NoDriver;
    }

    let driver = args.nth(1).unwrap_or(String::from("unknown"));
    WriteLogger::init(
        LevelFilter::Debug,
        Config::default(),
        File::create(LOG_FILE).expect("create log file"),
    )
    .expect("initialize logger");
    match &driver[..] {
        "http-client" => http_client_run(),
        _ => Status::NoDriver,
    }
}
