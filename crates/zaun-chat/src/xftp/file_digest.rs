//! SHA-512 of a file read in blocks (Crypto.hs `sha512HashChunks`).
//!
//! TODO(crypto): a streaming hash belongs in `crate::crypto::hash`.
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha512};

use super::error::XftpError;

pub fn sha512_files<P: AsRef<Path>>(paths: &[P]) -> Result<Vec<u8>, XftpError> {
    let mut hash = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    for path in paths {
        let mut file = File::open(path)?;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
    }
    Ok(hash.finalize().to_vec())
}
