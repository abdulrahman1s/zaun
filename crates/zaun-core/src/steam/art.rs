//! Cover and banner pictures from Steam's own local cache (ADR-0031 §3): no network request.
//!
//! `appcache/librarycache` has had three layouts, all read:
//!
//! 1. flat files: `<appid>_library_600x900.jpg`, `<appid>_library_hero.jpg`;
//! 2. a folder per app: `<appid>/library_600x900.jpg`, `<appid>/library_hero.jpg`;
//! 3. hashed sub-folders per asset: `<appid>/<sha1>/library_600x900.jpg` or
//!    `<appid>/<sha1>/library_capsule.jpg` (the 2:3 capsule), `<appid>/<sha1>/library_hero.jpg`.
//!
//! The picture is referenced where it is (never copied); its content hash is the texture
//! and thumbnail key, so Zaun's thumbnail cache serves it like any other cover.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256 as Sha};

use crate::fsx::{FileType, Fs};
use crate::model::{ArtSlot, Sha256};

/// Largest picture read from the cache.
pub const ART_MAX: u64 = 8 * 1024 * 1024;

/// A picture found in Steam's cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedArt {
    /// Slot it fills.
    pub slot: ArtSlot,
    /// The file in Steam's cache.
    pub path: PathBuf,
    /// SHA-256 of its content.
    pub hash: Sha256,
}

fn names(slot: ArtSlot) -> &'static [&'static str] {
    match slot {
        ArtSlot::Cover => &["library_600x900.jpg", "library_capsule.jpg"],
        ArtSlot::Hero => &["library_hero.jpg"],
        ArtSlot::Icon => &[],
    }
}

fn is_file(fs: &dyn Fs, p: &Path) -> Option<i64> {
    fs.metadata(p)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.mtime_ns)
}

/// The file Steam's cache under `root` holds for `app_id` and `slot`, if any.
pub fn find(fs: &dyn Fs, root: &Path, app_id: u32, slot: ArtSlot) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let dir = cache.join(app_id.to_string());
    for name in names(slot) {
        let p = dir.join(name);
        if is_file(fs, &p).is_some() {
            return Some(p);
        }
        // Hashed sub-folders: the newest wins.
        if let Ok(entries) = fs.read_dir(&dir) {
            let best = entries
                .into_iter()
                .filter(|e| e.file_type == FileType::Dir)
                .take(64)
                .filter_map(|e| {
                    let p = e.path.join(name);
                    is_file(fs, &p).map(|t| (t, p))
                })
                .max_by_key(|(t, _)| *t);
            if let Some((_, p)) = best {
                return Some(p);
            }
        }
        let flat = cache.join(format!("{app_id}_{name}"));
        if is_file(fs, &flat).is_some() {
            return Some(flat);
        }
    }
    None
}

/// The cover and banner of `app_id`, with their content hashes (unreadable files skipped).
pub fn read_art(fs: &dyn Fs, root: &Path, app_id: u32) -> Vec<CachedArt> {
    [ArtSlot::Cover, ArtSlot::Hero]
        .into_iter()
        .filter_map(|slot| {
            let path = find(fs, root, app_id, slot)?;
            let bytes = fs.read(&path, ART_MAX).ok()?;
            let hash: Sha256 = Sha::digest(&bytes).into();
            Some(CachedArt { slot, path, hash })
        })
        .collect()
}
