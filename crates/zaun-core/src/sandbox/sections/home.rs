//! §4.8 Home: the private home at the real home path (tmpfs for Provision), plus read-only
//! conveniences. Nothing under `~/.config/protonfixes` is ever bound from outside.

use std::path::Path;

use super::mk;
use crate::fsx::FileType;
use crate::model::SessionKind;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{EnvOrigin, HostAction, PlanBuilder};

fn is_dir(ctx: &SandboxContext<'_>, p: &Path) -> bool {
    ctx.fs
        .metadata_follow(p)
        .is_ok_and(|m| m.file_type == FileType::Dir)
}

/// §4.8.
pub fn emit(ctx: &SandboxContext<'_>, b: &mut PlanBuilder) {
    let home = ctx.home;
    match (ctx.kind, ctx.game) {
        (SessionKind::Provision, _) | (_, None) => {
            b.mount(mk::tmpfs(home, Some(0o700), None), "throw-away home", None);
        }
        (_, Some(game)) => {
            b.action(HostAction::CreateDir {
                path: game.private_home.to_path_buf(),
                mode: 0o700,
            });
            b.mount(
                mk::rw(game.private_home, home),
                "the game's private home",
                None,
            );
        }
    }
    b.env("HOME", home.display().to_string(), EnvOrigin::Fixed);

    let mut conveniences: Vec<(std::path::PathBuf, &'static str)> = Vec::new();
    if ctx.launch.compat.mangohud {
        conveniences.push((home.join(".config/MangoHud"), "MangoHud settings"));
    }
    for rel in [".local/share/fonts", ".fonts"] {
        let p = home.join(rel);
        if is_dir(ctx, &p) {
            conveniences.push((p, "your fonts"));
        }
    }
    conveniences.sort();
    for (p, reason) in conveniences {
        debug_assert!(!p.starts_with(home.join(".config/protonfixes")));
        b.mount(mk::ro_try(&p, &p), reason, None);
    }
}
