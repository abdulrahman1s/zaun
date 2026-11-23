//! Encrypting a file for upload: `Int64 length | header | content | '#' padding | tag` under one secretbox stream (Crypto.hs `encryptFile`).
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use crate::crypto::{CbNonce, SbKey, SbState};

use super::error::XftpError;
use super::progress::CancelFlag;
use super::version::AUTH_TAG_SIZE;

const BLOCK: usize = 64 * 1024;
const FILL: u8 = b'#';

struct Sealer<W: Write> {
    state: SbState,
    out: BufWriter<W>,
    written: u64,
}

impl<W: Write> Sealer<W> {
    fn put(&mut self, buf: &mut [u8]) -> Result<(), XftpError> {
        self.state.encrypt(buf);
        self.out.write_all(buf)?;
        self.written += buf.len() as u64;
        Ok(())
    }
}

pub struct EncryptParams<'a> {
    pub src: &'a Path,
    /// The encoded `FileHeader`.
    pub header: &'a [u8],
    pub key: &'a SbKey,
    pub nonce: &'a CbNonce,
    /// The length of `src`.
    pub content_len: u64,
    /// The size of the output, the sum of its chunks.
    pub enc_size: u64,
    pub dst: &'a Path,
}

/// Writes the encrypted file to `p.dst`: exactly `p.enc_size` bytes. `progress` gets the plaintext bytes read so far.
pub fn encrypt_file(
    p: &EncryptParams<'_>,
    cancel: CancelFlag<'_>,
    progress: &mut dyn FnMut(u64),
) -> Result<(), XftpError> {
    let EncryptParams {
        src,
        header,
        key,
        nonce,
        content_len,
        enc_size,
        dst,
    } = *p;
    let file_size = header.len() as u64 + content_len;
    let pad_len = enc_size
        .checked_sub(AUTH_TAG_SIZE as u64 + 8 + file_size)
        .ok_or_else(|| XftpError::File("encrypted size is too small for the file".into()))?;
    let mut sealer = Sealer {
        state: SbState::new(key, nonce),
        out: BufWriter::new(File::create(dst)?),
        written: 0,
    };
    let mut first = [
        &i64::try_from(file_size)
            .map_err(|_| XftpError::File("file too large".into()))?
            .to_be_bytes()[..],
        header,
    ]
    .concat();
    sealer.put(&mut first)?;
    let mut reader = File::open(src)?;
    let mut buf = vec![0u8; BLOCK];
    let mut left = content_len;
    while left > 0 {
        cancel.check()?;
        let n = usize::try_from(left).map_or(BLOCK, |l| l.min(BLOCK));
        reader
            .read_exact(&mut buf[..n])
            .map_err(|_| XftpError::File("the file changed while it was encrypted".into()))?;
        sealer.put(&mut buf[..n])?;
        left -= n as u64;
        progress(content_len - left);
    }
    let mut left = pad_len;
    while left > 0 {
        let n = usize::try_from(left).map_or(BLOCK, |l| l.min(BLOCK));
        buf[..n].fill(FILL);
        sealer.put(&mut buf[..n])?;
        left -= n as u64;
    }
    let Sealer {
        state,
        mut out,
        written,
    } = sealer;
    out.write_all(&state.tag())?;
    out.flush()?;
    debug_assert_eq!(written + AUTH_TAG_SIZE as u64, enc_size);
    Ok(())
}
