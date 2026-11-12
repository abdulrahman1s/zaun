//! Adopting an SLR variant that umu already installed for the user (`~/.local/share/umu`)
//! into EasyPlay's store, without downloading it again.
//!
//! RUNTIME_MANAGEMENT §5: "SLR installs are only ever taken from the store." Until
//! provisioning exists (M7) the smallest compliant way to use a runtime the user already has
//! is to **copy** it: the variant directory is cloned file by file into a staging directory
//! beside the store (reflinks on btrfs/XFS, so the copy costs almost no space), its
//! integrity manifest is built from the copy (§6.1), and the staging directory is renamed
//! into `STORE/umu/<variant>` in one step. The source is only ever read: nothing under
//! `~/.local/share/umu` (or any other source) is written, locked or renamed.
//!
//! Excluded from the copy: `var/` (pressure-vessel's per-user state) and the install marker,
//! which is written last so that an interrupted copy is never mistaken for an install.

use std::path::{Path, PathBuf};

use super::discovery::{SlrInstall as FoundSlr, detect_slr};
use super::manifest::{self, MAX_DEPTH, MAX_ENTRIES, Manifest};
use super::store::RuntimeStore;
use crate::clock::{Clock, Timestamp};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{FileType, Fs, WalkControl, WalkLimits, safe_remove_tree, safe_walk};
use crate::jobs::CancelToken;
use crate::model::{SlrVariant, from_hex32};
use crate::storage::SlrInstall;

/// What [`adopt_slr`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdoptReport {
    /// Variant.
    pub variant: SlrVariant,
    /// The store directory now holding it.
    pub path: PathBuf,
    /// The store already had a complete install; nothing was copied.
    pub already_installed: bool,
    /// Files copied.
    pub files: u64,
    /// Of which reflinked.
    pub reflinked: u64,
    /// Bytes copied (apparent size).
    pub bytes: u64,
    /// The row for `slr_installs` (with the manifest's tree hash).
    pub install: Option<SlrInstall>,
    /// The manifest written to `<data>/runtime-manifests`.
    pub manifest_entries: usize,
}

/// The variants umu installed outside the store that the store lacks.
pub fn adoptable<'a>(
    foreign: &'a [FoundSlr],
    store_slr: &[FoundSlr],
) -> impl Iterator<Item = &'a FoundSlr> {
    let have: Vec<SlrVariant> = store_slr
        .iter()
        .filter(|s| s.is_complete())
        .map(|s| s.variant)
        .collect();
    foreign
        .iter()
        .filter(move |s| s.is_complete() && !have.contains(&s.variant))
}

fn cancelled() -> Error {
    Error::new(ErrorCode::IntTaskFailed).with("reason", "cancelled")
}

fn invalid(path: &Path, rule: &str) -> Error {
    Error::new(ErrorCode::RtInvalidFolder)
        .with("path", path.display())
        .with("rule", rule)
}

struct CopyStats {
    files: u64,
    reflinked: u64,
    bytes: u64,
}

/// Clone `src` into the new directory `dst` (which must not exist). Never follows symlinks;
/// skips `var/` and the install marker; refuses special files and mount crossings.
fn copy_tree(
    fs: &dyn Fs,
    clock: &dyn Clock,
    src: &Path,
    dst: &Path,
    cancel: &CancelToken,
) -> Result<CopyStats> {
    let src_md = fs.metadata(src)?;
    fs.create_dir(dst, src_md.mode & 0o7777 | 0o700)?;
    let mut stats = CopyStats {
        files: 0,
        reflinked: 0,
        bytes: 0,
    };
    let mut failure: Option<Error> = None;
    let limits = WalkLimits {
        max_entries: MAX_ENTRIES,
        max_depth: MAX_DEPTH,
        deadline: None,
    };
    let summary = safe_walk(fs, src, limits, clock, |e| {
        if cancel.is_cancelled() {
            failure = Some(cancelled());
            return WalkControl::Stop;
        }
        let Ok(rel) = e.path.strip_prefix(src) else {
            return WalkControl::SkipDir;
        };
        let rel_s = rel.to_string_lossy();
        if rel_s == "var" || rel_s == ".installed.ok" || rel_s == ".installed.ok.tmp" {
            return WalkControl::SkipDir;
        }
        let to = dst.join(rel);
        let md = e.metadata;
        let r: Result<()> = match md.file_type {
            FileType::Dir => fs
                .create_dir(&to, (md.mode & 0o7777) | 0o700)
                .map_err(Error::from),
            FileType::Symlink => fs
                .read_link(&e.path)
                .and_then(|t| fs.symlink(&t, &to))
                .map_err(Error::from),
            FileType::File => fs
                .copy_file(&e.path, &to, md.mode & 0o7777)
                .map(|cloned| {
                    stats.files += 1;
                    stats.bytes += md.size;
                    if cloned {
                        stats.reflinked += 1;
                    }
                })
                .map_err(Error::from),
            _ => Err(invalid(&e.path, "special file in the runtime")),
        };
        match r {
            Ok(()) => WalkControl::Continue,
            Err(err) => {
                failure = Some(err);
                WalkControl::Stop
            }
        }
    })?;
    if let Some(e) = failure {
        return Err(e);
    }
    if summary.stopped.is_some() || !summary.errors.is_empty() || summary.mount_boundaries > 0 {
        return Err(invalid(
            src,
            "the runtime directory could not be read completely",
        ));
    }
    // Directories got owner write access for the copy (umu's are 0755 already).
    fs.set_mode(dst, src_md.mode & 0o7777)?;
    Ok(stats)
}

/// Copy the complete variant directory `source` into the store (see the module docs).
/// Blocking and long (hashing ~1 GiB): run on the job pool or in a CLI.
pub fn adopt_slr(
    fs: &dyn Fs,
    clock: &dyn Clock,
    store: &RuntimeStore,
    source: &Path,
    variant: SlrVariant,
    now: Timestamp,
    cancel: &CancelToken,
) -> Result<AdoptReport> {
    let dest = store.slr_dir(variant);
    if let Some(existing) = detect_slr(fs, &dest, variant).filter(FoundSlr::is_complete) {
        return Ok(AdoptReport {
            variant,
            path: existing.path,
            already_installed: true,
            files: 0,
            reflinked: 0,
            bytes: 0,
            install: None,
            manifest_entries: 0,
        });
    }
    if source.starts_with(&store.root) {
        return Err(invalid(source, "the source is inside EasyPlay's store"));
    }
    let found = detect_slr(fs, source, variant)
        .filter(FoundSlr::is_complete)
        .ok_or_else(|| invalid(source, "not a complete runtime install"))?;
    if fs.exists(&dest) {
        // An incomplete leftover (no marker): replace it.
        safe_remove_tree(fs, &store.umu, &dest)?;
    }
    fs.create_dir_all(&store.umu, 0o700)?;
    fs.create_dir_all(&store.manifests, 0o700)?;
    let staging = store
        .root
        .join(format!(".adopt-{}-{}", variant.as_str(), now.as_millis()));
    let result = (|| -> Result<(CopyStats, Manifest)> {
        let stats = copy_tree(fs, clock, source, &staging, cancel)?;
        // umu creates `umu-shim` beside the variants; take the user's when the store has none.
        let shim_src = source.parent().map(|p| p.join("umu-shim"));
        let shim = store.umu_shim();
        if !fs.exists(&shim)
            && let Some(s) = shim_src.filter(|s| fs.metadata(s).is_ok_and(|m| m.is_file()))
        {
            let mode = fs.metadata(&s)?.mode & 0o7777;
            fs.copy_file(&s, &shim, mode)?;
        }
        let m = manifest::build(
            fs,
            clock,
            &staging,
            variant,
            found.version.clone(),
            Some(&shim),
            now,
            cancel,
        )?;
        let json = serde_json::to_vec_pretty(&m)
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
        fs.atomic_write(&store.manifest_file(variant), &json, 0o600)?;
        // The marker last: an interrupted copy is never complete.
        let marker = fs.read(&source.join(".installed.ok"), 4096)?;
        fs.write_new(&staging.join(".installed.ok"), &marker, 0o644)?;
        fs.rename(&staging, &dest)?;
        Ok((stats, m))
    })();
    let (stats, m) = match result {
        Ok(v) => v,
        Err(e) => {
            if let Err(rm) = safe_remove_tree(fs, &store.root, &staging) {
                tracing::warn!(error = %rm, "could not remove the adoption staging directory");
            }
            return Err(e);
        }
    };
    let platform_dir = found
        .platform_dir
        .as_ref()
        .and_then(|p| p.file_name())
        .map(PathBuf::from)
        .unwrap_or_default();
    let install = SlrInstall {
        variant,
        version: found.version.clone().unwrap_or_default(),
        path: dest.clone(),
        platform_dir,
        manifest_tree_sha256: from_hex32(&m.tree_sha256),
        manifest_built_at: Some(now),
        size_bytes: Some(stats.bytes),
    };
    Ok(AdoptReport {
        variant,
        path: dest,
        already_installed: false,
        files: stats.files,
        reflinked: stats.reflinked,
        bytes: stats.bytes,
        install: Some(install),
        manifest_entries: m.entries,
    })
}
