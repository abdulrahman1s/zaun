//! §4.11 Prefix and save data. The external save directory is mounted after the prefix it
//! covers.

use super::mk;
use crate::model::{SandboxPolicy, SaveDataLocation, SessionKind};
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{
    DosDeviceLink, EnvOrigin, HostAction, PROVISION_PREFIX, PlanBuilder, PolicyField,
};

/// Profile directory inside a prefix that an external save folder replaces.
pub const SAVE_MOUNT_REL: &str = "drive_c/users/steamuser";

/// §4.11.
pub fn emit(policy: &SandboxPolicy, ctx: &SandboxContext<'_>, b: &mut PlanBuilder) {
    let game = match (ctx.kind, ctx.game) {
        (SessionKind::Provision, _) | (_, None) => {
            b.mount(
                mk::tmpfs(PROVISION_PREFIX, Some(0o700), None),
                "throw-away prefix",
                None,
            );
            b.env("WINEPREFIX", PROVISION_PREFIX, EnvOrigin::Fixed);
            return;
        }
        (_, Some(g)) => g,
    };
    b.action(HostAction::CreateDir {
        path: game.prefix.to_path_buf(),
        mode: 0o700,
    });
    b.action(HostAction::AcquireLock {
        path: game.prefix_lock.to_path_buf(),
    });
    b.mount(
        mk::rw(game.prefix, game.prefix),
        "the game's private Windows storage",
        None,
    );
    b.env(
        "WINEPREFIX",
        game.prefix.display().to_string(),
        EnvOrigin::Fixed,
    );
    if let SaveDataLocation::External { dir } = &policy.save_data {
        let mount_point = game.prefix.join(SAVE_MOUNT_REL);
        b.action(HostAction::CreateDir {
            path: mount_point.clone(),
            mode: 0o700,
        });
        b.mount(
            mk::rw(dir, &mount_point),
            "save data in your folder",
            Some(PolicyField::SaveData),
        );
    }
    if ctx.untrusted() {
        let mut links: Vec<DosDeviceLink> = policy
            .folders
            .iter()
            .filter_map(|g| {
                g.drive_letter.map(|letter| DosDeviceLink {
                    letter,
                    target: g.canonical.clone(),
                })
            })
            .collect();
        links.sort_by_key(|l| l.letter);
        if !links.is_empty() {
            b.action(HostAction::EnsureDosDevices {
                prefix: game.prefix.to_path_buf(),
                links,
            });
        }
    }
}
