//! A file description as the chat sends it (Description.hs `FileDescription`, `FileChunk`, `FileChunkReplica`, `RedirectFileInfo`).
use crate::xftp::address::XftpServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Recipient,
    Sender,
}

impl Party {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Recipient => "recipient",
            Self::Sender => "sender",
        }
    }
}

/// The replica of a redirect description (the real description is the file this one points at).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub size: u64,
    pub digest: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkReplica {
    pub server: XftpServer,
    /// The server's file id: a sender id in a sender description, else a recipient id.
    pub replica_id: Vec<u8>,
    /// PKCS#8 of the Ed25519 key that authorizes commands for `replica_id`.
    pub replica_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    pub chunk_no: u32,
    pub chunk_size: u32,
    /// SHA-256 of the chunk as stored.
    pub digest: Vec<u8>,
    pub replicas: Vec<ChunkReplica>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDescription {
    pub party: Party,
    /// Size of the encrypted file, the sum of its chunks.
    pub size: u64,
    /// SHA-512 of the encrypted file.
    pub digest: Vec<u8>,
    pub key: Vec<u8>,
    pub nonce: Vec<u8>,
    /// The usual chunk size; a chunk of another size says so in the description.
    pub chunk_size: u32,
    pub chunks: Vec<FileChunk>,
    pub redirect: Option<Redirect>,
}
