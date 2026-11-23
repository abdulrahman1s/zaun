//! Transport blocks: padded, or sbc-chain encrypted when the client sent its DH key (Transport.hs `tPutBlock`, `tGetBlock`, `blockEncryption`).
//!
//! Each direction has its own chain key, advanced once per block, so exactly one thread seals and one opens.
use std::io::Result as IoResult;

use crate::crypto::{DhSecret, SbKey, pad, sb_decrypt, sb_encrypt, sbc_hkdf, sbc_init, unpad};

use super::error::{HandshakeError, SmpError};
use super::transport::{Reader, Writer};
use super::version::{BLOCK_SIZE, BOX_OVERHEAD};

/// Largest payload a block can carry: the padded length less the 2-byte length prefix.
#[must_use]
pub const fn max_payload(encrypted: bool) -> usize {
    (if encrypted {
        BLOCK_SIZE - BOX_OVERHEAD
    } else {
        BLOCK_SIZE
    }) - 2
}

pub struct SndBlocks(Option<SbKey>);

pub struct RcvBlocks(Option<SbKey>);

/// The two directions of one session. `sbcInit` gives (a, b); the server sends with a, the client with b.
pub fn block_ciphers(
    session_id: &[u8],
    secret: Option<&DhSecret>,
) -> Result<(SndBlocks, RcvBlocks), SmpError> {
    let Some(secret) = secret else {
        return Ok((SndBlocks(None), RcvBlocks(None)));
    };
    let (server_snd, client_snd) = sbc_init(session_id, secret.as_bytes())?;
    Ok((SndBlocks(Some(client_snd)), RcvBlocks(Some(server_snd))))
}

impl SndBlocks {
    #[must_use]
    pub fn plain() -> Self {
        Self(None)
    }

    #[must_use]
    pub fn encrypted(&self) -> bool {
        self.0.is_some()
    }

    /// Pads `payload` to a whole block and, when encrypting, boxes it with the next chain key.
    pub fn seal(&mut self, payload: &[u8]) -> Result<Vec<u8>, SmpError> {
        let Some(chain) = &mut self.0 else {
            return pad(payload, BLOCK_SIZE).map_err(|_| SmpError::LargeMsg);
        };
        let (key, nonce, next) = sbc_hkdf(chain)?;
        *chain = next;
        sb_encrypt(&key, &nonce, payload, BLOCK_SIZE - BOX_OVERHEAD).map_err(|_| SmpError::LargeMsg)
    }

    pub fn write(&mut self, w: &mut dyn Writer, payload: &[u8]) -> Result<(), SmpError> {
        let block = self.seal(payload)?;
        Ok(w.write_all(&block)?)
    }
}

impl RcvBlocks {
    #[must_use]
    pub fn plain() -> Self {
        Self(None)
    }

    pub fn open(&mut self, block: &[u8]) -> Result<Vec<u8>, SmpError> {
        let Some(chain) = &mut self.0 else {
            return unpad(block)
                .map(<[u8]>::to_vec)
                .map_err(|_| HandshakeError::Block.into());
        };
        let (key, nonce, next) = sbc_hkdf(chain)?;
        *chain = next;
        sb_decrypt(&key, &nonce, block).map_err(|_| HandshakeError::Block.into())
    }

    pub fn read(&mut self, r: &mut dyn Reader) -> Result<Vec<u8>, SmpError> {
        let mut block = vec![0u8; BLOCK_SIZE];
        let read: IoResult<()> = r.read_exact(&mut block);
        read?;
        self.open(&block)
    }
}
