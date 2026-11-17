//! The never-stored game a prefix tool runs under, and its plan adjustment (ADR-0031 §6).
use crate::clock::Timestamp;
use crate::model::{
    ExeRole, Executable, Fingerprint, Game, GameDirAccess, GameFlags, LaunchConfig, PeInfo,
    RuntimeRef,
};
use crate::service::prelude::*;

/// The game a prefix tool runs under: never stored; the Offline preset with the program's
/// folder read-only (the file alone in a shared location such as Downloads), and the Proton
/// build Steam last used.
pub fn tool_game(
    id: GameId,
    name: &str,
    program: &Path,
    root: Option<&Path>,
    proton: &Path,
    now: Timestamp,
) -> Game {
    let mut policy = crate::sandbox::offline();
    policy.game_dir = if root.is_some() {
        GameDirAccess::ReadOnly
    } else {
        GameDirAccess::FileOnly
    };
    policy.preset = crate::sandbox::classify(&policy);
    Game {
        id,
        name: name.to_owned(),
        sort_name: crate::model::sort_name_of(name),
        name_is_user_set: false,
        root: root.map(Path::to_path_buf),
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy,
        policy_updated_at: now,
        launch: LaunchConfig::default(),
        umu_id: None,
        store: None,
        runtime: RuntimeRef::External(proton.to_path_buf()),
        runtime_follow_updates: false,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::empty(),
        last_launched_at: None,
        steam_app_id: id.steam_app_id(),
        steam_app_id_is_user_set: false,
        executables: vec![Executable {
            id: ExecutableId::from_uuid(id.as_uuid()),
            role: ExeRole::Primary,
            path: program.to_path_buf(),
            canonical_path: program.to_path_buf(),
            rel_path: None,
            mount_point: None,
            fingerprint: Fingerprint {
                size: 0,
                mtime_ns: 0,
                quick_hash: [0; 32],
                full_hash: None,
            },
            pe: PeInfo::unknown(),
            args_override: None,
            label: None,
            added_at: now,
        }],
    }
}

/// Adjust a planned Steam prefix tool session: there is no `games` row, so the session is
/// recorded without a game (its name is kept, like a removed game's history) and the
/// prefix is not marked as having run untrusted code (Steam's prefix is not EasyPlay's to
/// track; the game Steam runs there is outside EasyPlay's protection anyway).
pub fn steam_tool_plan(plan: &mut crate::launch::LaunchPlan) {
    plan.session.game_id = None;
    plan.host_actions
        .retain(|a| !matches!(a, crate::sandbox::plan::HostAction::MarkTainted { .. }));
}
