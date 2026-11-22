//! Download a release's bundle, verify and unpack it into a new version folder, and mark it
//! staged (ADR-0041 §3). Nothing the running copy uses changes here.

use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use super::UpdateError;
use super::archive;
use super::manifest::{Artifact, Release};
use super::source::{BUNDLE_MANIFEST_NAME, BUNDLE_SIGNATURE_NAME};
use super::store::{Store, clear_dir};
use crate::artwork::http::{HttpClient, HttpError};
use crate::fsx::Fs;

/// Hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Download, verify and stage `release` for `arch`. Replaces an earlier staged update; a
/// shared folder already in the store with the same hash is not downloaded again.
pub fn download(
    http: &HttpClient,
    fs: &dyn Fs,
    store: &Store,
    release: &Release,
    arch: &str,
) -> Result<(), UpdateError> {
    let files = release.files(arch)?;
    let dir = store.version_dir(&release.version);
    let linked = |link: &Path| store.linked(fs, link).is_some_and(|(_, d)| d == dir);
    if linked(&store.current()) || linked(&store.previous()) {
        return Err(UpdateError::Install(format!(
            "{} is already installed",
            release.version
        )));
    }
    let staging = store.staging();
    clear_dir(fs, &staging)?;
    fs.create_dir_all(&staging, 0o755)?;
    for a in files {
        let path = staging.join(a.name());
        if a.is_folder() {
            let component = fetch_folder(http, fs, store, a)?;
            fs.symlink(&relative_component(&component), &path)?;
        } else {
            let data = fetch(http, a)?;
            fs.write_new(&path, &data, 0o755)?;
            fs.set_mode(&path, 0o755)?;
        }
    }
    fs.write_new(
        &staging.join(BUNDLE_MANIFEST_NAME),
        &release.manifest,
        0o644,
    )?;
    fs.write_new(
        &staging.join(BUNDLE_SIGNATURE_NAME),
        &release.signature,
        0o644,
    )?;
    clear_dir(fs, &dir)?; // an earlier download of this version that never got staged
    fs.rename(&staging, &dir)?;
    store.set_link(fs, &store.staged(), &release.version)
}

/// `../../components/<name>`, as seen from a version folder.
fn relative_component(component: &Path) -> PathBuf {
    let name = component.file_name().unwrap_or_default();
    Path::new("../../components").join(name)
}

/// The gzip download, at most its signed size, matching its signed hash.
fn fetch_gz(http: &HttpClient, a: &Artifact) -> Result<Vec<u8>, UpdateError> {
    let name = a.name();
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
    Ok(r.body)
}

/// One file: downloaded, hashed, unpacked, hashed again.
fn fetch(http: &HttpClient, a: &Artifact) -> Result<Vec<u8>, UpdateError> {
    let body = fetch_gz(http, a)?;
    let mut out = Vec::new();
    GzDecoder::new(body.as_slice())
        .take(a.unpacked_size + 1)
        .read_to_end(&mut out)
        .map_err(|e| UpdateError::bad_file(a.name(), format!("gzip: {e}")))?;
    if out.len() as u64 != a.unpacked_size || sha256_hex(&out) != a.unpacked_sha256 {
        return Err(UpdateError::bad_file(
            a.name(),
            "unpacked file does not match its sha256",
        ));
    }
    Ok(out)
}

/// A folder: its shared copy in the store, downloaded and unpacked unless it is there.
fn fetch_folder(
    http: &HttpClient,
    fs: &dyn Fs,
    store: &Store,
    a: &Artifact,
) -> Result<PathBuf, UpdateError> {
    let target = store.component(a.name(), &a.sha256);
    // A component folder appears only by the rename below, so one that exists is complete.
    if fs.metadata(&target).is_ok_and(|m| m.is_dir()) {
        return Ok(target);
    }
    let body = fetch_gz(http, a)?;
    let tmp = store.component_staging();
    clear_dir(fs, &tmp)?;
    fs.create_dir_all(&tmp, 0o755)?;
    let unpacked = unpack_into(fs, &tmp, &body, a);
    if let Err(e) = unpacked {
        let _ = clear_dir(fs, &tmp); // best effort: the error is what matters
        return Err(e);
    }
    fs.rename(&tmp, &target)?;
    Ok(target)
}

fn unpack_into(fs: &dyn Fs, dir: &Path, gz: &[u8], a: &Artifact) -> Result<(), UpdateError> {
    let mut tar = Hashing::new(GzDecoder::new(gz).take(a.unpacked_size + 1));
    archive::unpack(&mut tar, |e| {
        fs.write_new(&dir.join(&e.name), &e.data, 0o644)?;
        Ok(())
    })?;
    // The zero blocks after the end marker count too.
    std::io::copy(&mut tar, &mut std::io::sink())
        .map_err(|e| UpdateError::bad_file(a.name(), format!("gzip: {e}")))?;
    if tar.len != a.unpacked_size || hex(&tar.hash.finalize()) != a.unpacked_sha256 {
        return Err(UpdateError::bad_file(
            a.name(),
            "unpacked folder does not match its sha256",
        ));
    }
    Ok(())
}

/// Hashes and counts what is read through it.
struct Hashing<R> {
    inner: R,
    hash: Sha256,
    len: u64,
}

impl<R> Hashing<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
            len: 0,
        }
    }
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }
}
