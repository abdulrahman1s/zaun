//! The XFTP file transfer client: protocol 1 to 4 over HTTP/2 over our TLS, descriptions and file encryption.
//! Ported from simplexmq FileTransfer/*.hs and nook src/sx/xftp (AGPL-3.0-only).

mod address;
mod chunks;
mod client;
mod commands;
mod connect;
mod delete;
pub mod description;
mod download;
mod download_fetch;
mod error;
mod error_type;
mod file_decrypt;
mod file_digest;
mod file_encrypt;
mod file_header;
mod handshake;
mod handshake_msgs;
mod ops;
mod ops_download;
mod pool;
mod progress;
mod responses;
mod safe_name;
mod servers;
mod signed_key;
mod transmission;
mod upload;
mod upload_chunk;
mod upload_descr;
mod upload_prepare;
mod version;

pub use address::XftpServer;
pub use chunks::{
    CHUNK_SIZE_0, CHUNK_SIZE_1, CHUNK_SIZE_2, CHUNK_SIZE_3, ChunkSpec, FILE_SIZE_LEN, GB, KB,
    MAX_FILE_SIZE, MAX_FILE_SIZE_HARD, MB, SERVER_CHUNK_SIZES, encrypted_size, prepare_chunk_sizes,
    prepare_chunk_specs, single_chunk_size,
};
pub use client::{ClientConfig, Ctl, XftpClient};
pub use commands::{Command, FileInfo};
pub use connect::{TcpXftpClient, connect};
pub use delete::delete_file;
pub use download::{DownloadParams, Downloaded, download};
pub use error::{HandshakeError, XftpError};
pub use error_type::{CommandError, XftpErrorType};
pub use file_decrypt::decrypt_chunks;
pub use file_encrypt::{EncryptParams, encrypt_file};
pub use file_header::FileHeader;
pub use handshake::{Negotiated, Patience, TlsFacts, client_handshake};
pub use handshake_msgs::{ServerHandshake, encode_client_handshake};
pub use ops::NewChunk;
pub use progress::{CancelFlag, ProgressFn};
pub use responses::Response;
pub use safe_name::{claim_unique, safe_file_name};
pub use servers::{FLUX, SIMPLEX, preset_servers};
pub use transmission::{decode_transmission, encode_transmission};
pub use upload::{UploadParams, Uploaded, upload};
pub use version::{
    ALPN, AUTH_TAG_SIZE, BLOCK_SIZE, BLOCKED_FILES, DEFAULT_PORT, FILE_STORAGE_TIME, MAX_VERSION,
    MIN_VERSION,
};
