//! The integrity manifest of an SLR variant directory (RUNTIME_MANAGEMENT §6.1, RT-006).
//!
//! `build` walks the variant directory without following symlinks and records every entry
//! (files with mode, size and SHA-256; directories with mode; symlinks with their target),
//! sorted bytewise by relative path. `tree_sha256` is the SHA-256 of the canonical JSON
//! serialisation of `files`; it is also stored in `slr_installs` so that replacing the
//! manifest file is detected. The audit that compares a tree against a manifest (RT-007)
//! arrives with M7.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256 as Sha};

use crate::clock::{Clock, Timestamp};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{FileType, Fs, WalkControl, WalkLimits, safe_walk};
use crate::jobs::CancelToken;
use crate::model::{Sha256, SlrVariant, to_hex};

/// Manifest file format version.
pub const MANIFEST_FORMAT: u32 = 1;
/// Most entries a variant directory may have (§6.2).
pub const MAX_ENTRIES: usize = 200_000;
/// Deepest nesting walked (§6.2).
pub const MAX_DEPTH: usize = 64;

/// One recorded entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path relative to the variant directory.
    pub p: String,
    /// Kind: `f` file, `d` directory, `l` symlink.
    pub k: String,
    /// Permission bits (files, directories).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<u32>,
    /// Size (files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    /// SHA-256, hex (files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<String>,
    /// Target (symlinks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

/// `<data>/runtime-manifests/<variant>.manifest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// [`MANIFEST_FORMAT`].
    pub format: u32,
    /// Variant name.
    pub variant: String,
    /// Installed version.
    #[serde(default)]
    pub version: Option<String>,
    /// Creation time, milliseconds since the epoch.
    pub created_at: i64,
    /// `files.len()`.
    pub entries: usize,
    /// SHA-256 of the canonical serialisation of `files`, hex.
    pub tree_sha256: String,
    /// The entries, sorted bytewise by path.
    pub files: Vec<ManifestEntry>,
    /// `umu-shim`, recorded separately (it lives beside the variant directories).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umu_shim: Option<ManifestEntry>,
}

/// Paths excluded from the manifest: `var/**`, the install markers and the platform's
/// `files/.ref` (pressure-vessel rewrites them).
pub fn is_excluded(rel: &str) -> bool {
    rel == "var"
        || rel.starts_with("var/")
        || rel == ".installed.ok"
        || rel == ".installed.ok.tmp"
        || rel
            .split_once('/')
            .is_some_and(|(first, rest)| first.contains("_platform_") && rest == "files/.ref")
}

/// SHA-256 of a file's content, streamed.
pub fn hash_file(fs: &dyn Fs, path: &Path, cancel: &CancelToken) -> Result<Sha256> {
    let mut r = fs.open_read(path)?;
    let mut h = Sha::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.is_cancelled() {
            return Err(Error::new(ErrorCode::IntTaskFailed).with("reason", "cancelled"));
        }
        let n = r
            .read(&mut buf)
            .map_err(|e| Error::new(ErrorCode::FsReadDenied).with_source(e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// The tree hash over `files` (canonical: compact JSON of the sorted list).
pub fn tree_hash(files: &[ManifestEntry]) -> Sha256 {
    let json = serde_json::to_vec(files).unwrap_or_default();
    Sha::digest(&json).into()
}

/// Describe one entry; `None` for kinds that have no place in a runtime (sockets, devices).
pub fn entry_of(
    fs: &dyn Fs,
    path: &Path,
    rel: &str,
    cancel: &CancelToken,
) -> Result<Option<ManifestEntry>> {
    let md = fs.metadata(path)?;
    let e = match md.file_type {
        FileType::File => ManifestEntry {
            p: rel.to_owned(),
            k: "f".into(),
            m: Some(md.mode & 0o7777),
            s: Some(md.size),
            h: Some(to_hex(&hash_file(fs, path, cancel)?)),
            t: None,
        },
        FileType::Dir => ManifestEntry {
            p: rel.to_owned(),
            k: "d".into(),
            m: Some(md.mode & 0o7777),
            s: None,
            h: None,
            t: None,
        },
        FileType::Symlink => ManifestEntry {
            p: rel.to_owned(),
            k: "l".into(),
            m: None,
            s: None,
            h: None,
            t: Some(fs.read_link(path)?.to_string_lossy().into_owned()),
        },
        _ => return Ok(None),
    };
    Ok(Some(e))
}

/// Build the manifest of `dir` (a variant directory); `umu_shim` is the shim beside it.
#[allow(clippy::too_many_arguments)]
pub fn build(
    fs: &dyn Fs,
    clock: &dyn Clock,
    dir: &Path,
    variant: SlrVariant,
    version: Option<String>,
    umu_shim: Option<&Path>,
    now: Timestamp,
    cancel: &CancelToken,
) -> Result<Manifest> {
    let mut paths = Vec::new();
    let limits = WalkLimits {
        max_entries: MAX_ENTRIES,
        max_depth: MAX_DEPTH,
        deadline: None,
    };
    let summary = safe_walk(fs, dir, limits, clock, |e| {
        let Ok(rel) = e.path.strip_prefix(dir) else {
            return WalkControl::SkipDir;
        };
        let rel = rel.to_string_lossy().into_owned();
        if is_excluded(&rel) {
            return WalkControl::SkipDir;
        }
        paths.push((rel, e.path.clone()));
        WalkControl::Continue
    })?;
    if summary.stopped.is_some() || !summary.errors.is_empty() || summary.mount_boundaries > 0 {
        return Err(Error::new(ErrorCode::RtVerificationFailed)
            .with("path", dir.display())
            .with(
                "reason",
                "the runtime directory could not be walked completely",
            ));
    }
    paths.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut files = Vec::with_capacity(paths.len());
    for (rel, path) in &paths {
        if let Some(e) = entry_of(fs, path, rel, cancel)? {
            files.push(e);
        }
    }
    let shim = match umu_shim {
        Some(p) if fs.metadata(p).is_ok_and(|m| m.is_file()) => {
            entry_of(fs, p, "umu-shim", cancel)?
        }
        _ => None,
    };
    let tree = tree_hash(&files);
    Ok(Manifest {
        format: MANIFEST_FORMAT,
        variant: variant.as_str().to_owned(),
        version,
        created_at: now.as_millis(),
        entries: files.len(),
        tree_sha256: to_hex(&tree),
        files,
        umu_shim: shim,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions() {
        for p in [
            "var",
            "var/tmp-1/x",
            ".installed.ok",
            ".installed.ok.tmp",
            "sniper_platform_3.0/files/.ref",
        ] {
            assert!(is_excluded(p), "{p}");
        }
        for p in [
            "variants",
            "_v2-entry-point",
            "sniper_platform_3.0/files/lib/x.so",
            "pressure-vessel/files/.ref",
        ] {
            assert!(!is_excluded(p), "{p}");
        }
    }

    #[test]
    fn tree_hash_depends_on_every_field() {
        let a = vec![ManifestEntry {
            p: "a".into(),
            k: "f".into(),
            m: Some(0o644),
            s: Some(1),
            h: Some("00".into()),
            t: None,
        }];
        let mut b = a.clone();
        b[0].m = Some(0o755);
        assert_ne!(tree_hash(&a), tree_hash(&b));
        assert_eq!(tree_hash(&a), tree_hash(&a.clone()));
    }
}
