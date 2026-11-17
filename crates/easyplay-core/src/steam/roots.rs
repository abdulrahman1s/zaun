//! Where Steam is installed (ADR-0031 §2) and which library folders it uses.
//!
//! Candidate roots, in this order: `~/.local/share/Steam`, `~/.steam/steam`, `~/.steam/root`
//! (native; the last two are usually symbolic links to the first), the Flatpak root
//! `~/.var/app/com.valvesoftware.Steam/.local/share/Steam` and the Snap root
//! `~/snap/steam/common/.local/share/Steam`. Each candidate is canonicalised (the links are
//! followed) and kept only when it holds a `steamapps` folder; duplicates are dropped.

use std::path::{Path, PathBuf};

use super::keyvalues::{KvLimits, parse};
use crate::fsx::Fs;
use crate::model::SteamInstallKind;

/// A Steam installation found on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamRoot {
    /// The canonical root (`…/Steam`).
    pub path: PathBuf,
    /// How Steam is installed.
    pub kind: SteamInstallKind,
}

/// The Flatpak application ID of Steam.
pub const FLATPAK_APP_ID: &str = "com.valvesoftware.Steam";

/// The candidate roots under `home`, in search order.
pub fn candidates(home: &Path) -> Vec<(PathBuf, SteamInstallKind)> {
    use SteamInstallKind::{Flatpak, Native, Snap};
    vec![
        (home.join(".local/share/Steam"), Native),
        (home.join(".steam/steam"), Native),
        (home.join(".steam/root"), Native),
        (
            home.join(".var/app")
                .join(FLATPAK_APP_ID)
                .join(".local/share/Steam"),
            Flatpak,
        ),
        (home.join("snap/steam/common/.local/share/Steam"), Snap),
    ]
}

/// The Steam installations under `home` (canonical, de-duplicated, each with `steamapps`).
pub fn find_roots(fs: &dyn Fs, home: &Path) -> Vec<SteamRoot> {
    let mut out: Vec<SteamRoot> = Vec::new();
    for (candidate, kind) in candidates(home) {
        let Ok(path) = fs.canonicalize(&candidate) else {
            continue;
        };
        if !fs
            .metadata(&path.join("steamapps"))
            .is_ok_and(|m| m.is_dir())
        {
            continue;
        }
        if out.iter().any(|r| r.path == path) {
            continue;
        }
        out.push(SteamRoot { path, kind });
    }
    out
}

/// The library folders listed in `libraryfolders.vdf` text, in file order. Both layouts are
/// read: the current one (`"0" { "path" "…" … }`) and the one before 2021 (`"1" "…"`).
/// Relative and empty paths are ignored.
pub fn parse_library_folders(bytes: &[u8]) -> Vec<PathBuf> {
    let parsed = parse(bytes, &KvLimits::SMALL);
    let Some(top) = parsed
        .root
        .obj("libraryfolders")
        .or_else(|| parsed.root.obj("LibraryFolders"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in &top.0 {
        if !key.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let path = match value {
            super::keyvalues::KvValue::Obj(o) => o.str("path"),
            super::keyvalues::KvValue::Str(s) => Some(s.as_str()),
        };
        if let Some(p) = path.map(str::trim).filter(|p| p.starts_with('/')) {
            out.push(PathBuf::from(p));
        }
    }
    out
}

/// Largest `libraryfolders.vdf` read.
pub const LIBRARY_FOLDERS_MAX: u64 = 1024 * 1024;
/// Most library folders considered per root.
pub const MAX_LIBRARY_FOLDERS: usize = 64;

/// The library folders of `root`: the root itself first, then those listed in
/// `steamapps/libraryfolders.vdf` (canonical, with a `steamapps` folder, de-duplicated). A
/// folder on a drive that is not connected is left out.
pub fn library_folders(fs: &dyn Fs, root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let listed = fs
        .read(
            &root.join("steamapps/libraryfolders.vdf"),
            LIBRARY_FOLDERS_MAX,
        )
        .map(|b| parse_library_folders(&b))
        .unwrap_or_default();
    for p in listed.into_iter().take(MAX_LIBRARY_FOLDERS) {
        let Ok(c) = fs.canonicalize(&p) else { continue };
        if out.contains(&c) {
            continue;
        }
        if fs.metadata(&c.join("steamapps")).is_ok_and(|m| m.is_dir()) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_layouts() {
        let new = include_bytes!("../../tests/data/steam/libraryfolders.vdf");
        assert_eq!(
            parse_library_folders(new),
            vec![
                PathBuf::from("/home/player/.local/share/Steam"),
                PathBuf::from("/mnt/games/SteamLibrary"),
            ]
        );
        let old = b"\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1\"\n\t\"ContentStatsID\"\t\t\"2\"\n\t\"1\"\t\t\"/mnt/old/Steam\"\n\t\"2\"\t\t\"relative/path\"\n}\n";
        assert_eq!(
            parse_library_folders(old),
            vec![PathBuf::from("/mnt/old/Steam")]
        );
        assert!(parse_library_folders(b"garbage {").is_empty());
    }

    #[test]
    fn candidate_order() {
        let c = candidates(Path::new("/home/p"));
        assert_eq!(c[0].0, PathBuf::from("/home/p/.local/share/Steam"));
        assert_eq!(c[3].1, SteamInstallKind::Flatpak);
        assert_eq!(c[4].1, SteamInstallKind::Snap);
    }
}
