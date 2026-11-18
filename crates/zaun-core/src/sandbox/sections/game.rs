//! §4.12 Game folder. Not emitted for Prepare and Provision.

use super::mk;
use crate::host::HostFacts;
use crate::model::{GameDirAccess, SandboxPolicy};
use crate::paths::normalize;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{HostAction, MountKind, PlanBuilder, PolicyField};
use crate::sandbox::validate::{IssueCode, PolicyIssue};

/// §4.12.
pub fn emit(
    policy: &SandboxPolicy,
    ctx: &SandboxContext<'_>,
    host: &HostFacts,
    b: &mut PlanBuilder,
) {
    if !ctx.untrusted() {
        return;
    }
    let Some(game) = ctx.game else {
        return;
    };
    let field = Some(PolicyField::GameDir);
    let root = match (policy.game_dir, game.root) {
        (GameDirAccess::FileOnly, _) | (_, None) => {
            b.mount(mk::ro_same(game.exe), "the program file", field);
            return;
        }
        (_, Some(root)) => root,
    };
    if !normalize(game.exe).starts_with(normalize(root)) {
        b.issue(PolicyIssue::error(
            IssueCode::ExeOutsideRoot,
            Some(game.exe.to_path_buf()),
            "sec.game.exe_outside|The program is not inside the game folder",
        ));
        return;
    }
    match policy.game_dir {
        GameDirAccess::ReadOnly => b.mount(mk::ro_same(root), "the game folder, read-only", field),
        GameDirAccess::ReadWrite => b.mount(mk::rw(root, root), "the game folder", field),
        GameDirAccess::Protected => {
            if !host.overlay_ok() {
                b.issue(PolicyIssue::error(
                    IssueCode::OverlayUnsupported,
                    Some(root.to_path_buf()),
                    "sec.game.overlay_unsupported|Protected game folders are not supported here",
                ));
                return;
            }
            for d in [game.overlay_upper, game.overlay_work] {
                b.action(HostAction::CreateDir {
                    path: d.to_path_buf(),
                    mode: 0o700,
                });
            }
            b.mount(
                MountKind::OverlayRw {
                    lower: root.to_path_buf(),
                    upper: game.overlay_upper.to_path_buf(),
                    work: game.overlay_work.to_path_buf(),
                    dst: root.to_path_buf(),
                },
                "the game folder, changes kept separately",
                field,
            );
        }
        GameDirAccess::FileOnly => {}
    }
}
