//! The SimpleX agent: link, envelope and message formats, and the connection layer on top of them (simplexmq 7.1.4 Agent.hs, Agent/Protocol.hs, Crypto/ShortLink.hs, AGPL-3.0-only).
mod e2e;
mod e2e_uri;
mod envelope;
mod error;
mod integrity;
mod kem;
mod keys;
mod limits;
mod ne;
mod query;
mod queue_info;
mod queue_mode;
mod queue_uri;
mod queue_uri_parse;
mod smp_server;
mod smp_server_uri;

pub mod client;
pub mod connection;
pub mod delivery;
pub mod links;
pub mod message;
pub mod records;
pub mod short_link;
#[cfg(test)]
mod testing;
#[cfg(test)]
pub(crate) mod tests;
pub mod version;

pub use crate::smp::SmpServer;
pub use e2e::{E2eParams, E2eParamsUri};
pub use envelope::AgentMsgEnvelope;
pub use error::{AgentError, Result};
pub use integrity::{MsgErrorType, MsgIntegrity, check_msg_integrity, msg_hash};
pub use kem::KemParams;
pub use limits::{
    E2E_ENC_CONFIRMATION_LENGTH, E2E_ENC_MESSAGE_LENGTH, FIXED_DATA_PADDED_LENGTH,
    USER_DATA_PADDED_LENGTH, e2e_enc_agent_msg_length, e2e_enc_conn_info_length,
};
pub use query::{url_decode, url_encode};
pub use queue_info::SmpQueueInfo;
pub use queue_mode::{QueueMode, sender_can_secure};
pub use queue_uri::{SmpQueueAddress, SmpQueueUri};
