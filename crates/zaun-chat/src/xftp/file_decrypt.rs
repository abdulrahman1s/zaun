//! Decrypting the downloaded chunks, concatenated, into the user's file; the tag is checked at the end (Crypto.hs `decryptChunks`).
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use subtle::ConstantTimeEq;

use crate::crypto::{CbNonce, SbKey, SbState};

use super::error::XftpError;
use super::file_header::FileHeader;
use super::progress::CancelFlag;
use super::version::AUTH_TAG_SIZE;

const BLOCK: usize = 64 * 1024;
const PREFIX: usize = 8 + 1024;

/// The decrypted stream: the length, then the header, then the content (`declared` minus the header), then padding.
struct Plain {
    prefix: Vec<u8>,
    header: Option<FileHeader>,
    declared: u64,
    content_left: u64,
    out: BufWriter<File>,
}

impl Plain {
    fn push(&mut self, bytes: &[u8]) -> Result<(), XftpError> {
        if self.header.is_some() {
            return self.write_content(bytes);
        }
        self.prefix.extend_from_slice(bytes);
        if self.prefix.len() >= PREFIX {
            self.start()
        } else {
            Ok(())
        }
    }

    fn write_content(&mut self, bytes: &[u8]) -> Result<(), XftpError> {
        let n = usize::try_from(self.content_left).map_or(bytes.len(), |l| l.min(bytes.len()));
        self.out.write_all(&bytes[..n])?;
        self.content_left -= n as u64;
        Ok(())
    }

    /// Reads the declared length and header off the front once enough bytes are here.
    fn start(&mut self) -> Result<(), XftpError> {
        let (len, rest) = self
            .prefix
            .split_first_chunk::<8>()
            .ok_or(XftpError::Mismatch("file length"))?;
        self.declared = u64::try_from(i64::from_be_bytes(*len))
            .map_err(|_| XftpError::Mismatch("file length"))?;
        let (header, used) =
            FileHeader::parse(rest).map_err(|_| XftpError::Mismatch("file header"))?;
        self.content_left = self
            .declared
            .checked_sub(used as u64)
            .ok_or(XftpError::Mismatch("file length"))?;
        let rest = rest[used..].to_vec();
        self.header = Some(header);
        self.prefix.clear();
        self.write_content(&rest)
    }

    fn finish(mut self) -> Result<FileHeader, XftpError> {
        if self.header.is_none() {
            self.start()?;
        }
        if self.content_left != 0 {
            return Err(XftpError::Mismatch("file length"));
        }
        self.out.flush()?;
        self.header.ok_or(XftpError::Mismatch("file header"))
    }
}

/// Decrypts `chunks` (in order, together `enc_size` bytes) into `dst`. A bad tag or length removes `dst`.
pub fn decrypt_chunks(
    chunks: &[PathBuf],
    key: &SbKey,
    nonce: &CbNonce,
    dst: &Path,
    cancel: CancelFlag<'_>,
    progress: &mut dyn FnMut(u64),
) -> Result<FileHeader, XftpError> {
    let result = decrypt_into(chunks, key, nonce, dst, cancel, progress);
    if result.is_err() {
        let _ = std::fs::remove_file(dst); // a partial or unauthenticated file must not stay
    }
    result
}

fn decrypt_into(
    chunks: &[PathBuf],
    key: &SbKey,
    nonce: &CbNonce,
    dst: &Path,
    cancel: CancelFlag<'_>,
    progress: &mut dyn FnMut(u64),
) -> Result<FileHeader, XftpError> {
    let sizes = chunks
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()))
        .collect::<Result<Vec<_>, _>>()?;
    let plain_len = sizes
        .iter()
        .sum::<u64>()
        .checked_sub(AUTH_TAG_SIZE as u64)
        .ok_or(XftpError::Mismatch("file size"))?;
    let mut plain = Plain {
        prefix: Vec::new(),
        header: None,
        declared: 0,
        content_left: 0,
        out: BufWriter::new(File::create(dst)?),
    };
    let mut state = SbState::new(key, nonce);
    let (mut done, mut tag) = (0u64, Vec::with_capacity(AUTH_TAG_SIZE));
    let mut buf = vec![0u8; BLOCK];
    for path in chunks {
        let mut file = File::open(path)?;
        loop {
            cancel.check()?;
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let cipher = usize::try_from(plain_len - done).map_or(n, |left| left.min(n));
            state.decrypt(&mut buf[..cipher]);
            plain.push(&buf[..cipher])?;
            tag.extend_from_slice(&buf[cipher..n]);
            done += cipher as u64;
            progress(done);
        }
    }
    let expected = state.tag();
    if tag.len() != AUTH_TAG_SIZE || !bool::from(tag.ct_eq(&expected)) {
        return Err(XftpError::Mismatch("file authentication tag"));
    }
    plain.finish()
}
