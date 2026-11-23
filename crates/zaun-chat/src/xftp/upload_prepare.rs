//! The first half of an upload: encrypt the file into one file the size of its chunks, and hash it (Agent.hs `encryptFileForUpload`).
use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::{CbNonce, SbKey, random_array};
use crate::encoding::b64_encode;

use super::chunks::{
    ChunkSpec, FILE_SIZE_LEN, MAX_FILE_SIZE_HARD, prepare_chunk_sizes, prepare_chunk_specs,
};
use super::error::XftpError;
use super::file_digest::sha512_files;
use super::file_encrypt::{EncryptParams, encrypt_file};
use super::file_header::FileHeader;
use super::progress::CancelFlag;
use super::version::AUTH_TAG_SIZE;

/// An encrypted file that is deleted when this is dropped, however the upload ends.
pub(super) struct Prepared {
    pub(super) path: PathBuf,
    pub(super) key: SbKey,
    pub(super) nonce: CbNonce,
    pub(super) digest: Vec<u8>,
    pub(super) enc_size: u64,
    pub(super) specs: Vec<ChunkSpec>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path); // best effort: a stale temp file is harmless
    }
}

pub(super) fn prepare(
    source: &Path,
    file_name: &str,
    work_dir: &Path,
    cancel: CancelFlag<'_>,
) -> Result<Prepared, XftpError> {
    let content_len = fs::metadata(source)?.len();
    let header = FileHeader {
        file_name: file_name.to_owned(),
        file_extra: None,
    }
    .encode()?;
    let payload = header.len() as u64 + content_len;
    if payload > MAX_FILE_SIZE_HARD {
        return Err(XftpError::File(
            "the file is larger than XFTP allows".into(),
        ));
    }
    let sizes = prepare_chunk_sizes(payload + FILE_SIZE_LEN + AUTH_TAG_SIZE as u64);
    let enc_size = sizes.iter().map(|s| u64::from(*s)).sum();
    let (key, nonce) = (SbKey::random()?, CbNonce::random()?);
    fs::create_dir_all(work_dir)?;
    let path = work_dir.join(format!(
        "{}.xftp-enc",
        b64_encode(&random_array::<9>()?, false)
    ));
    // Built before the file exists, so its Drop also cleans up after a failed encryption.
    let mut prepared = Prepared {
        path,
        key,
        nonce,
        digest: Vec::new(),
        enc_size,
        specs: prepare_chunk_specs(&sizes),
    };
    let params = EncryptParams {
        src: source,
        header: &header,
        key: &prepared.key,
        nonce: &prepared.nonce,
        content_len,
        enc_size,
        dst: &prepared.path,
    };
    encrypt_file(&params, cancel, &mut |_| {})?;
    prepared.digest = sha512_files(&[&prepared.path])?;
    Ok(prepared)
}
