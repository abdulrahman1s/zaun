//! Integration tests of the native engine's encoding and crypto modules.
#![allow(clippy::unwrap_used, reason = "tests")]

mod agent_golden;
mod agent_links;
mod agent_messages;
mod agent_short_link;
mod agent_support;
mod chat_batch;
mod chat_compress;
mod chat_golden_grp;
mod chat_golden_msg;
mod chat_nook;
mod chat_roundtrip;
mod chat_support;
mod crypto;
mod encoding;
mod engine_live;
mod engine_pipe;
mod h2_connection;
mod h2_hpack;
mod h2_support;
mod hex;
mod ratchet;
mod vectors;
mod xftp_description;
mod xftp_file;
mod xftp_live;
mod xftp_protocol;
