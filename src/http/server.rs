use noun::Noun;
use noun::convert;

use super::misc::HyperResponse;

// TODO: Should we name this Request if it contains a response?
enum Request {
    SetConfig(SetConfig),
    Response(HyperResponse),
    // TODO: bound?
}

#[derive(Debug)]
struct SetConfig {
    // TODO: 
}


