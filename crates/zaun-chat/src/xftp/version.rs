//! XFTP versions and sizes (FileTransfer/Transport.hs, Protocol.hs, simplexmq 7.1.4).

pub const MIN_VERSION: u16 = 1;
/// `blockedFilesXFTPVersion`: `ERR BLOCKED` exists from here.
pub const BLOCKED_FILES: u16 = 3;
/// `fileStorageTimeXFTPVersion`: FNEW and SIDS carry a storage time, the handshakes an entitlement and server info.
pub const FILE_STORAGE_TIME: u16 = 4;
pub const MAX_VERSION: u16 = 4;

/// `xftpBlockSize`: every command and response is one padded block.
pub const BLOCK_SIZE: usize = 16384;
pub const ALPN: &str = "xftp/1";
pub const DEFAULT_PORT: u16 = 443;
/// The secretbox tag after an encrypted chunk or file.
pub const AUTH_TAG_SIZE: usize = 16;
