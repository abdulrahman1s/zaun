//! File naming and maintenance of the artwork store (ADR-0009, PERSISTENCE §10).
//!
//! Originals live at `<data>/artwork/<game-id>/<slot>-<hash16>.<ext>`, thumbnails at
//! `<cache>/thumbnails/<hash16>-<w>x<h>.png`. Maintenance removes files no row references,
//! keeps the thumbnail cache under its cap, and finds rows whose file is gone.
//!
//! Safety rules: a file whose `hash16` matches **any** referenced hash is never deleted;
//! files younger than [`SWEEP_GRACE`] are left alone (an import may have written the file but
//! not yet committed its row); names that do not parse as ours are never touched; nothing is
//! followed through a symlink.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::fsx::{FileType, Fs, FsErrorKind};
use crate::ids::GameId;
use crate::model::{ArtSlot, ArtworkRef, Sha256, to_hex};
use crate::paths::AppPaths;
use crate::storage::ArtworkRepo;

/// Files younger than this are never swept.
pub const SWEEP_GRACE: Duration = Duration::from_secs(60 * 60);
/// Default cap of `<cache>/thumbnails` (ADR-0009).
pub const THUMB_CACHE_CAP: u64 = 512 * 1024 * 1024;

/// First 16 hex characters of a content hash (file names).
pub fn hash16(hash: &Sha256) -> String {
    to_hex(&hash[..8])
}

/// File name of a stored original: `<slot>-<hash16>.<ext>`.
pub fn original_file_name(slot: ArtSlot, hash: &Sha256, ext: &str) -> String {
    format!("{}-{}.{ext}", slot.as_str(), hash16(hash))
}

/// Full path of a row's file.
pub fn original_path(paths: &AppPaths, art: &ArtworkRef) -> PathBuf {
    paths.artwork_game_dir(art.game_id).join(&art.file)
}

/// Parse `<slot>-<hash16>.<png|jpg>`.
pub fn parse_original_name(name: &str) -> Option<(ArtSlot, String)> {
    let (stem, ext) = name.rsplit_once('.')?;
    if ext != "png" && ext != "jpg" {
        return None;
    }
    let (slot, h) = stem.split_once('-')?;
    let slot = ArtSlot::ALL.into_iter().find(|s| s.as_str() == slot)?;
    is_hash16(h).then(|| (slot, h.to_owned()))
}

/// Parse `<hash16>-<w>x<h>.png`.
pub fn parse_thumb_name(name: &str) -> Option<(String, u32, u32)> {
    let stem = name.strip_suffix(".png")?;
    let (h, dims) = stem.split_once('-')?;
    let (w, hh) = dims.split_once('x')?;
    if !is_hash16(h) {
        return None;
    }
    Some((h.to_owned(), w.parse().ok()?, hh.parse().ok()?))
}

fn is_hash16(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What a sweep did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Unreferenced originals removed.
    pub originals_removed: usize,
    /// Thumbnails removed (unreferenced or over the cap).
    pub thumbs_removed: usize,
    /// Bytes freed.
    pub bytes_freed: u64,
    /// Removals that failed (left for the next sweep).
    pub errors: usize,
}

/// Sweep options.
#[derive(Clone, Copy, Debug)]
pub struct SweepOptions {
    /// Current wall-clock time, nanoseconds since the epoch (for the grace period).
    pub now_ns: i64,
    /// Thumbnail cache cap in bytes.
    pub thumb_cap: u64,
}

fn old_enough(mtime_ns: i64, now_ns: i64) -> bool {
    let grace = i64::try_from(SWEEP_GRACE.as_nanos()).unwrap_or(i64::MAX);
    now_ns.saturating_sub(mtime_ns) >= grace
}

/// Remove unreferenced originals and thumbnails, then trim the thumbnail cache to its cap,
/// oldest first. `referenced` is every hash a row references
/// ([`ArtworkRepo::referenced_artwork_hashes`]).
pub fn sweep(
    fs: &dyn Fs,
    paths: &AppPaths,
    referenced: &BTreeSet<Sha256>,
    opts: SweepOptions,
) -> SweepReport {
    let keep: BTreeSet<String> = referenced.iter().map(hash16).collect();
    let mut report = SweepReport::default();

    // Originals.
    if let Ok(games) = fs.read_dir(&paths.artwork_dir) {
        for g in games.into_iter().filter(|e| e.file_type == FileType::Dir) {
            let Ok(files) = fs.read_dir(&g.path) else {
                continue;
            };
            let mut left = 0usize;
            for f in files {
                let parsed = f
                    .name
                    .to_str()
                    .filter(|_| f.file_type == FileType::File)
                    .and_then(parse_original_name);
                let Some((_, h)) = parsed else {
                    left += 1;
                    continue;
                };
                let Ok(md) = fs.metadata(&f.path) else {
                    left += 1;
                    continue;
                };
                if keep.contains(&h) || !old_enough(md.mtime_ns, opts.now_ns) {
                    left += 1;
                    continue;
                }
                match fs.remove_file(&f.path) {
                    Ok(()) => {
                        report.originals_removed += 1;
                        report.bytes_freed += md.size;
                    }
                    Err(_) => {
                        report.errors += 1;
                        left += 1;
                    }
                }
            }
            if left == 0 {
                let _ = fs.remove_dir(&g.path);
            }
        }
    }

    // Thumbnails: unreferenced first, then the cap (LRU by modification time: `Fs` does
    // not expose access times, and a thumbnail is rewritten whenever it is regenerated).
    let mut kept: Vec<(i64, u64, PathBuf)> = Vec::new();
    if let Ok(files) = fs.read_dir(&paths.thumbs_dir) {
        for f in files {
            let parsed = f
                .name
                .to_str()
                .filter(|_| f.file_type == FileType::File)
                .and_then(parse_thumb_name);
            let Some((h, _, _)) = parsed else { continue };
            let Ok(md) = fs.metadata(&f.path) else {
                continue;
            };
            if keep.contains(&h) || !old_enough(md.mtime_ns, opts.now_ns) {
                kept.push((md.mtime_ns, md.size, f.path));
                continue;
            }
            remove_counted(fs, &f.path, md.size, &mut report);
        }
    }
    let mut total: u64 = kept.iter().map(|k| k.1).sum();
    if total > opts.thumb_cap {
        kept.sort_by_key(|k| k.0);
        for (_, size, path) in kept {
            if total <= opts.thumb_cap {
                break;
            }
            if remove_counted(fs, &path, size, &mut report) {
                total -= size;
            }
        }
    }
    report
}

fn remove_counted(fs: &dyn Fs, path: &Path, size: u64, report: &mut SweepReport) -> bool {
    match fs.remove_file(path) {
        Ok(()) => {
            report.thumbs_removed += 1;
            report.bytes_freed += size;
            true
        }
        Err(e) if e.kind == FsErrorKind::NotFound => true,
        Err(_) => {
            report.errors += 1;
            false
        }
    }
}

/// Whether a row's file is missing or not a regular file.
pub fn is_broken(fs: &dyn Fs, paths: &AppPaths, art: &ArtworkRef) -> bool {
    !fs.metadata(&original_path(paths, art))
        .is_ok_and(|m| m.is_file() && m.size > 0)
}

/// Delete the rows of `games` whose file is gone (ERROR_HANDLING §4.8): the UI then falls
/// back to the icon or the generated placeholder. Returns the repaired slots.
pub fn repair_broken_rows<R: ArtworkRepo + ?Sized>(
    fs: &dyn Fs,
    paths: &AppPaths,
    repo: &mut R,
    games: &[GameId],
) -> Result<Vec<(GameId, ArtSlot)>> {
    let mut out = Vec::new();
    for &g in games {
        let rows = repo.artwork_of(g).map_err(storage_err)?;
        for art in rows.iter().filter(|a| is_broken(fs, paths, a)) {
            tracing::warn!(game = %g, slot = art.slot.as_str(), "artwork file missing; row removed");
            repo.delete_artwork(g, art.slot).map_err(storage_err)?;
            out.push((g, art.slot));
        }
    }
    Ok(out)
}

pub(crate) fn storage_err(e: crate::storage::StorageError) -> Error {
    Error::new(e.code()).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        let h = [0xab; 32];
        let n = original_file_name(ArtSlot::Cover, &h, "png");
        assert_eq!(n, "cover-abababababababab.png");
        assert_eq!(
            parse_original_name(&n),
            Some((ArtSlot::Cover, "abababababababab".into()))
        );
        assert_eq!(parse_original_name("cover-ABABABABABABABAB.png"), None);
        assert_eq!(parse_original_name("cover-abababababababab.exe"), None);
        assert_eq!(parse_original_name(".tmp-cover-abababababababab.png"), None);
        assert_eq!(
            parse_thumb_name("abababababababab-240x360.png"),
            Some(("abababababababab".into(), 240, 360))
        );
        assert_eq!(parse_thumb_name("abababababababab-240x.png"), None);
    }
}
