//! SMP versions this client speaks and the sizes the wire fixes (Transport.hs, simplexmq 7.1.4).

pub const MIN_VERSION: u16 = 15;
pub const MAX_VERSION: u16 = 23;
pub const SERVICE_CERTS: u16 = 16;
pub const NEW_NTF_CREDS: u16 = 17;
pub const SERVER_INFO: u16 = 21;

/// `smpBlockSize`.
pub const BLOCK_SIZE: usize = 16384;
/// The secretbox tag a block-encrypted block carries on top of its padded plaintext.
pub const BOX_OVERHEAD: usize = 16;
