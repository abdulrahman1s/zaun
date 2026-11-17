//! Programs run in a Steam game's Windows storage (ADR-0031 §6): Valve's Proton folders are
//! usable builds, and the game a prefix tool runs under plans through the sandbox with the
//! Steam prefix, offline, without a `games` row and without marking the prefix.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::GameId;
use easyplay_core::launch::{HostEnv, LaunchTarget, PlanRequest, plan_launch_in_prefix};
use easyplay_core::model::{NetworkAccess, RuntimeOrigin, SlrVariant, Tool};
use easyplay_core::runtime::discovery::{name_of_folder, validate_build};
use easyplay_core::sandbox::plan::HostAction;
use easyplay_core::service::flows::steam::{steam_tool_plan, tool_game};
use easyplay_testkit::MemFs;
use easyplay_testkit::fixtures::hosts::{self, SandboxWorld};

/// Valve's own builds live in folders such as `Proton - Experimental`.
#[test]
fn a_valve_proton_folder_is_a_usable_build() {
    let fs = MemFs::new();
    let dir = Path::new("/home/p/.local/share/Steam/steamapps/common/Proton - Experimental");
    fs.file(
        dir.join("toolmanifest.vdf"),
        r#""manifest" { "commandline" "/proton %verb%" "require_tool_appid" "1628350" "compatmanager_layer_name" "proton" }"#,
    );
    fs.file(dir.join("proton"), "#!/usr/bin/env python3\n");
    let b = validate_build(
        &fs,
        dir,
        RuntimeOrigin::External,
        "Custom",
        Some(Path::new("/home/p")),
        Timestamp::from_millis(0),
    )
    .unwrap();
    assert_eq!(b.name.as_str(), "Proton-Experimental");
    assert_eq!(b.path, dir);
    assert_eq!(b.required_slr, SlrVariant::Steamrt3);
    assert_eq!(name_of_folder("Proton 9.0").unwrap().as_str(), "Proton-9.0");
    assert_eq!(name_of_folder("  ???  "), None);
}

/// Windows settings: the game's install folder is the read-only game folder, the program a
/// path inside it (`flows::steam` does exactly this).
#[test]
fn windows_settings_plans_under_a_steam_root() {
    let w = SandboxWorld::worked_example();
    let steam = Path::new(hosts::HOME).join(".local/share/Steam");
    let install = steam.join("steamapps/common/Red Dead Redemption 2");
    w.fs.dir(&install);
    plan_in_steam_prefix(
        &w,
        &install.join("winecfg.exe"),
        Some(&install),
        &Tool::Winecfg,
    );
}

/// A mod installer in Downloads: the file alone (Downloads is a shared location).
#[test]
fn a_program_from_downloads_plans_under_a_steam_root() {
    let w = SandboxWorld::worked_example();
    let setup = Path::new(hosts::HOME).join("Downloads/setup.exe");
    w.fs.file(&setup, b"MZ");
    let tool = Tool::RunExe {
        path: setup.clone(),
        args: vec![],
    };
    plan_in_steam_prefix(&w, &setup, None, &tool);
}

fn plan_in_steam_prefix(w: &SandboxWorld, program: &Path, root: Option<&Path>, tool: &Tool) {
    let steam = Path::new(hosts::HOME).join(".local/share/Steam");
    let compat = steam.join("steamapps/compatdata/1174180");
    w.fs.dir(compat.join("pfx/drive_c/windows/system32"));
    let proton = steam.join("steamapps/common/Proton - Experimental");
    let id = GameId::steam(1_174_180);
    let game = tool_game(
        id,
        "Red Dead Redemption 2",
        program,
        root,
        &proton,
        Timestamp::from_millis(1_794_480_000_000),
    );
    assert_eq!(game.policy.network, NetworkAccess::None);
    let env = HostEnv {
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        home: Path::new(hosts::HOME),
        paths: &w.paths,
        host_env: &w.host_env,
        install_id: hosts::INSTALL_ID,
        init_binary: &w.init,
        developer_mode: false,
        seccomp_available: true,
        verbose_log: false,
        global_env: &[],
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
    };
    let req = PlanRequest {
        session_id: hosts::SESSION_ID.parse().unwrap(),
        target: LaunchTarget::Tool(tool),
        game: Some(&game),
        runtime: Some(&w.runtime),
        proton_codename: None,
        network_override: None,
        host: &w.host,
        env: &env,
    };
    let mut planned = plan_launch_in_prefix(&req, &compat).unwrap();
    let args: Vec<String> = planned
        .plan
        .sandbox
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.iter().any(|a| *a == compat.display().to_string()));
    assert!(planned.sandbox.namespaces.net, "offline");
    steam_tool_plan(&mut planned.plan);
    assert_eq!(planned.plan.session.game_id, None);
    assert!(
        !planned
            .plan
            .host_actions
            .iter()
            .any(|a| matches!(a, HostAction::MarkTainted { .. }))
    );
}
