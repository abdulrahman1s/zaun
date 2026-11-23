//! The structural limits of a description (Description.hs `validateFileDescription`); messages are Haskell's.
use crate::xftp::chunks::{CHUNK_SIZE_3, SERVER_CHUNK_SIZES};

use super::model::FileDescription;

const MAX_REPLICAS_PER_CHUNK: usize = 16;
const MAX_REPLICAS: usize = 131_072;

impl FileDescription {
    pub fn validate(&self) -> Result<(), String> {
        let counts = self.chunks.iter().map(|c| c.replicas.len());
        if !self.chunks.iter().zip(1u32..).all(|(c, n)| c.chunk_no == n) {
            return Err("chunk numbers are not sequential".into());
        }
        if self
            .chunks
            .iter()
            .map(|c| u64::from(c.chunk_size))
            .sum::<u64>()
            != self.size
        {
            return Err("chunks total size is different than file size".into());
        }
        if self
            .chunks
            .iter()
            .any(|c| !SERVER_CHUNK_SIZES.contains(&c.chunk_size))
        {
            return Err("incorrect chunk size".into());
        }
        if counts.clone().any(|n| n > MAX_REPLICAS_PER_CHUNK) {
            return Err("too many chunk replicas".into());
        }
        if counts.sum::<usize>() > MAX_REPLICAS {
            return Err("too many file replicas".into());
        }
        if self.redirect.is_some() && self.size > u64::from(CHUNK_SIZE_3) {
            return Err("redirect file description is too large".into());
        }
        Ok(())
    }
}
