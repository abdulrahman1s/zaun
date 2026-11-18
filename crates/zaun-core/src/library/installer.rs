//! Programs installed by an installer session (LOGIC_SPECS §13): discovery inside the
//! prefix (shortcuts first, then new executables by size) and conversion of the entry.
//!
//! Every read stays inside the prefix: walks never follow symlinks and stop at mount
//! boundaries; shortcut targets are resolved by `identity::lnk::resolve_target`, which
//! accepts only `C:\` paths and relative paths that stay inside the prefix and refuses
//! symlinked components.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::service::{Library, storage};
use crate::clock::{Clock, Timestamp};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{Fs, WalkControl, WalkLimits, safe_walk};
use crate::identity::lnk;
use crate::identity::naming;
use crate::identity::pe::PeLimits;
use crate::ids::{ExecutableId, GameId};
use crate::model::{EntryKind, ExeRole, WorkingDir};
use crate::storage::{ExecutableRepo, GamePatch, GameRepo};

/// Most programs offered.
pub const MAX_PROGRAMS: usize = 20;
/// Most entries visited under `drive_c`.
pub const MAX_WALK_ENTRIES: usize = 200_000;
/// Time budget of the walk.
pub const WALK_BUDGET: Duration = Duration::from_secs(5);

/// A program offered in the *Installer finished* dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledProgram {
    /// Host path inside the prefix.
    pub path: PathBuf,
    /// Display name (shortcut name or file stem).
    pub name: String,
    /// File size.
    pub size: u64,
    /// Found through a shortcut.
    pub from_shortcut: bool,
}

fn lower_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Names that are never offered.
pub fn is_excluded_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("unins")
        || n.contains("setup")
        || n.starts_with("vc_redist")
        || n.starts_with("vcredist")
        || n.starts_with("dxsetup")
}

/// Directories (relative to `drive_c`, lower case) never searched for executables.
fn is_excluded_dir(rel: &Path) -> bool {
    let s = rel.to_string_lossy().to_lowercase();
    s == "windows"
        || s.starts_with("windows/")
        || s == "program files/common files"
        || s == "program files (x86)/common files"
}

/// Longest offered name, in bytes.
const MAX_NAME_BYTES: usize = 80;

/// A display name from game-controlled text: control and bidirectional characters removed,
/// length-capped (SECURITY_MODEL §9 rule 5).
fn display_name(s: &str) -> Option<String> {
    crate::identity::pe::sanitize_str(s, MAX_NAME_BYTES)
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| display_name(&s.to_string_lossy()))
        .unwrap_or_else(|| "Program".to_owned())
}

/// `discover(prefix, since)` of LOGIC_SPECS §13.
pub fn discover(
    fs: &dyn Fs,
    clock: &dyn Clock,
    prefix: &Path,
    since: Timestamp,
) -> Vec<InstalledProgram> {
    let drive_c = prefix.join("drive_c");
    let since_ns = since.as_millis().saturating_mul(1_000_000);
    let mut shortcuts: Vec<PathBuf> = Vec::new();
    let mut exes: Vec<(PathBuf, u64)> = Vec::new();
    let limits = WalkLimits {
        max_entries: MAX_WALK_ENTRIES,
        max_depth: 16,
        ..WalkLimits::with_budget(clock, WALK_BUDGET)
    };
    let walked = safe_walk(fs, &drive_c, limits, clock, |e| {
        let rel = e.path.strip_prefix(&drive_c).unwrap_or(&e.path);
        if e.metadata.is_dir() {
            return if is_excluded_dir(rel) {
                WalkControl::SkipDir
            } else {
                WalkControl::Continue
            };
        }
        if !e.metadata.is_file() {
            return WalkControl::Continue;
        }
        let name = lower_name(&e.path);
        let top = rel
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase());
        if name.ends_with(".lnk") && matches!(top.as_deref(), Some("users" | "programdata")) {
            shortcuts.push(e.path.clone());
        } else if name.ends_with(".exe")
            && e.metadata.mtime_ns >= since_ns
            && !is_excluded_name(&name)
        {
            exes.push((e.path.clone(), e.metadata.size));
        }
        WalkControl::Continue
    });
    if let Err(e) = walked {
        tracing::info!(error = %e, "installer discovery could not walk the prefix");
        return Vec::new();
    }
    let mut out: Vec<InstalledProgram> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for s in shortcuts {
        let Ok(target) = lnk::read(fs, &s) else {
            continue;
        };
        let Some(path) = lnk::resolve_target(fs, prefix, &s, &target) else {
            continue;
        };
        let name = lower_name(&path);
        if !name.ends_with(".exe") || is_excluded_name(&name) || !path.starts_with(&drive_c) {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let size = fs.metadata(&path).map(|m| m.size).unwrap_or(0);
        let display = target
            .name
            .filter(|n| n.chars().count() <= MAX_NAME_BYTES)
            .and_then(|n| display_name(&n))
            .unwrap_or_else(|| stem(&s));
        out.push(InstalledProgram {
            path,
            name: display,
            size,
            from_shortcut: true,
        });
    }
    exes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (path, size) in exes {
        if seen.insert(path.clone()) {
            out.push(InstalledProgram {
                name: stem(&path),
                path,
                size,
                from_shortcut: false,
            });
        }
    }
    out.truncate(MAX_PROGRAMS);
    out
}

impl Library {
    /// Convert an installer entry: `program` (inside the prefix) becomes the primary, the
    /// old primary keeps the role `Installer` ("Run installer again"), the name follows the
    /// program unless the user set it, the icon is refreshed.
    pub fn convert_installer(&mut self, id: GameId, program: &Path) -> Result<ExecutableId> {
        let game = self.game(id)?;
        let prefix = self.deps.paths.prefix_dir(id);
        let canonical = self.deps.fs.canonicalize(program)?;
        if !canonical.starts_with(&prefix) {
            return Err(Error::new(ErrorCode::LibInvalidField)
                .with("field", "executable.path")
                .with("path", program.display()));
        }
        let old_primary = game.primary().cloned().ok_or_else(|| super::no_game(id))?;
        let exe = self.new_executable(None, &canonical, ExeRole::Alternate)?;
        let exe_id = exe.id;
        let mut patch = GamePatch {
            flags: Some(super::with_entry_kind(game.flags, EntryKind::Game)),
            ..GamePatch::default()
        };
        if !game.name_is_user_set {
            let name = naming::derive_name(&exe.pe, None, &canonical);
            patch.sort_name = Some(naming::sort_name(&name));
            patch.name = Some(name);
        }
        if game.launch.working_dir != WorkingDir::ExeDir {
            let mut launch = game.launch.clone();
            launch.working_dir = WorkingDir::ExeDir;
            patch.launch = Some(launch);
        }
        let mut preview = super::service::apply_patch(&game, &patch);
        preview.executables.push(exe.clone());
        for e in &mut preview.executables {
            e.role = if e.id == exe_id {
                ExeRole::Primary
            } else if e.id == old_primary.id {
                ExeRole::Installer
            } else {
                e.role
            };
        }
        self.validate_game(&preview, Some(&game))?;
        let now = self.deps.clock.now();
        let mut installer = old_primary.clone();
        self.db.transaction(|tx| -> Result<()> {
            tx.add_executable(id, &exe).map_err(storage)?;
            tx.set_primary_executable(id, exe_id).map_err(storage)?;
            installer.role = ExeRole::Installer;
            if installer.label.is_none() {
                installer.label = Some("Installer".to_owned());
            }
            tx.update_executable(&installer).map_err(storage)?;
            tx.update_game(id, &patch, now).map_err(storage)?;
            Ok(())
        })?;
        if let Some(codec) = self.deps.codec.clone()
            && let Ok(Some(icon)) = self.deps.pe.largest_icon(&canonical, &PeLimits::default())
            && let Err(e) = self.store_icon(id, &*codec, &icon.ico, now)
        {
            tracing::info!(code = %e.code, "installed program's icon not stored");
        }
        self.refresh_game(id)?;
        Ok(exe_id)
    }
}
