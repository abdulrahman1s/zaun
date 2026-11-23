//! What goes through the command queue to the writer thread.
use crate::crypto::PrivateKey;
use crate::smp::commands::Command;
use crate::smp::error::SmpError;

use super::pending::Reply;

/// One command to send. Authorisation happens in the writer, against the session it actually goes out on.
#[derive(Debug, Clone)]
pub struct Request {
    pub command: Command,
    /// Queue id the command acts on; empty for NEW and PING.
    pub entity_id: Vec<u8>,
    /// Signs (Ed25519/Ed448) or authenticates (X25519) the command; `None` for PING, LGET and an unsecured SEND.
    pub auth_key: Option<PrivateKey>,
    /// A 24-byte correlation id; random when `None`. Short-link NEW derives the sender id from it.
    pub corr_id: Option<Vec<u8>>,
}

impl Request {
    #[must_use]
    pub fn new(command: Command, entity_id: Vec<u8>, auth_key: Option<PrivateKey>) -> Self {
        Self {
            command,
            entity_id,
            auth_key,
            corr_id: None,
        }
    }
}

pub struct Outgoing {
    pub request: Request,
    pub reply: Reply,
}

pub enum Ctl {
    Close,
    /// The reader of session `epoch` stopped.
    ReaderDone {
        epoch: u64,
        err: SmpError,
    },
}
