//! SMP client, protocol v15 to v23: addresses, commands, responses, handshake with block encryption, and a reconnecting connection.
//! Ported from simplexmq Protocol.hs, Transport.hs, Client.hs and nook src/sx/smp (AGPL-3.0-only).

mod address;
mod auth;
mod block;
mod clock;
mod commands;
mod config;
mod connection;
mod error;
mod events;
mod handshake;
mod hello;
mod message;
mod queue_data;
mod responses;
mod server_error;
mod signed_key;
mod tls_connector;
mod transmission;
mod transport;
mod version;

#[cfg(test)]
mod tests;

pub use address::{DEFAULT_PORT, SmpServer};
pub use auth::Authorizer;
pub use block::{RcvBlocks, SndBlocks, block_ciphers, max_payload};
pub use clock::{Clock, SystemClock};
pub use commands::{Command, NewQueue};
pub use config::ConnConfig;
pub use connection::{Answer, Request, SmpConnection};
pub use error::{HandshakeError, SmpError};
pub use events::{SessionInfo, SmpEvent, SmpEventKind};
pub use handshake::{HandshakeConfig, TlsFacts, client_handshake, negotiate};
pub use hello::{ServerHello, encode_client_hello};
pub use message::{RcvMsg, decrypt_message, parse_rcv_body, rcv_dh_secret};
pub use queue_data::{
    ContactLink, MessagingLink, NewNtfCreds, QueueLinkData, QueueMode, QueueReqData,
    SubscriptionMode,
};
pub use responses::{QueueIds, Response, ServerNtfCreds};
pub use server_error::{CommandError, ErrorType};
pub use tls_connector::TlsConnector;
pub use transmission::{Incoming, authorized, encode_transmission, pack, parse_payload};
pub use transport::{Closer, Connector, Established, Reader, Wire, Writer};
pub use version::{BLOCK_SIZE, MAX_VERSION, MIN_VERSION};
