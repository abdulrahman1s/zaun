//! The first half of a download: the chunks of a description into files, checked against their digests (Agent.hs `downloadFileChunk`, `decryptFile`).
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use subtle::ConstantTimeEq;

use crate::crypto::{PrivateKey, random_array};
use crate::encoding::b64_encode;
use crate::h2::Progress;
use crate::smp::Clock;

use super::client::Ctl;
use super::description::{FileChunk, FileDescription, Party};
use super::error::XftpError;
use super::file_digest::sha512_files;
use super::pool::Pool;
use super::version::BLOCK_SIZE;

const ATTEMPTS: usize = 3;

/// Chunk files that are deleted when this is dropped.
pub(super) struct ChunkFiles(pub(super) Vec<PathBuf>);

impl Drop for ChunkFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = fs::remove_file(p); // best effort: a stale temp file is harmless
        }
    }
}

pub(super) struct Fetch<'a> {
    pub(super) work_dir: &'a Path,
    pub(super) cancel: &'a AtomicBool,
    pub(super) clock: &'a dyn Clock,
}

/// Each replica in turn until one answers; a missing or refused chunk is tried elsewhere, a cancel is not.
fn fetch_chunk(
    f: &Fetch<'_>,
    pool: &mut Pool,
    chunk: &FileChunk,
    progress: &mut dyn FnMut(usize),
) -> Result<Vec<u8>, XftpError> {
    let mut last = XftpError::Description(format!("chunk {} has no replicas", chunk.chunk_no));
    for r in &chunk.replicas {
        let key = PrivateKey::from_der(&r.replica_key)
            .map_err(|_| XftpError::Description("bad replica key".into()))?;
        let got = pool.run(&r.server, f.cancel, ATTEMPTS, |c| {
            let mut on_progress = |p: Progress| progress(p.received.saturating_sub(BLOCK_SIZE));
            let mut ctl = Ctl {
                cancel: f.cancel,
                clock: f.clock,
                on_progress: &mut on_progress,
            };
            c.download_chunk(
                &key,
                &r.replica_id,
                chunk.chunk_size,
                &chunk.digest,
                &mut ctl,
            )
        });
        match got {
            Ok(bytes) => return Ok(bytes),
            Err(XftpError::Cancelled) => return Err(XftpError::Cancelled),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Downloads every chunk to a file under `work_dir`, then checks the total size and the SHA-512 of the encrypted file.
pub(super) fn fetch(
    f: &Fetch<'_>,
    pool: &mut Pool,
    fd: &FileDescription,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<ChunkFiles, XftpError> {
    if fd.party != Party::Recipient {
        return Err(XftpError::Description(
            "a recipient description is needed to download".into(),
        ));
    }
    fs::create_dir_all(f.work_dir)?;
    let token = b64_encode(&random_array::<9>()?, false);
    let mut files = ChunkFiles(Vec::new());
    let mut done = 0u64;
    progress(0, fd.size);
    for chunk in &fd.chunks {
        let bytes = fetch_chunk(f, pool, chunk, &mut |n| progress(done + n as u64, fd.size))?;
        let path = f.work_dir.join(format!("{token}.chunk{}", chunk.chunk_no));
        fs::write(&path, &bytes)?;
        files.0.push(path);
        done += bytes.len() as u64;
        progress(done, fd.size);
    }
    if done != fd.size {
        return Err(XftpError::Mismatch("file size"));
    }
    if !bool::from(sha512_files(&files.0)?.as_slice().ct_eq(&fd.digest)) {
        return Err(XftpError::Mismatch("file digest"));
    }
    Ok(files)
}
