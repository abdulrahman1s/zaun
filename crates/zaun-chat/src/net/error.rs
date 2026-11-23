//! Connection failures before TLS starts.
use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("{0} is a .onion host; Tor is not supported yet")]
    OnionUnsupported(String),
    #[error("cannot resolve {host}: {source}")]
    Resolve { host: String, source: io::Error },
    #[error("cannot connect to {host}:{port}: {source}")]
    Connect {
        host: String,
        port: u16,
        source: io::Error,
    },
}
