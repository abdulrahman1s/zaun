//! Serde round trips and validation tables for the domain model.

#![allow(clippy::type_complexity)]

use std::fmt::Debug;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::*;
use crate::clock::Timestamp;
use crate::ids::{ExecutableId, GameId, PromptId, RuntimeName, SessionId, TagId, TagName};

/// The example document of DATA_MODEL §3, verbatim.
const EXAMPLE_POLICY: &str = r#"{
  "schema_version": 1,
  "preset": {"custom": {"base": "online"}},
  "network": "full",
  "game_dir": "read_write",
  "folders": [
    {"path": "/home/sam/Music", "canonical": "/home/sam/Music", "access": "read_only",
     "origin": {"well_known": "music"}, "drive_letter": "m", "granted_at": 1794480000000}
  ],
  "save_data": "internal",
  "removable": "none",
  "devices": {"audio": "full", "alsa_devices": false, "controllers": "detected", "camera": false},
  "display": {"wayland": "auto", "x11": "auto", "isolation": "none"},
  "ipc": {"session_bus": "none"},
  "hardening": {"seccomp": "default", "mask_hostname": true, "mask_machine_id": true,
                "mask_hardware_ids": true, "runtime_isolation": "auto"},
  "exceptions": []
}"#;

/// Remove insignificant whitespace from JSON text without reordering anything.
fn minify(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut in_str = false;
    let mut escaped = false;
    for c in json.chars() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

fn rt<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let json = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, value, "{json}");
    // And through a `Value`, as storage does for JSON columns.
    let v = serde_json::to_value(value).unwrap();
    let back: T = serde_json::from_value(v).unwrap();
    assert_eq!(&back, value);
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms)
}

fn gid() -> GameId {
    "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap()
}

fn eid() -> ExecutableId {
    "0192f0c2-7b1e-7c3a-9d2f-000000000001".parse().unwrap()
}

pub(super) fn example_policy() -> SandboxPolicy {
    serde_json::from_str(EXAMPLE_POLICY).unwrap()
}

fn offline_policy() -> SandboxPolicy {
    SandboxPolicy {
        schema_version: 1,
        preset: Preset::Offline,
        network: NetworkAccess::None,
        game_dir: GameDirAccess::ReadWrite,
        folders: vec![],
        save_data: SaveDataLocation::Internal,
        removable: RemovableAccess::None,
        devices: DeviceAccess {
            audio: AudioAccess::Playback,
            alsa_devices: false,
            controllers: ControllerAccess::Detected,
            camera: false,
        },
        display: DisplayAccess {
            wayland: SocketAccess::Auto,
            x11: SocketAccess::Auto,
            isolation: DisplayIsolation::None,
        },
        ipc: IpcAccess {
            session_bus: SessionBusAccess::None,
        },
        hardening: Hardening {
            seccomp: SeccompMode::Default,
            mask_hostname: true,
            mask_machine_id: true,
            mask_hardware_ids: true,
            runtime_isolation: RuntimeIsolation::Auto,
        },
        exceptions: vec![],
    }
}

fn fingerprint() -> Fingerprint {
    Fingerprint {
        size: 123_456,
        mtime_ns: 1_794_480_000_123_456_789,
        quick_hash: [0xab; 32],
        full_hash: Some([0x01; 32]),
    }
}

fn executable(role: ExeRole) -> Executable {
    Executable {
        id: eid(),
        role,
        path: "/mnt/games/Hollow Knight/hollow_knight.exe".into(),
        canonical_path: "/mnt/games/Hollow Knight/hollow_knight.exe".into(),
        rel_path: Some("hollow_knight.exe".into()),
        mount_point: Some("/mnt/games".into()),
        fingerprint: fingerprint(),
        pe: PeInfo {
            product_name: Some("Hollow Knight".into()),
            machine: PeMachine::X86_64,
            subsystem: PeSubsystem::Gui,
            timestamp: Some(1_500_000_000),
            has_icon: true,
            notes: vec!["no version resource".into()],
            ..PeInfo::default()
        },
        args_override: Some(vec!["-windowed".into()]),
        label: Some("DirectX 12".into()),
        added_at: ts(1_794_480_000_000),
    }
}

fn game() -> Game {
    Game {
        id: gid(),
        name: "Hollow Knight".into(),
        sort_name: sort_name_of("Hollow Knight"),
        name_is_user_set: false,
        root: Some("/mnt/games/Hollow Knight".into()),
        added_at: ts(1_794_480_000_000),
        updated_at: ts(1_794_480_000_500),
        favorite: true,
        hidden: false,
        notes: "Beat the Radiance.\nNext: Godhome.".into(),
        policy: example_policy(),
        policy_updated_at: ts(1_794_480_000_000),
        launch: LaunchConfig::default(),
        umu_id: Some("umu-367520".into()),
        store: Some(Store::Gog),
        runtime: RuntimeRef::Channel(Channel::GeProton),
        runtime_follow_updates: true,
        runtime_last_resolved: Some(RuntimeName::new("GE-Proton10-25").unwrap()),
        prefix_tainted_at: Some(ts(1_794_480_100_000)),
        prefix_prepared_with: Some(RuntimeName::new("GE-Proton10-25").unwrap()),
        flags: GameFlags::X11_NOTICE_ACK | GameFlags::BROAD_GRANT_ACK,
        last_launched_at: None,
        steam_app_id: None,
        steam_app_id_is_user_set: false,
        executables: vec![executable(ExeRole::Primary)],
    }
}

#[test]
fn example_policy_round_trips_byte_identically() {
    let policy = example_policy();
    assert_eq!(
        policy.preset,
        Preset::Custom {
            base: BasePreset::Online
        }
    );
    assert_eq!(
        policy.folders[0].origin,
        GrantOrigin::WellKnown(UserDir::Music)
    );
    assert_eq!(policy.folders[0].drive_letter, Some('m'));
    let out = serde_json::to_string(&policy).unwrap();
    assert_eq!(out, minify(EXAMPLE_POLICY));
    policy.validate().unwrap();
}

#[test]
fn policy_round_trips() {
    rt(&offline_policy());
    let mut p = example_policy();
    p.save_data = SaveDataLocation::External {
        dir: "/home/sam/Saves".into(),
    };
    p.exceptions = vec![
        PolicyException::ExtraReadOnlyPath {
            path: "/opt/fonts".into(),
            canonical: "/opt/fonts".into(),
            reason: "Custom fonts".into(),
        },
        PolicyException::PassEnv {
            name: "SDL_VIDEODRIVER".into(),
            reason: "Needed".into(),
        },
        PolicyException::ExtraDevice {
            path: "/dev/ttyUSB0".into(),
            reason: "Wheel".into(),
        },
    ];
    p.folders.push(FolderGrant {
        path: "/data".into(),
        canonical: "/data".into(),
        access: FolderAccess::ReadWrite,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: ts(5),
    });
    rt(&p);
    p.validate().unwrap();
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        v["save_data"],
        serde_json::json!({"external": {"dir": "/home/sam/Saves"}})
    );
    assert_eq!(v["exceptions"][1]["pass_env"]["name"], "SDL_VIDEODRIVER");
    assert_eq!(v["folders"][1]["origin"], "user_chosen");
}

#[test]
fn permissions_are_the_policys_bits_and_write_back_exactly() {
    use Permissions as P;
    // The example grants what Offline and Online grant by default, the internet and the
    // microphone.
    let example = example_policy();
    assert_eq!(
        example.permissions(),
        P::INTERNET | P::SOUND | P::MICROPHONE | P::CONTROLLERS
    );
    assert_eq!(offline_policy().permissions(), P::SOUND | P::CONTROLLERS);

    // Every set of bits writes to a policy that reads back as the same set, and touches
    // nothing a bit does not stand for.
    for bits in 0..=P::all().bits() {
        let granted = P::from_bits_retain(bits);
        let mut p = example.clone();
        p.set_permissions(granted);
        assert_eq!(p.permissions(), granted.normalized(), "{granted:?}");
        let mut rest = p.clone();
        rest.set_permissions(example.permissions());
        assert_eq!(rest, example, "{granted:?}");
    }

    // A degree grants nothing without its permission, and is the wider grant with it.
    assert_eq!(P::ALL_INPUT.normalized(), P::empty());
    assert_eq!((P::REMOVABLE_WRITE | P::CAMERA).normalized(), P::CAMERA);
    assert_eq!(P::MICROPHONE.normalized(), P::empty());
    assert_eq!(
        (P::CONTROLLERS | P::REMOVABLE | P::SOUND).degrees(),
        P::ALL_INPUT | P::REMOVABLE_WRITE | P::MICROPHONE
    );
    assert_eq!(P::INTERNET.degrees(), P::empty());
    // Each degree brings the permission it widens.
    assert_eq!(
        (P::ALL_INPUT | P::REMOVABLE_WRITE | P::MICROPHONE).prerequisites(),
        P::CONTROLLERS | P::REMOVABLE | P::SOUND
    );
    for degree in [P::ALL_INPUT, P::REMOVABLE_WRITE, P::MICROPHONE] {
        assert!(degree.prerequisites().degrees().contains(degree));
    }
    let mut p = example.clone();
    p.set_permissions(P::SOUND);
    assert_eq!(p.devices.audio, AudioAccess::Playback);
    p.set_permissions(P::SOUND | P::MICROPHONE);
    assert_eq!(p.devices.audio, AudioAccess::Full);
    let mut p = example.clone();
    p.set_permissions(P::CONTROLLERS | P::ALL_INPUT | P::REMOVABLE | P::REMOVABLE_WRITE);
    assert_eq!(p.devices.controllers, ControllerAccess::AllInput);
    assert_eq!(p.removable, RemovableAccess::ReadWrite);
    assert_eq!(p.network, NetworkAccess::None);
    assert_eq!(p.devices.audio, AudioAccess::None);

    // The protections are permissions seen from the other side.
    let mut p = example.clone();
    p.set_permissions(P::MACHINE_ID | P::UNFILTERED_SYSCALLS);
    assert!(p.hardening.mask_hostname && p.hardening.mask_hardware_ids);
    assert!(!p.hardening.mask_machine_id);
    assert_eq!(p.hardening.seccomp, SeccompMode::Off);
    // What is more than yes or no stays as it was.
    assert_eq!(p.folders, example.folders);
    assert_eq!(p.game_dir, example.game_dir);
    assert_eq!(p.preset, example.preset);
}

#[test]
fn policy_validation_table() {
    let cases: Vec<(&str, Box<dyn Fn(&mut SandboxPolicy)>)> = vec![
        ("policy.schema_version", Box::new(|p| p.schema_version = 2)),
        ("policy.schema_version", Box::new(|p| p.schema_version = 0)),
        (
            "policy.folders.drive_letter",
            Box::new(|p| p.folders[0].drive_letter = Some('c')),
        ),
        (
            "policy.folders.drive_letter",
            Box::new(|p| p.folders[0].drive_letter = Some('z')),
        ),
        (
            "policy.folders.drive_letter",
            Box::new(|p| {
                let g = p.folders[0].clone();
                p.folders.push(g);
            }),
        ),
        (
            "policy.folders.path",
            Box::new(|p| p.folders[0].path = "Music".into()),
        ),
        (
            "policy.save_data.dir",
            Box::new(|p| p.save_data = SaveDataLocation::External { dir: "rel".into() }),
        ),
        (
            "policy.exceptions.reason",
            Box::new(|p| {
                p.exceptions.push(PolicyException::PassEnv {
                    name: "X".into(),
                    reason: " ".into(),
                })
            }),
        ),
        (
            "policy.exceptions.name",
            Box::new(|p| {
                p.exceptions.push(PolicyException::PassEnv {
                    name: "BAD-NAME".into(),
                    reason: "r".into(),
                })
            }),
        ),
        (
            "policy.exceptions.path",
            Box::new(|p| {
                p.exceptions.push(PolicyException::ExtraDevice {
                    path: "/etc/passwd".into(),
                    reason: "r".into(),
                })
            }),
        ),
        (
            "policy.display.isolation",
            Box::new(|p| p.display.isolation = DisplayIsolation::Gamescope),
        ),
    ];
    for (field, mutate) in cases {
        let mut p = example_policy();
        mutate(&mut p);
        assert_eq!(p.validate().unwrap_err().field, field);
    }
    let err: Error = {
        let mut p = example_policy();
        p.schema_version = 9;
        p.validate().unwrap_err().into()
    };
    assert_eq!(err.code, ErrorCode::SbxPolicyUnreadable);
}

#[test]
fn preset_forms() {
    for p in [
        Preset::Offline,
        Preset::Online,
        Preset::Standard,
        Preset::Custom {
            base: BasePreset::Standard,
        },
    ] {
        rt(&p);
        assert_eq!(Preset::from_column(&p.as_column()), Some(p));
    }
    assert_eq!(serde_json::to_value(Preset::Offline).unwrap(), "offline");
    assert_eq!(
        Preset::Custom {
            base: BasePreset::Online
        }
        .base(),
        BasePreset::Online
    );
    assert_eq!(BasePreset::parse("ONLINE"), Some(BasePreset::Online));
    assert_eq!(BasePreset::parse("nope"), None);
}

#[test]
fn game_round_trip_and_validation() {
    let g = game();
    rt(&g);
    g.validate().unwrap();
    assert_eq!(g.primary().unwrap().role, ExeRole::Primary);
    let v = serde_json::to_value(&g).unwrap();
    assert_eq!(v["flags"], 6);
    assert_eq!(v["store"], "gog");
    assert_eq!(v["runtime"], serde_json::json!({"channel": "ge_proton"}));
    assert_eq!(
        v["executables"][0]["fingerprint"]["quick_hash"],
        "ab".repeat(32)
    );
    assert_eq!(v["executables"][0]["pe"]["machine"], "x86_64");

    let mut bad = g.clone();
    bad.name = " ".into();
    assert_eq!(bad.validate().unwrap_err().problem, Problem::Empty);
    let mut bad = g.clone();
    bad.name = "x".repeat(81);
    assert!(matches!(
        bad.validate().unwrap_err().problem,
        Problem::TooLong { max: 80 }
    ));
    let mut bad = g.clone();
    bad.notes = "x".repeat(4001);
    assert_eq!(bad.validate().unwrap_err().field, "game.notes");
    let mut bad = g.clone();
    bad.umu_id = Some("umu 1".into());
    assert_eq!(bad.validate().unwrap_err().field, "game.umu_id");
    let mut bad = g.clone();
    bad.executables.push(executable(ExeRole::Primary));
    assert_eq!(
        bad.validate().unwrap_err().problem,
        Problem::ExactlyOnePrimary { found: 2 }
    );
    let mut bad = g.clone();
    bad.executables[0].role = ExeRole::Alternate;
    assert_eq!(
        bad.validate().unwrap_err().problem,
        Problem::ExactlyOnePrimary { found: 0 }
    );
    let mut bad = g.clone();
    bad.executables = (0..33).map(|_| executable(ExeRole::Tool)).collect();
    assert!(matches!(
        bad.validate().unwrap_err().problem,
        Problem::TooMany { max: 32 }
    ));
    let mut bad = g;
    bad.executables[0].rel_path = Some("../escape.exe".into());
    assert_eq!(bad.validate().unwrap_err().field, "executable.rel_path");
}

#[test]
fn game_flags_preserve_unknown_bits() {
    let f: GameFlags = serde_json::from_str("1025").unwrap();
    assert!(f.contains(GameFlags::SUSPECTED_TAMPER));
    assert_eq!(serde_json::to_string(&f).unwrap(), "1025");
}

#[test]
fn store_forms() {
    for s in [Store::Egs, Store::Zoom, Store::Other("custom".into())] {
        rt(&s);
        assert_eq!(Store::parse(s.as_str()), s);
    }
    assert_eq!(Store::Zoom.as_str(), "zoomplatform");
}

#[test]
fn sort_names() {
    assert_eq!(sort_name_of("The Witcher 3"), "witcher 3");
    assert_eq!(sort_name_of("A Short Hike"), "short hike");
    assert_eq!(sort_name_of("The"), "the");
    assert_eq!(sort_name_of("Hades"), "hades");
}

#[test]
fn executable_and_hex() {
    rt(&executable(ExeRole::Installer));
    rt(&PeInfo::default());
    let mut f = fingerprint();
    f.full_hash = None;
    rt(&f);
    assert_eq!(
        serde_json::to_value(f).unwrap()["full_hash"],
        serde_json::Value::Null
    );
    let h = [0x0f; 32];
    assert_eq!(from_hex32(&to_hex(&h)), Some(h));
    assert_eq!(from_hex32("zz"), None);
    assert!(
        serde_json::from_str::<Fingerprint>(
            r#"{"size":1,"mtime_ns":1,"quick_hash":"00","full_hash":null}"#
        )
        .is_err()
    );
    for r in [
        ExeRole::Primary,
        ExeRole::Alternate,
        ExeRole::Launcher,
        ExeRole::Tool,
        ExeRole::Installer,
    ] {
        assert_eq!(ExeRole::parse(r.as_str()), Some(r));
        assert_eq!(serde_json::to_value(r).unwrap(), r.as_str());
    }
}

#[test]
fn runtime_types() {
    for r in [
        RuntimeRef::Default,
        RuntimeRef::Channel(Channel::UmuProton),
        RuntimeRef::Pinned(RuntimeName::new("GE-Proton10-25").unwrap()),
        RuntimeRef::External("/opt/proton".into()),
    ] {
        rt(&r);
    }
    assert_eq!(
        serde_json::to_value(RuntimeRef::Default).unwrap(),
        "default"
    );
    let build = RuntimeBuild {
        name: RuntimeName::new("UMU-Proton-10.0-4").unwrap(),
        family: RuntimeFamily::UmuProton,
        version_key: "0010.0000-0004".into(),
        origin: RuntimeOrigin::Store,
        origin_label: "EasyPlay".into(),
        path: "/data/runtimes/Steam/compatibilitytools.d/UMU-Proton-10.0-4".into(),
        required_slr: SlrVariant::Steamrt3,
        display_name: "UMU-Proton 10.0-4".into(),
        has_fixes: true,
        size_bytes: Some(1 << 30),
        discovered_at: ts(1),
        last_used_at: None,
    };
    rt(&build);
    let resolved = ResolvedRuntime {
        name: build.name.clone(),
        proton_path_host: build.path.clone(),
        proton_path_sandbox: build.path.clone(),
        origin: RuntimeOrigin::Store,
        slr_variant: SlrVariant::Steamrt3,
        slr_dir: "/data/runtimes/umu/steamrt3".into(),
        platform_dir: "/data/runtimes/umu/steamrt3/sniper_platform_3.0".into(),
        isolation: RuntimeIsolationMode::Isolated,
    };
    rt(&resolved);
    assert_eq!(SlrVariant::parse("steamrt4"), Some(SlrVariant::Steamrt4));
    assert_eq!(
        serde_json::to_value(SlrVariant::Steamrt3).unwrap(),
        "steamrt3"
    );
    assert_eq!(Channel::GeProton.umu_codename(), "GE-Proton");
}

#[test]
fn session_types() {
    let s = Session {
        id: "01930a12-0000-7000-8000-000000000001"
            .parse::<SessionId>()
            .unwrap(),
        game_id: Some(gid()),
        game_name: "Hollow Knight".into(),
        executable_id: Some(eid()),
        kind: SessionKind::Game,
        state: SessionState::Ended,
        started_at: ts(1_000),
        ended_at: Some(ts(61_000)),
        duration_ms: Some(60_000),
        active_ms: 59_000,
        last_heartbeat_at: Some(ts(60_000)),
        end_reason: Some(EndReason::FailedToStart {
            code: ErrorCode::SesSupervisorNotStarted,
        }),
        exit_status: Some(1),
        counted: true,
        supervisor: Some(SupervisorIdent {
            pid: 4190,
            start_ticks: 123_456,
            boot_id: "8a6b…".into(),
        }),
        sandbox_pid: Some(4195),
        pid_ns: Some(4_026_533_244),
        context: SessionContext {
            preset: Some(Preset::Offline),
            network: Some(NetworkAccess::None),
            runtime_name: Some(RuntimeName::new("GE-Proton10-25").unwrap()),
            slr_variant: Some(SlrVariant::Steamrt3),
            isolation: Some(RuntimeIsolationMode::Isolated),
        },
        tz_offset_min: -300,
    };
    rt(&s);
    assert!(!s.is_open());
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(
        v["end_reason"],
        serde_json::json!({"failed_to_start": {"code": "EP-SES-001"}})
    );
    for r in [
        EndReason::Exited { code: 3 },
        EndReason::Crashed { signal: 11 },
        EndReason::StoppedByUser,
        EndReason::Killed,
        EndReason::SystemShutdown,
        EndReason::LauncherTerminated,
        EndReason::Interrupted,
        EndReason::FailedToStart {
            code: ErrorCode::SbxSetupFailed,
        },
    ] {
        rt(&r);
        let detail = r.detail();
        assert_eq!(
            EndReason::from_columns(r.as_str(), detail.as_deref()),
            Some(r)
        );
    }
    assert_eq!(
        serde_json::to_value(EndReason::StoppedByUser).unwrap(),
        "stopped_by_user"
    );
    for k in [
        SessionKind::Game,
        SessionKind::Installer,
        SessionKind::Tool,
        SessionKind::Prepare,
        SessionKind::Provision,
    ] {
        assert_eq!(SessionKind::parse(k.as_str()), Some(k));
    }
    assert!(SessionKind::Tool.is_untrusted());
    assert!(!SessionKind::Prepare.is_untrusted());
    {
        use SessionState::*;
        assert!(Starting.can_become(Running));
        assert!(Starting.can_become(Ended));
        assert!(Lingering.can_become(Running));
        assert!(!Ended.can_become(Running));
        assert!(!Stopping.can_become(Running));
        for st in [Starting, Running, Lingering, Stopping, Ended] {
            assert_eq!(SessionState::parse(st.as_str()), Some(st));
        }
    }
}

#[test]
fn stats_and_tags_and_artwork() {
    let mut st = GameStats::empty(gid());
    st.total_ms = 10;
    st.first_played_at = Some(ts(1));
    rt(&st);
    for k in [GameKey::Game(gid()), GameKey::Removed("Old Game".into())] {
        assert_eq!(GameKey::from_column(&k.as_column()), Some(k.clone()));
        rt(&k);
    }
    rt(&PlaytimeDay {
        game_key: GameKey::Game(gid()),
        day: 20261112,
        ms: 3_600_000,
        sessions: 2,
    });
    let tag = Tag {
        id: TagId::new(3),
        name: TagName::new("Co-op").unwrap(),
        color: TagColor::new(7).unwrap(),
        created_at: ts(9),
    };
    rt(&tag);
    assert!(TagColor::new(8).is_err());
    assert!(serde_json::from_str::<TagColor>("8").is_err());
    let art = ArtworkRef {
        game_id: gid(),
        slot: ArtSlot::Cover,
        file: "cover-0123456789abcdef.png".into(),
        hash: [7; 32],
        width: 600,
        height: 900,
        source: ArtOrigin::ExecutableIcon,
        updated_at: ts(4),
    };
    rt(&art);
    assert_eq!(
        serde_json::to_value(ArtOrigin::ExecutableIcon).unwrap(),
        "executable_icon"
    );
}

#[test]
fn prompts() {
    let kinds = vec![
        PromptKind::ExeChanged {
            game: gid(),
            exe: eid(),
            old: fingerprint(),
            new: fingerprint(),
        },
        PromptKind::InsideKnownRoot {
            game: gid(),
            path: "/g/x.exe".into(),
        },
        PromptKind::CopyOfKnown {
            game: gid(),
            path: "/g/x.exe".into(),
        },
        PromptKind::GrantTargetChanged {
            game: gid(),
            grant: "/a".into(),
            old: "/a".into(),
            new: "/b".into(),
        },
        PromptKind::RuntimeUnavailable {
            game: gid(),
            reference: RuntimeRef::Default,
        },
        PromptKind::FixNeedsNetwork {
            game: gid(),
            prefix_tainted: true,
        },
        PromptKind::ToolNeedsNetwork {
            game: gid(),
            tool: Tool::Winetricks {
                verbs: vec!["vcrun2019".into()],
            },
            prefix_tainted: false,
        },
        PromptKind::InstallerFinished {
            game: gid(),
            programs: vec![ProgramCandidate {
                path: PathBuf::from("/p/drive_c/Game/game.exe"),
                label: "Game".into(),
                from_shortcut: true,
                size: 10,
            }],
        },
        PromptKind::ConfirmBroadGrant {
            game: None,
            path: "/home/sam".into(),
            reason: "home_directory".into(),
        },
        PromptKind::RemoveRunningGame { game: gid() },
    ];
    for kind in kinds {
        let req = PromptRequest {
            id: "01930a12-0000-7000-8000-0000000000ff"
                .parse::<PromptId>()
                .unwrap(),
            kind: kind.clone(),
        };
        rt(&req);
        let default = kind.default_answer();
        assert!(
            kind.accepts(&default),
            "{kind:?} rejects its default {default:?}"
        );
        rt(&default);
    }
    let installer = PromptKind::InstallerFinished {
        game: gid(),
        programs: vec![],
    };
    assert_eq!(installer.default_answer(), PromptAnswer::KeepAsInstaller);
    assert!(!installer.accepts(&PromptAnswer::PickProgram { path: "/x".into() }));
    let grant = PromptKind::GrantTargetChanged {
        game: gid(),
        grant: "/a".into(),
        old: "/a".into(),
        new: "/b".into(),
    };
    assert_eq!(grant.default_answer(), PromptAnswer::Cancel);
    assert!(!grant.accepts(&PromptAnswer::ItsAnUpdate));
    assert_eq!(grant.game(), Some(gid()));
}

#[test]
fn summary_support_types() {
    rt(&Activity::Running { since: ts(1) });
    rt(&Activity::Launching(LaunchStateTag::Provisioning));
    rt(&Availability::DriveDisconnected);
    rt(&ArtKey([1; 32]));
    rt(&SummaryFlags::default());
    rt(&EntryKind::Installer);
    rt(&SaveLocationKind::External);
}
