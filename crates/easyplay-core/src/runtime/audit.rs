//! Compare an SLR variant directory with its integrity manifest (RUNTIME_MANAGEMENT §6.2,
//! RT-007).
//!
//! 1. Walk the variant directory without following symlinks (limits of §6.2).
//! 2. Compare the path sets: `added`, `removed`.
//! 3. For common paths compare kind, mode, symlink target and size; hash files whose size
//!    matches (in parallel chunks, [`manifest::describe_all`]).
//! 4. The platform's `files/.ref` must be a regular file; its content is ignored, and when
//!    it is larger than 1 MiB it is truncated and noted.
//! 5. `umu-shim` (beside the variants) is compared with the manifest's separate entry.
//!
//! A manifest that is missing, unreadable, or whose tree hash differs from the one stored in
//! the library counts as tampering: nothing in the tree can be trusted then.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::manifest::{
    self, MAX_DEPTH, MAX_ENTRIES, Manifest, ManifestEntry, ManifestProblem, is_excluded,
};
use super::store::RuntimeStore;
use crate::clock::Clock;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{FileType, Fs, WalkControl, WalkLimits, safe_walk};
use crate::jobs::CancelToken;
use crate::model::{Sha256, SlrVariant};

/// Paths listed per category in a report.
pub const LIST_CAP: usize = 50;
/// `.ref` files larger than this are truncated.
pub const REF_LIMIT: u64 = 1 << 20;

/// The differences found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TamperSummary {
    /// Changed entries (content, mode, kind, link target), at most [`LIST_CAP`].
    pub changed: Vec<String>,
    /// Entries not in the manifest, at most [`LIST_CAP`].
    pub added: Vec<String>,
    /// Manifest entries not on disk, at most [`LIST_CAP`].
    pub removed: Vec<String>,
    /// Total changed.
    pub changed_count: usize,
    /// Total added.
    pub added_count: usize,
    /// Total removed.
    pub removed_count: usize,
    /// The manifest itself was missing, invalid or replaced.
    pub manifest_problem: Option<String>,
}

impl TamperSummary {
    fn push(list: &mut Vec<String>, count: &mut usize, p: &str) {
        *count += 1;
        if list.len() < LIST_CAP {
            list.push(p.to_owned());
        }
    }

    /// Whether anything differs.
    pub fn is_empty(&self) -> bool {
        self.changed_count == 0
            && self.added_count == 0
            && self.removed_count == 0
            && self.manifest_problem.is_none()
    }

    /// One line for logs and event rows.
    pub fn line(&self) -> String {
        match &self.manifest_problem {
            Some(p) => format!("manifest {p}"),
            None => format!(
                "{} changed, {} added, {} removed",
                self.changed_count, self.added_count, self.removed_count
            ),
        }
    }
}

/// The result of an audit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum AuditReport {
    /// The tree matches its manifest.
    Clean {
        /// Entries compared.
        entries: usize,
        /// Remarks (a truncated `.ref`).
        notes: Vec<String>,
    },
    /// Something differs.
    Tampered(TamperSummary),
    /// The variant directory does not exist.
    Missing,
}

impl AuditReport {
    /// `clean`, `tampered`, `missing` (the `runtime_audit.last_result` column).
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditReport::Clean { .. } => "clean",
            AuditReport::Tampered(_) => "tampered",
            AuditReport::Missing => "missing",
        }
    }
}

fn walk_limits() -> WalkLimits {
    WalkLimits {
        max_entries: MAX_ENTRIES,
        max_depth: MAX_DEPTH,
        deadline: None,
    }
}

/// Check the platform `.ref` files: regular files, truncated when larger than 1 MiB.
fn check_refs(fs: &dyn Fs, dir: &Path, summary: &mut TamperSummary, notes: &mut Vec<String>) {
    let Ok(entries) = fs.read_dir(dir) else {
        return;
    };
    for e in entries {
        let name = e.name.to_string_lossy().into_owned();
        if !name.contains("_platform_") || e.file_type != FileType::Dir {
            continue;
        }
        let rel = format!("{name}/files/.ref");
        let path = dir.join(&rel);
        match fs.metadata(&path) {
            Ok(md) if md.is_file() => {
                if md.size > REF_LIMIT {
                    match fs.atomic_write(&path, b"", md.mode & 0o7777) {
                        Ok(()) => notes.push(format!("{rel} was {} bytes; truncated", md.size)),
                        Err(_) => {
                            TamperSummary::push(
                                &mut summary.changed,
                                &mut summary.changed_count,
                                &rel,
                            );
                        }
                    }
                }
            }
            Ok(_) => {
                TamperSummary::push(&mut summary.changed, &mut summary.changed_count, &rel);
            }
            Err(_) => {
                TamperSummary::push(&mut summary.removed, &mut summary.removed_count, &rel);
            }
        }
    }
}

fn same_meta(a: &ManifestEntry, b: &ManifestEntry) -> bool {
    a.k == b.k && a.m == b.m && a.s == b.s && a.t == b.t
}

/// Audit `dir` against `m`. `umu_shim` is the shim beside the variants (compared when the
/// manifest records one).
pub fn run(
    fs: &dyn Fs,
    clock: &dyn Clock,
    dir: &Path,
    m: &Manifest,
    umu_shim: Option<&Path>,
    cancel: &CancelToken,
) -> Result<AuditReport> {
    match fs.metadata(dir) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => {
            return Ok(AuditReport::Tampered(TamperSummary {
                manifest_problem: None,
                changed: vec![".".into()],
                changed_count: 1,
                ..TamperSummary::default()
            }));
        }
        Err(_) => return Ok(AuditReport::Missing),
    }
    let mut on_disk: Vec<(String, PathBuf, FileType, u32, u64)> = Vec::new();
    let summary_walk = safe_walk(fs, dir, walk_limits(), clock, |e| {
        if cancel.is_cancelled() {
            return WalkControl::Stop;
        }
        let Ok(rel) = e.path.strip_prefix(dir) else {
            return WalkControl::SkipDir;
        };
        let rel = rel.to_string_lossy().into_owned();
        if is_excluded(&rel) {
            return WalkControl::SkipDir;
        }
        on_disk.push((
            rel,
            e.path.clone(),
            e.metadata.file_type,
            e.metadata.mode & 0o7777,
            e.metadata.size,
        ));
        WalkControl::Continue
    })?;
    if cancel.is_cancelled() {
        return Err(Error::new(ErrorCode::IntTaskFailed).with("reason", "cancelled"));
    }
    let mut s = TamperSummary::default();
    if summary_walk.stopped.is_some() || summary_walk.mount_boundaries > 0 {
        // Too many entries, too deep, or a mount inside the runtime: not what umu installs.
        TamperSummary::push(&mut s.added, &mut s.added_count, "<limits exceeded>");
    }
    let expected: BTreeMap<&str, &ManifestEntry> =
        m.files.iter().map(|e| (e.p.as_str(), e)).collect();
    let mut to_hash: Vec<(String, PathBuf)> = Vec::new();
    let mut seen = 0usize;
    for (rel, path, ft, mode, size) in &on_disk {
        let Some(exp) = expected.get(rel.as_str()) else {
            TamperSummary::push(&mut s.added, &mut s.added_count, rel);
            continue;
        };
        seen += 1;
        let kind_ok = matches!(
            (ft, exp.k.as_str()),
            (FileType::File, "f") | (FileType::Dir, "d") | (FileType::Symlink, "l")
        );
        let mode_ok = match ft {
            FileType::Symlink => true,
            _ => exp.m == Some(*mode),
        };
        let size_ok = *ft != FileType::File || exp.s == Some(*size);
        if !(kind_ok && mode_ok && size_ok) {
            TamperSummary::push(&mut s.changed, &mut s.changed_count, rel);
            continue;
        }
        match ft {
            FileType::File => to_hash.push((rel.clone(), path.clone())),
            FileType::Symlink => {
                let target = fs
                    .read_link(path)
                    .map(|t| t.to_string_lossy().into_owned())
                    .ok();
                if target.as_deref() != exp.t.as_deref() {
                    TamperSummary::push(&mut s.changed, &mut s.changed_count, rel);
                }
            }
            _ => {}
        }
    }
    if seen < expected.len() {
        let present: std::collections::HashSet<&str> =
            on_disk.iter().map(|(r, ..)| r.as_str()).collect();
        for e in &m.files {
            if !present.contains(e.p.as_str()) {
                TamperSummary::push(&mut s.removed, &mut s.removed_count, &e.p);
            }
        }
    }
    let hashed = manifest::describe_all(fs, &to_hash, cancel)?;
    for h in hashed {
        if let Some(exp) = expected.get(h.p.as_str())
            && (exp.h != h.h || !same_meta(exp, &h))
        {
            TamperSummary::push(&mut s.changed, &mut s.changed_count, &h.p);
        }
    }
    if let (Some(exp), Some(shim)) = (&m.umu_shim, umu_shim) {
        match manifest::entry_of(fs, shim, "umu-shim", cancel) {
            Ok(Some(e)) if e == *exp => {}
            Ok(_) | Err(_) => {
                TamperSummary::push(&mut s.changed, &mut s.changed_count, "../umu-shim");
            }
        }
    }
    let mut notes = Vec::new();
    check_refs(fs, dir, &mut s, &mut notes);
    if s.is_empty() {
        Ok(AuditReport::Clean {
            entries: m.files.len(),
            notes,
        })
    } else {
        Ok(AuditReport::Tampered(s))
    }
}

/// Audit `variant` in `store` against its manifest file; `stored_tree` is the tree hash
/// recorded in `slr_installs` (a mismatch means the manifest file was replaced).
pub fn audit_variant(
    fs: &dyn Fs,
    clock: &dyn Clock,
    store: &RuntimeStore,
    variant: SlrVariant,
    stored_tree: Option<&Sha256>,
    cancel: &CancelToken,
) -> Result<(AuditReport, Duration)> {
    let t0 = clock.mono();
    let dir = store.slr_dir(variant);
    if !fs.exists(&dir) {
        return Ok((AuditReport::Missing, Duration::ZERO));
    }
    let m = match manifest::load(fs, &store.manifest_file(variant), variant, stored_tree) {
        Ok(m) => m,
        Err(p) => {
            let problem = match p {
                ManifestProblem::Missing => "missing".to_owned(),
                ManifestProblem::Invalid(why) => format!("invalid ({why})"),
                ManifestProblem::Replaced => "replaced".to_owned(),
            };
            return Ok((
                AuditReport::Tampered(TamperSummary {
                    manifest_problem: Some(problem),
                    ..TamperSummary::default()
                }),
                clock.mono().saturating_duration_since(t0),
            ));
        }
    };
    let shim = store.umu_shim();
    let r = run(fs, clock, &dir, &m, Some(&shim), cancel)?;
    Ok((r, clock.mono().saturating_duration_since(t0)))
}
