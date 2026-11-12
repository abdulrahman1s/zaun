//! Programs in a dropped or chosen folder (GAME_LIBRARY §11): a bounded scan (depth 4,
//! 2 000 entries, no symlinks followed, one filesystem) listing Windows programs, best
//! candidate first. Uninstallers, redistributable installers and crash handlers are kept but
//! marked `filtered` (the add-game sheet shows them under a disclosure).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::clock::Clock;
use crate::fsx::{Fs, FsErrorKind, WalkControl, WalkLimits, safe_walk};
use crate::model::ExeRole;

/// Deepest folder level visited below the scanned folder.
pub const MAX_DEPTH: usize = 4;
/// Most entries visited.
pub const MAX_ENTRIES: usize = 2_000;
/// Time budget of one scan.
pub const BUDGET: Duration = Duration::from_secs(5);

/// One program found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundProgram {
    /// Path.
    pub path: PathBuf,
    /// Role guess (`Primary` for the best candidate, `Installer` for setups).
    pub role: ExeRole,
    /// An uninstaller, redistributable or crash handler.
    pub filtered: bool,
    /// Size in bytes.
    pub size: u64,
}

/// The result of one scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderScan {
    /// The folder.
    pub folder: PathBuf,
    /// Programs, best first.
    pub programs: Vec<FoundProgram>,
    /// The scan stopped at a limit.
    pub truncated: bool,
    /// Why the folder could not be read.
    pub error: Option<String>,
}

fn lower(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Uninstallers, redistributables, crash handlers and helper programs never meant to be
/// started by the user.
pub fn is_filtered(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("unins")
        || n.starts_with("vc_redist")
        || n.starts_with("vcredist")
        || n.starts_with("dxsetup")
        || n.starts_with("dotnet")
        || n.starts_with("directx")
        || n.contains("crashhandler")
        || n.contains("crashreport")
        || n.contains("crashpad")
        || n.starts_with("unitycrashhandler")
        || n == "ue4prereqsetup_x64.exe"
        || n.starts_with("uepre")
}

fn is_installer(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".msi") || n.contains("setup") || n.contains("install")
}

/// Scan `folder`.
pub fn scan(fs: &dyn Fs, clock: &dyn Clock, folder: &Path) -> FolderScan {
    let mut out = FolderScan {
        folder: folder.to_path_buf(),
        ..FolderScan::default()
    };
    let mut found: Vec<(PathBuf, u64, usize)> = Vec::new();
    let limits = WalkLimits {
        max_entries: MAX_ENTRIES,
        max_depth: MAX_DEPTH,
        ..WalkLimits::with_budget(clock, BUDGET)
    };
    let walked = safe_walk(fs, folder, limits, clock, |e| {
        if e.metadata.is_file() {
            let n = lower(&e.path);
            if n.ends_with(".exe") || n.ends_with(".msi") {
                let depth = e
                    .path
                    .strip_prefix(folder)
                    .map_or(0, |r| r.components().count());
                found.push((e.path.clone(), e.metadata.size, depth));
            }
        }
        WalkControl::Continue
    });
    match walked {
        Ok(s) => out.truncated = s.stopped.is_some(),
        Err(e) => {
            out.error = Some(match e.kind {
                FsErrorKind::NotFound => "The folder does not exist".to_owned(),
                FsErrorKind::PermissionDenied => "EasyPlay cannot read this folder".to_owned(),
                _ => "The folder could not be read".to_owned(),
            });
            return out;
        }
    }
    // Best first: wanted before filtered, games before installers, shallow before deep,
    // large before small.
    found.sort_by(|a, b| {
        let key = |p: &(PathBuf, u64, usize)| {
            let n = lower(&p.0);
            (is_filtered(&n), is_installer(&n), p.2)
        };
        key(a)
            .cmp(&key(b))
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut primary_given = false;
    for (path, size, _) in found {
        let n = lower(&path);
        let filtered = is_filtered(&n);
        let role = if is_installer(&n) && !filtered {
            ExeRole::Installer
        } else if !filtered && !primary_given {
            primary_given = true;
            ExeRole::Primary
        } else {
            ExeRole::Alternate
        };
        out.programs.push(FoundProgram {
            path,
            role,
            filtered,
            size,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_filtered("unins000.exe"));
        assert!(is_filtered("UnityCrashHandler64.exe"));
        assert!(is_filtered("vc_redist.x64.exe"));
        assert!(!is_filtered("Game.exe"));
        assert!(is_installer("setup.exe"));
        assert!(is_installer("game.msi"));
    }
}
