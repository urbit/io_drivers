fn main() -> io_drivers::Status {
    env_logger::init();
    io_drivers::http::client::http_client_run()
}
