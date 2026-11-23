//! Agent messages inside the ratchet (Agent/Protocol.hs, simplexmq 7.1.4, AGPL-3.0-only).
mod agent_message;
mod amessage;
mod message_type;
mod priv_header;
mod receipt;
mod snd_addr;

pub use agent_message::{AgentMessage, RatchetInfo, RequestSignature};
pub use amessage::AMessage;
pub use message_type::AgentMessageType;
pub use priv_header::APrivHeader;
pub use receipt::AMessageReceipt;
pub use snd_addr::SndQAddr;
