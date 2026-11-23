//! Downloading one chunk: FGET, then the server's transport encryption off the stream (Client.hs `downloadXFTPChunk`).
use std::io::{Read, Write};

use subtle::ConstantTimeEq;

use crate::crypto::{Alg, CbNonce, KeyPair, PrivateKey, PublicKey, SbKey, SbState, dh, sha256};

use super::client::{Ctl, XftpClient};
use super::commands::Command;
use super::error::XftpError;
use super::ops::unexpected;
use super::version::AUTH_TAG_SIZE;

impl<T: Read + Write> XftpClient<T> {
    /// Fetches a chunk: agrees a key with the server, decrypts the stream, checks its tag and its SHA-256.
    pub fn download_chunk(
        &mut self,
        recipient_key: &PrivateKey,
        file_id: &[u8],
        chunk_size: u32,
        digest: &[u8],
        ctl: &mut Ctl<'_>,
    ) -> Result<Vec<u8>, XftpError> {
        let ours = KeyPair::generate(Alg::X25519)?;
        let size = usize::try_from(chunk_size).map_err(|_| XftpError::Unexpected("chunk size"))?;
        let (response, mut body) = self.call(
            Some(recipient_key),
            file_id,
            &Command::Get {
                rcv_dh_key: ours.public.clone(),
            },
            &[],
            size + AUTH_TAG_SIZE,
            ctl,
        )?;
        let (dh_key, nonce) = response.into_file().ok_or_else(unexpected)?;
        if body.len() != size + AUTH_TAG_SIZE {
            return Err(XftpError::Mismatch("chunk size"));
        }
        let tag = body.split_off(size);
        open_chunk(&ours.private, &dh_key, &nonce, &mut body, &tag)?;
        if !bool::from(sha256(&body).as_slice().ct_eq(digest)) {
            return Err(XftpError::Mismatch("chunk digest"));
        }
        Ok(body)
    }
}

/// `cbInit` and `sbDecryptChunk` over a whole chunk, then the tag (Lazy.hs; the stream is the box of the DH secret).
fn open_chunk(
    own: &PrivateKey,
    server_key: &PublicKey,
    nonce: &CbNonce,
    body: &mut [u8],
    tag: &[u8],
) -> Result<(), XftpError> {
    let secret = dh(server_key, own)?;
    if bool::from(secret.as_bytes().ct_eq(&[0u8; 32])) {
        return Err(XftpError::Mismatch("transport key"));
    }
    let mut state = SbState::new(&SbKey::from_slice(secret.as_bytes())?, nonce);
    state.decrypt(body);
    if bool::from(state.tag().ct_eq(tag)) {
        Ok(())
    } else {
        Err(XftpError::Mismatch("transport authentication tag"))
    }
}
