//! A TLS 1.3 client for the SMP and XFTP transports (ADR-0042); ported from nook sx/tls (AGPL-3.0-only).

mod alert;
mod der;
mod error;
mod handshake;
mod identity;
mod info;
mod key_schedule;
mod reader;
mod record;
mod shared;
mod stream;
mod verify;
mod wire;
mod writer;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

pub use alert::Alert;
pub use der::{CertError, certificate_public_key};
pub use error::TlsError;
pub use identity::{ChainCertificates, IdentityError, chain_id_ca_certs, verify_server_chain};
pub use info::TlsInfo;
pub use reader::TlsReader;
pub use stream::TlsStream;
pub use writer::TlsWriter;
