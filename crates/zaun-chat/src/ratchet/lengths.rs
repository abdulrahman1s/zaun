//! Padded sizes: the header per PQ support, and the agent's message bodies (Ratchet.hs `paddedHeaderLen`, `fullHeaderLen`; Agent/Protocol.hs `e2eEncConnInfoLength`, `e2eEncAgentMsgLength`).
use super::pq::PqSupport;

pub const IV_LEN: usize = 16;
pub const AUTH_TAG_LEN: usize = 16;

/// Room is kept for header growth: the actual header is 69 bytes, or 2288 with a KEM key and ciphertext.
#[must_use]
pub fn padded_header_len(pq: PqSupport) -> usize {
    if pq.0 { 2310 } else { 88 }
}

/// Version, a length byte, the padded header, tag and IV; the wire adds 2 length bytes and the message tag.
#[must_use]
pub fn full_header_len(pq: PqSupport) -> usize {
    2 + 1 + padded_header_len(pq) + AUTH_TAG_LEN + IV_LEN
}

/// The padded length of the encrypted connection info (confirmation, invitation).
#[must_use]
pub fn e2e_enc_conn_info_length(pq: PqSupport) -> usize {
    if pq.0 { 11106 } else { 14832 }
}

/// The padded length of encrypted agent messages.
#[must_use]
pub fn e2e_enc_agent_msg_length(pq: PqSupport) -> usize {
    if pq.0 { 13618 } else { 15840 }
}
