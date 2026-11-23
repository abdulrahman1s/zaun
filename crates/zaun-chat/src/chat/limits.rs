//! Size and count limits (Protocol.hs 7.1.4 `maxEncodedMsgLength` and friends).

pub const MAX_ENCODED_MSG_LENGTH: usize = 15602;
pub const MAX_ENCODED_MSG_LENGTH_PQ: usize = 13380;
pub const MAX_FORWARD_BATCH_LENGTH: usize = MAX_ENCODED_MSG_LENGTH + 161;
/// Conn-info (x.info, x.contact, ...) limits: `e2eEncConnInfoLength - 2 - 256`, without and with PQ.
pub const MAX_CONN_INFO_LENGTH: usize = 14574;
pub const MAX_CONN_INFO_LENGTH_PQ: usize = 10848;
pub const MAX_ENCODED_PROFILE_MSG_LENGTH: usize = 16384;
pub const MAX_DECOMPRESSED_MSG_LENGTH: usize = 65536;
pub const MAX_BATCH_ELEMENT_COUNT: usize = 255;
pub const MAX_FWD_DEPTH: usize = 1;
/// Bodies up to this length stay as they are inside the `X` container (Compression.hs).
pub const MAX_LENGTH_PASSTHROUGH: usize = 180;
pub const COMPRESSION_LEVEL: i32 = 3;
