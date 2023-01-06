// TODO: Rename from misc to something more specific??

use hyper::{http::response::Parts, body::Bytes, header};
use noun::{Noun, Rc, Atom, Cell};

/// A response to an HTTP request.
#[derive(Debug)]
pub struct HyperResponse {
    pub req_num: u64,
    pub parts: Parts,
    pub body: Bytes,
}

impl TryFrom<HyperResponse> for Noun {
    type Error = header::ToStrError;

    /// The resulting noun is:
    ///
    /// ```text
    /// [
    ///   <req_num>
    ///   <status>
    ///   <headers>
    ///   <body>
    /// ]
    /// ```
    fn try_from(resp: HyperResponse) -> Result<Self, Self::Error> {
        let req_num = Rc::<Noun>::from(Atom::from(resp.req_num));
        let status = Rc::<Noun>::from(Atom::from(resp.parts.status.as_u16()));
        let null = Rc::<Noun>::from(Atom::null());

        let headers = {
            let mut headers_cell = null.clone();
            let headers = &resp.parts.headers;
            for key in headers.keys().map(|k| k.as_str()) {
                let vals = headers.get_all(key);
                let key = Rc::<Noun>::from(Atom::from(key));
                for val in vals {
                    let val = Rc::<Noun>::from(Atom::from(val.to_str()?));
                    headers_cell = Rc::<Noun>::from(Cell::from([
                        Rc::<Noun>::from(Cell::from([key.clone(), val])),
                        headers_cell,
                    ]));
                }
            }
            headers_cell
        };

        let body = {
            let body = resp.body.to_vec();
            if body.is_empty() {
                null
            } else {
                let body_len = Atom::from(body.len());
                let body = Atom::from(body);
                Rc::<Noun>::from(Cell::from([
                    null,
                    Rc::<Noun>::from(Cell::from([body_len, body])),
                ]))
            }
        };

        Ok(Noun::from(Cell::from([req_num, status, headers, body])))
    }
}