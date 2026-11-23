//! Downloading a file from its description: fetch, check, decrypt into a folder (Agent.hs receive workers).
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::crypto::{CbNonce, SbKey, random_array};
use crate::encoding::b64_encode;
use crate::smp::{Clock, SystemClock};

use super::client::ClientConfig;
use super::description::FileDescription;
use super::download_fetch::{Fetch, fetch};
use super::error::XftpError;
use super::file_decrypt::decrypt_chunks;
use super::pool::Pool;
use super::progress::{CancelFlag, ProgressFn};
use super::safe_name::{claim_unique, safe_file_name};

pub struct DownloadParams<'a> {
    pub description: &'a FileDescription,
    /// Where chunks and the unfinished file live while downloading; cleaned up.
    pub work_dir: &'a Path,
    /// The folder the finished file is created in, under its own (made safe) name.
    pub dest_dir: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    /// The name the sender gave, as in the header; `path` has it made safe.
    pub file_name: String,
    pub size: u64,
}

struct Partial(PathBuf);

impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0); // gone after a move; otherwise an unfinished file
    }
}

/// Decrypts the fetched chunks of `fd` to a new file under `work_dir`.
fn decrypt_to_work(
    fd: &FileDescription,
    f: &Fetch<'_>,
    pool: &mut Pool,
    progress: &mut ProgressFn<'_>,
) -> Result<(Partial, super::file_header::FileHeader), XftpError> {
    fd.validate().map_err(XftpError::Description)?;
    let (key, nonce) = (SbKey::from_slice(&fd.key)?, CbNonce::from_slice(&fd.nonce)?);
    if fd.digest.len() != 64 {
        return Err(XftpError::Description(
            "the file digest must be 64 bytes".into(),
        ));
    }
    let chunks = fetch(f, pool, fd, progress)?;
    let out = Partial(
        f.work_dir
            .join(format!("{}.part", b64_encode(&random_array::<9>()?, false))),
    );
    let header = decrypt_chunks(
        &chunks.0,
        &key,
        &nonce,
        &out.0,
        CancelFlag(f.cancel),
        &mut |_| {},
    )?;
    Ok((out, header))
}

/// A redirect description is a small file holding the real description; its size and digest are promised by the outer one.
fn resolve_redirect(
    fd: &FileDescription,
    f: &Fetch<'_>,
    pool: &mut Pool,
) -> Result<FileDescription, XftpError> {
    let Some(redirect) = &fd.redirect else {
        return Ok(fd.clone());
    };
    let (part, _) = decrypt_to_work(fd, f, pool, &mut |_, _| {})?;
    let inner = FileDescription::parse_valid(&fs::read_to_string(&part.0)?)
        .map_err(XftpError::Description)?;
    if inner.redirect.is_some() || inner.size != redirect.size || inner.digest != redirect.digest {
        return Err(XftpError::Mismatch("redirected description"));
    }
    Ok(inner)
}

/// Blocks until the file is complete and verified: every chunk's SHA-256, the SHA-512 of the encrypted file and its tag.
/// The result is a new file in `dest_dir`; on any failure nothing is left there.
pub fn download(
    params: &DownloadParams<'_>,
    cancel: &AtomicBool,
    progress: &mut ProgressFn<'_>,
) -> Result<Downloaded, XftpError> {
    let clock = SystemClock::new();
    let f = Fetch {
        work_dir: params.work_dir,
        cancel,
        clock: &clock as &dyn Clock,
    };
    let mut pool = Pool::new(ClientConfig::default());
    let fd = resolve_redirect(params.description, &f, &mut pool)?;
    let (part, header) = decrypt_to_work(&fd, &f, &mut pool, progress)?;
    let size = fs::metadata(&part.0)?.len();
    let dest = claim_unique(params.dest_dir, &safe_file_name(&header.file_name))?;
    if let Err(e) = move_file(&part.0, &dest) {
        let _ = fs::remove_file(&dest); // the claimed name must not stay empty
        return Err(e.into());
    }
    Ok(Downloaded {
        path: dest,
        file_name: header.file_name,
        size,
    })
}

/// Rename, or copy when the folders are on different filesystems.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}
