use io_drivers::{fs::fs_run, http::client::http_client_run, Status};
use simplelog::{Config, LevelFilter, WriteLogger};
use std::{env, fs::File};

fn main() -> Status {
    let mut args = env::args();
    if args.len() != 2 {
        return Status::NoDriver;
    }

    let driver = args.nth(1).unwrap_or(String::from("unknown"));
    if let Ok(log) = env::var("URBIT_IO_DRIVERS_LOG") {
        WriteLogger::init(
            LevelFilter::Debug,
            Config::default(),
            File::options()
                .create(true)
                .append(true)
                .open(log)
                .expect("create log file"),
        )
        .expect("initialize logger");
    }
    match &driver[..] {
        "fs" => fs_run(),
        "http-client" => http_client_run(),
        _ => Status::NoDriver,
    }
}
