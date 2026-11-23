//! Padded lengths of the agent's encrypted bodies (Protocol.hs `e2eEncConfirmationLength`, Agent/Protocol.hs `e2eEncConnInfoLength`; simplexmq 7.1.4, AGPL-3.0-only).

/// A per-queue encrypted confirmation, which carries the sender's DH key in its public header.
pub const E2E_ENC_CONFIRMATION_LENGTH: usize = 15904;
/// A per-queue encrypted message.
pub const E2E_ENC_MESSAGE_LENGTH: usize = 16000;

/// The ratchet-padded connection info inside a confirmation.
#[must_use]
pub const fn e2e_enc_conn_info_length(pq: bool) -> usize {
    if pq { 11106 } else { 14832 }
}

/// The ratchet-padded agent message.
#[must_use]
pub const fn e2e_enc_agent_msg_length(pq: bool) -> usize {
    if pq { 13618 } else { 15840 }
}

/// Padded length of the encrypted fixed link data (`2048 - 24 - 16`).
pub const FIXED_DATA_PADDED_LENGTH: usize = 2008;
/// Padded length of the encrypted user link data (`13824 - 24 - 16`).
pub const USER_DATA_PADDED_LENGTH: usize = 13784;
