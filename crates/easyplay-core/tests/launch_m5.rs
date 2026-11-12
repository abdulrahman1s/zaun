//! M5 launch planning: the planner for every session kind, plan file round trip and
//! version handling, plan.txt, the shell rendering, preflight failures, and the agreement
//! of `umu::env` with the sandbox planner (LAUNCH-001…003, LAUNCH-006, UMU-001/002).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use easyplay_core::clock::Timestamp;
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{Fs, FsErrorKind, FsOp};
use easyplay_core::host::{HostFacts, ToolVersion};
use easyplay_core::ids::{ExecutableId, SessionId};
use easyplay_core::launch::plan_file::{self, PlanFileError};
use easyplay_core::launch::preflight::STAT_DEADLINE;
use easyplay_core::launch::{
    self, FingerprintCheck, HostEnv, LaunchPlan, LaunchTarget, NoLocks, PlanRequest, PlannedLaunch,
    PrefixLocks, PreflightFailure, PreflightInput,
};
use easyplay_core::logging::Redactor;
use easyplay_core::model::{
    CompatOptions, EnvVar, ExeRole, Executable, Fingerprint, FolderAccess, FolderGrant, Game,
    GameFlags, GrantOrigin, LaunchConfig, NetworkAccess, PeInfo, Renderer, RuntimeRef, SessionKind,
    SyncMode, Tool, WindowSystem,
};
use easyplay_core::sandbox::plan::EnvOrigin;
use easyplay_core::sandbox::{self, ValidationInputs};
use easyplay_core::umu;
use easyplay_testkit::fixtures::hosts::{self, SandboxWorld};

fn game_of(w: &SandboxWorld) -> Game {
    let meta = w.fs.metadata_follow(&w.exe).unwrap();
    let now = Timestamp::from_millis(1_794_480_000_000);
    let exe = Executable {
        id: "0192f0c2-1c3a-7d4e-8f00-0000000000e1".parse().unwrap(),
        role: ExeRole::Primary,
        path: w.exe.clone(),
        canonical_path: w.exe.clone(),
        rel_path: None,
        mount_point: Some("/mnt/games".into()),
        fingerprint: Fingerprint {
            size: meta.size,
            mtime_ns: meta.mtime_ns,
            quick_hash: [0; 32],
            full_hash: None,
        },
        pe: PeInfo::default(),
        args_override: None,
        label: None,
        added_at: now,
    };
    Game {
        id: w.game_id,
        name: "Hollow Knight".into(),
        sort_name: "hollow knight".into(),
        name_is_user_set: false,
        root: w.root.clone(),
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy: sandbox::offline(),
        policy_updated_at: now,
        launch: LaunchConfig {
            args: vec!["-windowed".into()],
            ..LaunchConfig::default()
        },
        umu_id: Some("umu-367520".into()),
        store: None,
        runtime: RuntimeRef::Default,
        runtime_follow_updates: true,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::default(),
        last_launched_at: None,
        steam_app_id: None,
        steam_app_id_is_user_set: false,
        executables: vec![exe],
    }
}

fn sid() -> SessionId {
    hosts::SESSION_ID.parse().unwrap()
}

fn env_of(w: &SandboxWorld) -> HostEnv<'_> {
    HostEnv {
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
    }
}

fn plan(
    w: &SandboxWorld,
    game: &Game,
    target: LaunchTarget<'_>,
    codename: Option<&str>,
    net: Option<NetworkAccess>,
) -> Result<PlannedLaunch, launch::LaunchPlanError> {
    let env = env_of(w);
    let provision = target.kind() == SessionKind::Provision;
    launch::plan_launch(&PlanRequest {
        session_id: sid(),
        target,
        game: Some(game),
        runtime: (!provision).then_some(&w.runtime),
        proton_codename: codename,
        network_override: net,
        host: &w.host,
        env: &env,
    })
}

fn tail_after_dashes(p: &LaunchPlan) -> Vec<String> {
    let i = p.sandbox.command.iter().position(|a| a == "--").unwrap();
    p.sandbox.command[i + 1..]
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn every_session_kind_plans() {
    let w = SandboxWorld::worked_example();
    let g = game_of(&w);
    let exe = &g.executables[0];
    let winecfg = Tool::Winecfg;
    let run_exe = Tool::RunExe {
        path: w.exe.clone(),
        args: vec!["--patch".into()],
    };
    let none: [String; 0] = [];
    let extra = ["-extra".to_owned()];
    let umu_run = "/usr/bin/umu-run".to_owned();
    let exe_s = w.exe.display().to_string();
    type Case<'a> = (
        LaunchTarget<'a>,
        Option<&'a str>,
        Option<NetworkAccess>,
        Vec<String>,
        NetworkAccess,
    );
    let cases: Vec<Case<'_>> = vec![
        (
            LaunchTarget::Game {
                exe,
                extra_args: &extra,
            },
            None,
            None,
            vec![
                umu_run.clone(),
                exe_s.clone(),
                "-windowed".into(),
                "-extra".into(),
            ],
            NetworkAccess::None,
        ),
        (
            LaunchTarget::Installer { exe },
            None,
            None,
            vec![umu_run.clone(), exe_s.clone()],
            NetworkAccess::None,
        ),
        (
            LaunchTarget::Tool(&winecfg),
            None,
            None,
            vec![umu_run.clone(), "winecfg".into()],
            NetworkAccess::None,
        ),
        (
            LaunchTarget::Tool(&run_exe),
            None,
            None,
            vec![umu_run.clone(), exe_s.clone(), "--patch".into()],
            NetworkAccess::None,
        ),
        (
            LaunchTarget::Prepare,
            None,
            None,
            vec![umu_run.clone(), String::new()],
            NetworkAccess::None,
        ),
        (
            LaunchTarget::Prepare,
            Some("GE-Proton"),
            Some(NetworkAccess::Full),
            vec![umu_run.clone(), String::new()],
            NetworkAccess::Full,
        ),
        (
            LaunchTarget::Provision,
            Some("GE-Proton"),
            None,
            vec![umu_run.clone(), String::new()],
            NetworkAccess::Full,
        ),
    ];
    let _ = none;
    for (target, code, net, argv, network) in cases {
        let kind = target.kind();
        let p = plan(&w, &g, target, code, net).unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
        assert_eq!(tail_after_dashes(&p.plan), argv, "{kind:?}");
        assert_eq!(p.plan.session.kind, kind);
        assert_eq!(p.plan.session.counted_eligible, kind == SessionKind::Game);
        assert_eq!(p.plan.context.network, Some(network), "{kind:?}");
        let env = &p.sandbox.env;
        assert_eq!(
            env.get("UMU_RUNTIME_UPDATE").is_some(),
            kind != SessionKind::Provision,
            "{kind:?}"
        );
        for (k, v) in &p.umu.env {
            assert_eq!(env.get(k), Some(v.as_str()), "{kind:?} {k}");
        }
        for name in ["container", "UMU_ZENITY", "DBUS_SESSION_BUS_ADDRESS"] {
            assert!(env.get(name).is_none());
        }
        match kind {
            SessionKind::Provision => {
                assert_eq!(p.plan.session.game_id, None);
                assert_eq!(env.get("WINEPREFIX"), Some(umu::PROVISION_PREFIX));
                assert_eq!(env.get("PROTONPATH"), Some("GE-Proton"));
                assert_eq!(p.plan.paths.prefix_lock, None);
            }
            _ => {
                assert_eq!(p.plan.session.game_id, Some(g.id));
                assert_eq!(env.get("GAMEID"), Some("umu-367520"));
                assert_eq!(p.plan.paths.prefix_lock, Some(w.paths.prefix_lock(g.id)));
            }
        }
        assert_eq!(p.plan.paths.runtime_dir, w.paths.run_session_dir(sid()));
        assert_eq!(p.plan.bwrap, PathBuf::from("/usr/bin/bwrap"));
        assert_eq!(p.plan.sandbox.args, p.rendered.args);
    }
    // The executable is identified for game kinds.
    let p = plan(&w, &g, LaunchTarget::Installer { exe }, None, None).unwrap();
    assert_eq!(p.plan.session.executable_id, Some(exe.id));
    assert_eq!(p.plan.context.preset, Some(g.policy.preset));
    assert_eq!(
        p.plan.paths.session_var,
        Some(w.paths.session_var_dir(sid()))
    );
}

#[test]
fn planning_rejections_carry_codes() {
    let w = SandboxWorld::worked_example();
    let g = game_of(&w);
    let bad = Tool::Winetricks {
        verbs: vec!["x;y".into()],
    };
    let e = plan(&w, &g, LaunchTarget::Tool(&bad), None, None).unwrap_err();
    assert_eq!(Error::from(e).code, ErrorCode::LibInvalidField);
    // A grant to a missing folder is a sandbox issue with its own code.
    let mut g2 = g.clone();
    g2.policy.folders.push(FolderGrant {
        path: "/mnt/nowhere".into(),
        canonical: "/mnt/nowhere".into(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: Timestamp::from_millis(0),
    });
    let e = plan(
        &w,
        &g2,
        LaunchTarget::Game {
            exe: &g.executables[0],
            extra_args: &[],
        },
        None,
        None,
    )
    .unwrap_err();
    assert!(!e.issues().is_empty());
    assert_eq!(Error::from(e).code, ErrorCode::SbxGrantMissing);
    // Without bwrap or umu.
    let mut w2 = SandboxWorld::worked_example();
    w2.host.bwrap = None;
    let e = plan(&w2, &g, LaunchTarget::Prepare, None, None).unwrap_err();
    assert_eq!(Error::from(e).code, ErrorCode::HostBwrapNotFound);
    // A codename for a game session is refused.
    let e = plan(&w, &g, LaunchTarget::Provision, None, None).unwrap_err();
    assert!(matches!(e, launch::LaunchPlanError::Request(_)));
}

#[test]
fn plan_file_round_trip_and_versions() {
    let w = SandboxWorld::worked_example();
    let g = game_of(&w);
    let p = plan(
        &w,
        &g,
        LaunchTarget::Game {
            exe: &g.executables[0],
            extra_args: &[],
        },
        None,
        None,
    )
    .unwrap()
    .plan;
    let path = w.paths.plan_file(sid());
    plan_file::write(&w.fs, &path, &p).unwrap();
    assert_eq!(w.fs.metadata(&path).unwrap().mode, 0o600);
    assert_eq!(
        w.fs.metadata(path.parent().unwrap()).unwrap().mode & 0o777,
        0o700
    );
    assert_eq!(plan_file::read(&w.fs, &path).unwrap(), p);
    // Descriptor payloads survive as base64 and decode to the planner's bytes.
    let passwd = p
        .sandbox
        .fds
        .iter()
        .find(|f| f.target.as_deref() == Some(Path::new("/etc/passwd")))
        .unwrap();
    assert!(
        String::from_utf8(passwd.content().unwrap())
            .unwrap()
            .starts_with("sam:x:1000")
    );
    // The JSON shape of DATA_MODEL §8.
    let v: serde_json::Value = serde_json::from_slice(&plan_file::to_json(&p).unwrap()).unwrap();
    assert_eq!(v["format"], 1);
    assert_eq!(v["session"]["kind"], "game");
    assert_eq!(v["context"]["network"], "none");
    assert_eq!(v["context"]["isolation"], "isolated");
    assert!(v["sandbox"]["fds"][0]["content_b64"].is_string());
    assert!(v["host_actions"].is_array());
    // Unknown format → typed error → EP-SES-002.
    let mut v2 = v.clone();
    v2["format"] = 2.into();
    let err = plan_file::from_json(&serde_json::to_vec(&v2).unwrap()).unwrap_err();
    assert!(matches!(err, PlanFileError::UnknownFormat { found: 2 }));
    assert_eq!(Error::from(err).code, ErrorCode::SesPlanVersionMismatch);
    // A future format with a different shape still reports the version.
    let err = plan_file::from_json(br#"{"format": 9, "whatever": []}"#).unwrap_err();
    assert!(matches!(err, PlanFileError::UnknownFormat { found: 9 }));
    assert!(matches!(
        plan_file::from_json(b"{}").unwrap_err(),
        PlanFileError::Invalid(_)
    ));
    assert!(matches!(
        plan_file::from_json(b"not json").unwrap_err(),
        PlanFileError::Invalid(_)
    ));
}

#[test]
fn plan_text_is_redacted_and_complete() {
    let w = SandboxWorld::worked_example();
    let g = game_of(&w);
    let p = plan(&w, &g, LaunchTarget::Prepare, None, None)
        .unwrap()
        .plan;
    let r = Redactor::new(Some(Path::new(hosts::HOME)), Some("sam"), Some("host1"));
    let text = plan_file::render_text(&p, &r);
    assert!(!text.contains("/home/sam"), "{text}");
    for needle in [
        "kind         prepare",
        "# environment",
        "UMU_RUNTIME_UPDATE=0",
        "# mounts",
        "ro-bind",
        "# sandbox options",
        "--unshare-net",
        "umu-run: /usr/bin/umu-run \"\"",
    ] {
        assert!(text.contains(needle), "missing {needle}\n{text}");
    }
    // Option grouping keeps modifiers with their option.
    let groups = plan_file::group_args(&p.sandbox.args);
    assert!(
        groups
            .iter()
            .any(|g| g.len() == 4 && g[0] == "--perms" && g[2] == "--tmpfs")
    );
    assert_eq!(
        groups.iter().map(Vec::len).sum::<usize>(),
        p.sandbox.args.len()
    );
}

/// DEVELOPER_MODE §9 "shell rendering round trip": the generated script, run by a POSIX
/// shell with `printf` in place of bwrap, yields the original argument vector, for hostile
/// paths.
#[test]
#[allow(clippy::disallowed_methods)]
fn shell_rendering_round_trip() {
    let mut w = SandboxWorld::worked_example();
    let hostile = PathBuf::from("/mnt/games/Hollow Knight/it's \"$HOME\" `x` \\ -n\nnew.exe");
    w.fs.file(&hostile, b"MZ");
    w.exe = hostile.clone();
    let mut g = game_of(&w);
    g.launch.args = vec!["--a b".into(), "'".into(), String::new()];
    let p = plan(
        &w,
        &g,
        LaunchTarget::Game {
            exe: &g.executables[0],
            extra_args: &[],
        },
        None,
        None,
    )
    .unwrap()
    .plan;
    let script = launch::render_shell(&p, "2026-11-12T00:00:00Z");
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("exec 8<<'EOF_EASYPLAY_8'"));
    assert!(!script.contains("--json-status-fd") && !script.contains("--sync-fd"));
    assert!(!script.contains("--status-fd"));
    // Only the bwrap invocation, with printf instead of bwrap, and no other side effect.
    let start = script.find("exec '/usr/bin/bwrap'").unwrap();
    let call = script[start..].replacen("exec '/usr/bin/bwrap'", "printf '%s\\0'", 1);
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&call)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
    let mut expected: Vec<Vec<u8>> = Vec::new();
    for gr in plan_file::group_args(&p.sandbox.args) {
        let o = gr[0].to_str().unwrap();
        if o == "--json-status-fd" || o == "--sync-fd" {
            continue;
        }
        expected.extend(gr.iter().map(|a| a.as_encoded_bytes().to_vec()));
    }
    expected.push(b"--".to_vec());
    let mut cmd = p.sandbox.command.clone();
    cmd.drain(1..3);
    expected.extend(cmd.iter().map(|a| a.as_encoded_bytes().to_vec()));
    let got: Vec<Vec<u8>> = got[..got.len() - 1].iter().map(|s| s.to_vec()).collect();
    assert_eq!(got, expected);
    assert!(expected.contains(&hostile.as_os_str().as_encoded_bytes().to_vec()));
    assert_seccomp_reproduced(&p, &script);
}

/// The script recreates the system-call filter (the same program the spawner loads) instead
/// of dropping it: the reproduction is never weaker than the launch.
#[allow(clippy::disallowed_methods)]
fn assert_seccomp_reproduced(p: &LaunchPlan, script: &str) {
    use easyplay_core::sandbox::seccomp;
    let Some(fd) = p
        .sandbox
        .fds
        .iter()
        .find(|f| f.kind == plan_file::FdKind::Seccomp)
    else {
        panic!("the worked example plans the system-call filter");
    };
    assert!(
        script.contains(&format!("'--seccomp' '{}'", fd.fd)),
        "{script}"
    );
    if !seccomp::AVAILABLE {
        assert!(script.contains("exit 1"), "{script}");
        return;
    }
    // Run the payload part of the script and read the descriptor back.
    let start = script.find("# system-call filter").unwrap();
    let end = start + script[start..].find("rm -f").unwrap();
    let body = format!("{}cat <&{} | base64\n", &script[start..end], fd.fd);
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(body)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let b64: String = String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .collect();
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    let expected = seccomp::program(seccomp::Profile::for_args(&p.sandbox.args)).unwrap();
    assert_eq!(bytes, expected);
}

/// Table C as planned equals `umu::env::compat_vars`, and table B is one list.
#[test]
fn umu_env_is_the_planner_source() {
    assert_eq!(
        umu::env::HOST_PASSTHROUGH,
        sandbox::sections::env::HOST_PASSTHROUGH
    );
    let mut w = SandboxWorld::worked_example();
    w.developer_mode = true;
    w.launch.compat = CompatOptions {
        renderer: Renderer::OpenGl,
        window_system: WindowSystem::Wayland,
        sync: SyncMode::NoEsyncNoFsync,
        disable_fixes: true,
        dll_overrides: "d3d9=n,b".into(),
        mangohud: true,
        dxvk_hud: Some("fps".into()),
        locale: Some("ja_JP.UTF-8".into()),
        proton_log: true,
        wine_debug: Some("+seh".into()),
    };
    let p = sandbox::plan(&sandbox::offline(), &w.ctx(), &w.host).unwrap();
    let mut planned: Vec<(String, String)> = p
        .env
        .set
        .iter()
        .filter(|(_, e)| matches!(e.origin, EnvOrigin::Option(_)))
        .map(|(k, e)| (k.clone(), e.value.clone()))
        .collect();
    planned.sort();
    let mut table: Vec<(String, String)> = umu::compat_vars(
        &w.launch.compat,
        umu::env::CompatContext {
            wayland: true,
            developer_mode: true,
        },
    )
    .into_iter()
    .map(|v| (v.name.to_owned(), v.value))
    .collect();
    table.sort();
    assert_eq!(planned, table);
    // Table D through the sandbox's names.
    assert_eq!(
        sandbox::validate::classify_user_var("LD_PRELOAD"),
        umu::EnvRule::Dangerous
    );
    assert_eq!(umu::PROVISION_PREFIX, sandbox::plan::PROVISION_PREFIX);
    // A Developer Mode user variable appears; a reserved one never does.
    let mut w = SandboxWorld::worked_example();
    w.launch.env = vec![
        EnvVar {
            name: "MY_VAR".into(),
            value: "1".into(),
        },
        EnvVar {
            name: "UMU_ZENITY".into(),
            value: "1".into(),
        },
    ];
    let r = sandbox::plan(&sandbox::offline(), &w.ctx(), &w.host);
    assert!(
        r.is_err(),
        "reserved user variables are rejected before planning"
    );
}

// ---- preflight ------------------------------------------------------------------------

/// `Arc<dyn Fs>` view of a world's `MemFs` (preflight runs checks on helper threads).
struct WorldFs(Arc<SandboxWorld>);

macro_rules! delegate {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        impl Fs for WorldFs {
            $(fn $name(&self, $($a: $t),*) -> $r { self.0.fs.$name($($a),*) })*
        }
    };
}

use easyplay_core::fsx::{DirEntry, FsResult, Metadata};
delegate! {
    metadata(p: &Path) -> FsResult<Metadata>;
    metadata_follow(p: &Path) -> FsResult<Metadata>;
    read_link(p: &Path) -> FsResult<PathBuf>;
    read(p: &Path, l: u64) -> FsResult<Vec<u8>>;
    read_follow(p: &Path, l: u64) -> FsResult<Vec<u8>>;
    read_range(p: &Path, o: u64, l: usize) -> FsResult<Vec<u8>>;
    open_read(p: &Path) -> FsResult<Box<dyn std::io::Read + Send>>;
    read_dir(p: &Path) -> FsResult<Vec<DirEntry>>;
    canonicalize(p: &Path) -> FsResult<PathBuf>;
    create_dir(p: &Path, m: u32) -> FsResult<()>;
    create_dir_all(p: &Path, m: u32) -> FsResult<()>;
    write_new(p: &Path, d: &[u8], m: u32) -> FsResult<()>;
    atomic_write(p: &Path, d: &[u8], m: u32) -> FsResult<()>;
    rename(a: &Path, b: &Path) -> FsResult<()>;
    remove_file(p: &Path) -> FsResult<()>;
    remove_dir(p: &Path) -> FsResult<()>;
    symlink(a: &Path, b: &Path) -> FsResult<()>;
    set_mode(p: &Path, m: u32) -> FsResult<()>;
    free_space(p: &Path) -> FsResult<u64>;
    reflink_supported(a: &Path, b: &Path) -> FsResult<bool>;
}

struct Held;
impl PrefixLocks for Held {
    fn is_held(&self, _: &Path) -> bool {
        true
    }
}

fn run_preflight(
    w: &Arc<SandboxWorld>,
    g: &Game,
    host: &HostFacts,
    locks: &dyn PrefixLocks,
) -> Result<launch::PreflightReport, PreflightFailure> {
    let env = env_of(w);
    let req = PlanRequest {
        session_id: sid(),
        target: LaunchTarget::Game {
            exe: &g.executables[0],
            extra_args: &[],
        },
        game: Some(g),
        runtime: Some(&w.runtime),
        proton_codename: None,
        network_override: None,
        host,
        env: &env,
    };
    let fs: Arc<dyn Fs> = Arc::new(WorldFs(Arc::clone(w)));
    let lock = w.paths.prefix_lock(g.id);
    launch::with_context(&req, |ctx, _, _| {
        launch::preflight(&PreflightInput {
            game: g,
            exe: &g.executables[0],
            kind: SessionKind::Game,
            host,
            validation: ValidationInputs::from_context(ctx, host),
            fs: Arc::clone(&fs),
            mounts: &w.mounts,
            init_binary: &w.init,
            prefix_lock: &lock,
            locks,
            data_dir: &w.paths.data_dir,
            isolation: Some((w.runtime.isolation, host.fs.reflink_ok)),
            deadline: STAT_DEADLINE,
        })
    })
    .unwrap()
}

fn ready_world() -> Arc<SandboxWorld> {
    let w = SandboxWorld::worked_example();
    w.fs.file(&w.init, b"\x7fELF");
    w.fs.set_mode(&w.init, 0o755).unwrap();
    w.fs.set_free_space(100 << 30);
    Arc::new(w)
}

#[test]
fn preflight_failure_table() {
    let w = ready_world();
    let g = game_of(&w);
    let ok = run_preflight(&w, &g, &w.host, &NoLocks).unwrap();
    assert_eq!(ok.fingerprint, FingerprintCheck::Unchanged);
    assert!(ok.grant_changes.is_empty());
    assert_eq!(ok.low_space, None);

    let code = |r: Result<launch::PreflightReport, PreflightFailure>| r.unwrap_err().code();

    // An alternate executable of the game is fine.
    let mut g3 = g.clone();
    let mut alt = g.executables[0].clone();
    alt.id = ExecutableId::from_uuid(Default::default());
    alt.role = ExeRole::Alternate;
    g3.executables.insert(0, alt);
    assert!(run_preflight(&w, &g3, &w.host, &NoLocks).is_ok());
    // Missing executable; disconnected drive.
    let mut gone = g.clone();
    gone.executables[0].canonical_path = "/mnt/games/Hollow Knight/missing.exe".into();
    gone.executables[0].mount_point = Some("/mnt/games".into());
    assert_eq!(
        code(run_preflight(&w, &gone, &w.host, &NoLocks)),
        ErrorCode::LibExeNotFound
    );
    let mut away = g.clone();
    away.executables[0].canonical_path = "/media/usb/game.exe".into();
    away.executables[0].mount_point = Some("/media/usb".into());
    assert_eq!(
        code(run_preflight(&w, &away, &w.host, &NoLocks)),
        ErrorCode::LibDriveDisconnected
    );
    // Unreadable and hanging filesystems.
    w.fs.fail(FsOp::MetadataFollow, &w.exe, FsErrorKind::PermissionDenied);
    assert_eq!(
        code(run_preflight(&w, &g, &w.host, &NoLocks)),
        ErrorCode::LibExeUnreadable
    );
    w.fs.clear_faults();
    w.fs.fail(FsOp::MetadataFollow, &w.exe, FsErrorKind::TimedOut);
    assert_eq!(
        code(run_preflight(&w, &g, &w.host, &NoLocks)),
        ErrorCode::FsTimeout
    );
    w.fs.clear_faults();
    // Fingerprint changed → reported, not a failure.
    let mut changed = g.clone();
    changed.executables[0].fingerprint.size += 1;
    assert!(matches!(
        run_preflight(&w, &changed, &w.host, &NoLocks)
            .unwrap()
            .fingerprint,
        FingerprintCheck::Changed { .. }
    ));
    // Policy invalid on this host.
    let mut bad = g.clone();
    bad.policy.folders.push(FolderGrant {
        path: "/mnt/nowhere".into(),
        canonical: "/mnt/nowhere".into(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: Timestamp::from_millis(0),
    });
    assert_eq!(
        code(run_preflight(&w, &bad, &w.host, &NoLocks)),
        ErrorCode::SbxGrantMissing
    );
    // Grant target changed (a link that now points elsewhere).
    w.fs.dir("/mnt/data/new")
        .link("/mnt/data/current", "/mnt/data/new");
    let mut moved = g.clone();
    moved.policy.folders.push(FolderGrant {
        path: "/mnt/data/current".into(),
        canonical: "/mnt/data/old".into(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: Timestamp::from_millis(0),
    });
    let r = run_preflight(&w, &moved, &w.host, &NoLocks).unwrap();
    assert_eq!(r.grant_changes.len(), 1);
    assert_eq!(r.grant_changes[0].new, PathBuf::from("/mnt/data/new"));
    // Host requirements.
    let mut h = w.host.clone();
    h.bwrap = None;
    assert_eq!(
        code(run_preflight(&w, &g, &h, &NoLocks)),
        ErrorCode::HostBwrapNotFound
    );
    let mut h = w.host.clone();
    h.bwrap.as_mut().unwrap().userns_ok = false;
    assert_eq!(
        code(run_preflight(&w, &g, &h, &NoLocks)),
        ErrorCode::HostUsernsUnavailable
    );
    let mut h = w.host.clone();
    h.bwrap.as_mut().unwrap().nested_userns_ok = false;
    assert_eq!(
        code(run_preflight(&w, &g, &h, &NoLocks)),
        ErrorCode::HostNestedUsernsUnavailable
    );
    let mut h = w.host.clone();
    h.umu.as_mut().unwrap().version = Some(ToolVersion::new(1, 3, 0));
    assert_eq!(
        code(run_preflight(&w, &g, &h, &NoLocks)),
        ErrorCode::HostUmuTooOld
    );
    // Init shim missing.
    let w2 = Arc::new(SandboxWorld::worked_example());
    w2.fs.set_free_space(100 << 30);
    assert_eq!(
        code(run_preflight(&w2, &g, &w2.host, &NoLocks)),
        ErrorCode::HostInitMissing
    );
    // Already running.
    assert_eq!(
        code(run_preflight(&w, &g, &w.host, &Held)),
        ErrorCode::SesAlreadyRunning
    );
    // Low space is a dismissible warning.
    w.fs.set_free_space(100 << 20);
    let r = run_preflight(&w, &g, &w.host, &NoLocks).unwrap();
    assert_eq!(
        r.low_space,
        Some((100 << 20, launch::preflight::MIN_FREE_LAUNCH))
    );
}

#[test]
fn preflight_failures_map_to_errors() {
    let e = Error::from(PreflightFailure::ExeMissing("/g/a.exe".into()));
    assert_eq!(e.code, ErrorCode::LibExeNotFound);
    assert_eq!(e.context_value("path"), Some("/g/a.exe"));
    assert_eq!(
        PreflightFailure::AlreadyRunning.code(),
        ErrorCode::SesAlreadyRunning
    );
    assert_eq!(
        PreflightFailure::Host(ErrorCode::HostNoDisplay).code(),
        ErrorCode::HostNoDisplay
    );
    let mut h = hosts::arch_mesa();
    h.display.wayland = None;
    h.display.x11 = None;
    assert_eq!(
        launch::preflight::host_requirements(&h, SessionKind::Game),
        Err(ErrorCode::HostNoDisplay)
    );
    assert_eq!(
        launch::preflight::host_requirements(&h, SessionKind::Prepare),
        Ok(())
    );
}
