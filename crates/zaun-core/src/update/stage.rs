//! Download an owned install's files, verify and unpack them, and stage them next to the
//! binary (ADR-0040 §5). Nothing outside `.update/` changes here.

use std::io::Read as _;
use std::path::Path;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use super::UpdateError;
use super::install::{Layout, file_name};
use super::manifest::{Artifact, Release};
use super::source::{MANIFEST_NAME, SIGNATURE_NAME, TARBALL_CHANNEL};
use crate::artwork::http::{HttpClient, HttpError};
use crate::fsx::{Fs, FsErrorKind};

/// Hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download, verify and stage `release` for `arch`. Replaces an earlier staged update.
pub fn download(
    http: &HttpClient,
    fs: &dyn Fs,
    layout: &Layout,
    release: &Release,
    arch: &str,
) -> Result<(), UpdateError> {
    let files = release.files(TARBALL_CHANNEL, arch)?;
    let staging = layout.staging();
    clear_dir(fs, &staging)?;
    fs.create_dir_all(&staging, 0o755)?;
    for a in files {
        let data = fetch(http, a)?;
        fs.write_new(&staging.join(file_name(&a.path)), &data, 0o755)?;
        fs.set_mode(&staging.join(file_name(&a.path)), 0o755)?;
    }
    fs.write_new(&staging.join(MANIFEST_NAME), &release.manifest, 0o644)?;
    fs.write_new(&staging.join(SIGNATURE_NAME), &release.signature, 0o644)?;
    clear_dir(fs, &layout.staged())?;
    fs.rename(&staging, &layout.staged())?;
    Ok(())
}

/// One file: downloaded at most its signed size, hashed, unpacked, hashed again.
fn fetch(http: &HttpClient, a: &Artifact) -> Result<Vec<u8>, UpdateError> {
    let name = file_name(&a.path);
    let r = http.get_capped(&a.url, a.size).map_err(|e| match e {
        HttpError::TooLarge => UpdateError::bad_file(name, "larger than signed"),
        other => other.into(),
    })?;
    if r.status != 200 {
        return Err(UpdateError::Unreachable(format!(
            "{name}: status {}",
            r.status
        )));
    }
    if r.body.len() as u64 != a.size || sha256_hex(&r.body) != a.sha256 {
        return Err(UpdateError::bad_file(
            name,
            "download does not match its sha256",
        ));
    }
    let mut out = Vec::new();
    GzDecoder::new(r.body.as_slice())
        .take(a.unpacked_size + 1)
        .read_to_end(&mut out)
        .map_err(|e| UpdateError::bad_file(name, format!("gzip: {e}")))?;
    if out.len() as u64 != a.unpacked_size || sha256_hex(&out) != a.unpacked_sha256 {
        return Err(UpdateError::bad_file(
            name,
            "unpacked file does not match its sha256",
        ));
    }
    Ok(out)
}

/// Remove a folder of plain files (`.update/*` holds no subfolders); missing is fine.
pub(super) fn clear_dir(fs: &dyn Fs, dir: &Path) -> Result<(), UpdateError> {
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for e in entries {
        fs.remove_file(&dir.join(&e.name))?;
    }
    fs.remove_dir(dir)?;
    Ok(())
}
