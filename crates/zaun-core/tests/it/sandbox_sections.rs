//! Section, preset, renderer, explain, invariant and serde tests of the sandbox planner
//! (SANDBOX-001, -002, -005 … -020), plus the field-tested points recorded in
//! TECHNICAL_RESEARCH F-220 … F-226.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use zaun_core::model::{
    AudioAccess, BasePreset, ControllerAccess, EnvVar, FolderAccess, GameDirAccess, NetworkAccess,
    Preset, RemovableAccess, RuntimeIsolationMode, RuntimeOrigin, SandboxPolicy, SaveDataLocation,
    SeccompMode, SessionKind, SocketAccess, WorkingDir,
};
use zaun_core::sandbox::explain::diff as explain_diff;
use zaun_core::sandbox::plan::{
    EnvEntry, EnvOrigin, FdContent, HostAction, MountKind, MountMode, PlanBuilder, PolicyField,
    Section,
};
use zaun_core::sandbox::sections::etc_files::{
    EtcItem, is_allowed_destination, stand_in_machine_id,
};
use zaun_core::sandbox::validate::IssueCode;
use zaun_core::sandbox::{
    SandboxPlan, Widening, apply_preset, bwrap, classify, explain, offline, online, plan,
    plan_invariants, render, standard, widening,
};
use zaun_testkit::fixtures::hosts::{self, SandboxWorld};
use zaun_testkit::fixtures::policies::grant;

fn ok(w: &SandboxWorld, p: &SandboxPolicy) -> SandboxPlan {
    plan(p, &w.ctx(), &w.host).unwrap_or_else(|e| panic!("{e:#?}"))
}

fn lines(p: &SandboxPlan) -> Vec<String> {
    bwrap::display_lines(&render(p).unwrap())
        .lines()
        .map(str::to_owned)
        .collect()
}

fn index_of(p: &SandboxPlan, dst: &str) -> usize {
    p.mounts
        .iter()
        .position(|m| m.kind.dst() == Path::new(dst))
        .unwrap_or_else(|| panic!("no mount at {dst}"))
}

// ---- presets (SANDBOX-001) ------------------------------------------------------------

#[test]
fn presets_classify_as_themselves() {
    let host = hosts::arch_mesa();
    assert_eq!(classify(&offline()), Preset::Offline);
    assert_eq!(classify(&online()), Preset::Online);
    assert_eq!(classify(&standard(&host)), Preset::Standard);
    assert_eq!(
        classify(&standard(&hosts::input_group_user())),
        Preset::Standard
    );
    assert_eq!(
        standard(&hosts::input_group_user()).devices.controllers,
        ControllerAccess::Detected
    );
    assert_eq!(
        standard(&host).devices.controllers,
        ControllerAccess::AllInput
    );
}

type Change = Box<dyn Fn(&mut SandboxPolicy)>;

#[test]
fn any_single_field_change_is_custom() {
    let host = hosts::arch_mesa();
    for base in BasePreset::ALL {
        let p = zaun_core::sandbox::presets::preset_policy(base, &host);
        let changes: Vec<Change> = vec![
            Box::new(|p| {
                p.network = if p.network == NetworkAccess::None {
                    NetworkAccess::Full
                } else {
                    NetworkAccess::None
                }
            }),
            Box::new(|p| p.game_dir = GameDirAccess::ReadOnly),
            Box::new(|p| {
                p.folders
                    .push(grant("/home/sam/Music", FolderAccess::ReadOnly, None))
            }),
            Box::new(|p| {
                p.save_data = SaveDataLocation::External {
                    dir: "/mnt/saves/hk".into(),
                }
            }),
            Box::new(|p| p.removable = RemovableAccess::ReadOnly),
            Box::new(|p| p.devices.audio = AudioAccess::None),
            Box::new(|p| p.devices.alsa_devices = true),
            Box::new(|p| p.devices.controllers = ControllerAccess::None),
            Box::new(|p| p.devices.camera = true),
            Box::new(|p| p.display.wayland = SocketAccess::Deny),
            Box::new(|p| p.display.x11 = SocketAccess::Deny),
            Box::new(|p| p.hardening.seccomp = SeccompMode::Off),
            Box::new(|p| p.hardening.mask_hostname = false),
            Box::new(|p| p.hardening.mask_machine_id = !p.hardening.mask_machine_id),
            Box::new(|p| p.hardening.mask_hardware_ids = !p.hardening.mask_hardware_ids),
            Box::new(|p| {
                p.hardening.runtime_isolation = zaun_core::model::RuntimeIsolation::Shared
            }),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut q = p.clone();
            change(&mut q);
            // Offline and Online differ only in `network`: that change lands on the other preset.
            let expected = if i == 0 && base != BasePreset::Standard {
                Preset::from(if base == BasePreset::Offline {
                    BasePreset::Online
                } else {
                    BasePreset::Offline
                })
            } else {
                Preset::Custom { base }
            };
            assert_eq!(classify(&q), expected, "{base:?} change {i}");
        }
        let mut loose = p.clone();
        loose.game_dir = GameDirAccess::FileOnly;
        assert_eq!(
            classify(&loose),
            Preset::from(base),
            "FileOnly is preset-neutral"
        );
        // No preset decides the microphone (ADR-0026); switching presets keeps it.
        let mut mic = p.clone();
        mic.devices.audio = AudioAccess::Full;
        assert_eq!(classify(&mic), Preset::from(base), "the microphone");
        for other in BasePreset::ALL {
            assert_eq!(
                apply_preset(&mic, other, &host).devices.audio,
                AudioAccess::Full
            );
        }
    }
}

#[test]
fn apply_preset_keeps_game_dir_grants_and_save() {
    let host = hosts::arch_mesa();
    let mut p = offline();
    p.game_dir = GameDirAccess::Protected;
    p.folders
        .push(grant("/home/sam/Music", FolderAccess::ReadOnly, Some('m')));
    p.save_data = SaveDataLocation::External {
        dir: "/mnt/saves/hk".into(),
    };
    let q = apply_preset(&p, BasePreset::Online, &host);
    assert_eq!(q.network, NetworkAccess::Full);
    assert_eq!(
        (q.game_dir, q.folders.clone(), q.save_data.clone()),
        (p.game_dir, p.folders.clone(), p.save_data.clone())
    );
    assert_eq!(
        q.preset,
        Preset::Custom {
            base: BasePreset::Online
        }
    );
    assert_eq!(
        apply_preset(&offline(), BasePreset::Standard, &host).preset,
        Preset::Standard
    );
}

#[test]
fn widening_detection() {
    let host = hosts::arch_mesa();
    let old = offline();
    let mut new = standard(&host);
    new.folders
        .push(grant("/home/sam/Music", FolderAccess::ReadWrite, None));
    new.hardening.seccomp = SeccompMode::Off;
    new.devices.camera = true;
    new.devices.audio = AudioAccess::Full;
    let w = widening(&old, &new);
    for expected in [
        Widening::Microphone,
        Widening::Network,
        Widening::NewGrant("/home/sam/Music".into()),
        Widening::Controllers,
        Widening::Camera,
        Widening::SeccompOff,
        Widening::MaskingOff,
    ] {
        assert!(w.contains(&expected), "{expected:?} in {w:?}");
    }
    assert!(widening(&new, &old).is_empty());
    let mut ro = offline();
    ro.folders
        .push(grant("/home/sam/Music", FolderAccess::ReadOnly, None));
    let mut rw = ro.clone();
    rw.folders[0].access = FolderAccess::ReadWrite;
    assert_eq!(
        widening(&ro, &rw),
        vec![Widening::GrantWritable("/home/sam/Music".into())]
    );
}

// ---- plan types and builder (SANDBOX-002) ----------------------------------------------

#[test]
fn plan_serialises_losslessly() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    let json = serde_json::to_string(&p).unwrap();
    let back: SandboxPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

#[test]
fn non_utf8_paths_round_trip() {
    use std::os::unix::ffi::OsStrExt;
    use zaun_core::sandbox::plan::codec;
    for raw in [
        &b"/mnt/\xff\xfe game"[..],
        b"b:literal",
        b"/back\\slash",
        b"/plain/path",
    ] {
        let s = std::ffi::OsStr::from_bytes(raw);
        assert_eq!(codec::decode(&codec::encode(s)).unwrap(), s);
    }
}

#[test]
fn builder_rejects_out_of_order_sections() {
    let mut b = PlanBuilder::new();
    b.begin(Section::System).unwrap();
    b.begin(Section::Home).unwrap();
    assert!(b.begin(Section::Etc).is_err());
    assert!(b.begin(Section::Home).is_err());
    assert!(b.misuse().is_some());
}

#[test]
fn every_mount_carries_its_section_in_order() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    let sections: Vec<Section> = p.mounts.iter().map(|m| m.origin.section).collect();
    let mut sorted = sections.clone();
    sorted.sort();
    assert_eq!(sections, sorted);
}

// ---- namespaces (SANDBOX-005) -----------------------------------------------------------

#[test]
fn unshare_net_iff_network_none_across_kinds() {
    let cases = [
        (SessionKind::Game, None, NetworkAccess::None, true),
        (SessionKind::Game, None, NetworkAccess::Full, false),
        (
            SessionKind::Tool,
            Some(NetworkAccess::Full),
            NetworkAccess::None,
            false,
        ),
        (SessionKind::Prepare, None, NetworkAccess::None, true),
        (
            SessionKind::Prepare,
            Some(NetworkAccess::Full),
            NetworkAccess::None,
            false,
        ),
        (SessionKind::Provision, None, NetworkAccess::None, false),
    ];
    for (kind, ov, net, unshare) in cases {
        let mut w = SandboxWorld::worked_example();
        w.set_kind(kind, ov);
        let mut p = offline();
        p.network = net;
        let pl = ok(&w, &p);
        assert_eq!(pl.namespaces.net, unshare, "{kind:?} {ov:?} {net:?}");
        assert!(
            pl.namespaces.user
                && pl.namespaces.pid
                && pl.namespaces.uts
                && pl.namespaces.die_with_parent
                && pl.namespaces.new_session
        );
    }
}

#[test]
fn seccomp_follows_policy_and_availability() {
    let mut w = SandboxWorld::worked_example();
    let mut p = offline();
    p.hardening.seccomp = SeccompMode::Off;
    let pl = ok(&w, &p);
    assert!(pl.namespaces.seccomp_fd.is_none());
    assert!(
        !pl.fds
            .iter()
            .any(|f| f.content == FdContent::SeccompProgram)
    );
    w.seccomp_available = false;
    let pl = ok(&w, &offline());
    assert!(pl.namespaces.seccomp_fd.is_none());
    assert!(pl.notes.iter().any(|n| n.key == "note.seccomp.unavailable"));
}

// ---- field-tested points (TECHNICAL_RESEARCH F-220 … F-226) ----------------------------

#[test]
fn ipc_is_shared_exactly_when_x11_is_bound() {
    let w = SandboxWorld::worked_example();
    assert!(
        !ok(&w, &offline()).namespaces.ipc,
        "Xwayland bound → MIT-SHM needs the host IPC namespace"
    );
    let mut p = offline();
    p.display.x11 = SocketAccess::Deny;
    let pl = ok(&w, &p);
    assert!(pl.namespaces.ipc);
    assert!(lines(&pl).contains(&"--unshare-ipc".to_owned()));
    let mut w2 = SandboxWorld::worked_example();
    if let Some(x) = &mut w2.host.display.x11 {
        x.filesystem_socket = false;
    }
    assert!(
        ok(&w2, &offline()).namespaces.ipc,
        "abstract-only X socket is not bound"
    );
}

#[test]
fn x11_socket_is_bound_after_tmp_tmpfs_and_is_the_filesystem_socket_offline() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    assert!(p.namespaces.net);
    let tmp = index_of(&p, "/tmp");
    let x = index_of(&p, "/tmp/.X11-unix/X0");
    assert!(x > tmp);
    assert!(
        matches!(&p.mounts[x].kind, MountKind::Bind { src, .. } if src == Path::new("/tmp/.X11-unix/X0"))
    );
}

#[test]
fn identity_files_are_synthesized_read_only() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    for f in ["/etc/passwd", "/etc/group", "/etc/nsswitch.conf"] {
        let m = p.mount_at(Path::new(f)).unwrap();
        assert!(
            matches!(m.kind, MountKind::FileData { perms: 0o444, .. }),
            "{f}"
        );
    }
    let fd = p
        .fds
        .iter()
        .find(|f| f.target.as_deref() == Some(Path::new("/etc/passwd")))
        .unwrap();
    assert_eq!(
        fd.content,
        FdContent::Bytes(b"sam:x:1000:1000:sam:/home/sam:/bin/sh\nnobody:x:65534:65534:nobody:/:/usr/sbin/nologin\n".to_vec())
    );
}

#[test]
fn nixos_etc_links_through_etc_static_are_bound_by_resolved_target() {
    let mut w = SandboxWorld::new(hosts::nixos());
    w.host.etc_links.links.get_mut("zoneinfo").unwrap();
    let p = ok(&w, &offline());
    let z = p.mount_at(Path::new("/etc/zoneinfo")).unwrap();
    assert!(
        matches!(&z.kind, MountKind::Bind { src, mode: MountMode::Ro, .. } if src == Path::new("/nix/store/t1-tzdata/share/zoneinfo"))
    );
    let l = p.mount_at(Path::new("/etc/localtime")).unwrap();
    assert!(matches!(&l.kind, MountKind::Bind { src, .. } if src.starts_with("/nix/store")));
    // On a merged-/usr host, a link into /usr is recreated.
    let a = ok(&SandboxWorld::worked_example(), &offline());
    assert!(matches!(
        &a.mount_at(Path::new("/etc/localtime")).unwrap().kind,
        MountKind::Symlink { .. }
    ));
}

#[test]
fn pressure_vessel_share_home_and_gst_debug_are_fixed() {
    let w = SandboxWorld::worked_example();
    for kind in [
        SessionKind::Game,
        SessionKind::Prepare,
        SessionKind::Provision,
    ] {
        let mut w = SandboxWorld::worked_example();
        w.set_kind(kind, None);
        let p = ok(&w, &offline());
        assert_eq!(p.env.get("PRESSURE_VESSEL_SHARE_HOME"), Some("1"));
        assert_eq!(p.env.get("GST_DEBUG"), Some("0"));
    }
    let _ = w;
}

#[test]
fn offline_sessions_disable_runtime_update() {
    let w = SandboxWorld::worked_example();
    assert_eq!(ok(&w, &offline()).env.get("UMU_RUNTIME_UPDATE"), Some("0"));
    let mut p = SandboxWorld::worked_example();
    p.set_kind(SessionKind::Provision, None);
    assert_eq!(ok(&p, &offline()).env.get("UMU_RUNTIME_UPDATE"), None);
}

#[test]
fn tuning_variables_come_from_settings_not_the_host() {
    let mut w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    assert_eq!(
        p.env.get("DXVK_CONFIG_FILE"),
        None,
        "host value is not forwarded"
    );
    assert_eq!(p.env.get("PROTON_USE_WINED3D"), None);
    w.launch.env = vec![
        EnvVar {
            name: "DXVK_CONFIG_FILE".into(),
            value: "/mnt/games/Hollow Knight/dxvk.conf".into(),
        },
        EnvVar {
            name: "VKD3D_CONFIG".into(),
            value: "dxr".into(),
        },
    ];
    w.launch.compat.renderer = zaun_core::model::Renderer::OpenGl;
    w.launch.compat.dll_overrides = "d3d9=n,b".into();
    let p = ok(&w, &offline());
    assert_eq!(
        p.env.get("DXVK_CONFIG_FILE"),
        Some("/mnt/games/Hollow Knight/dxvk.conf")
    );
    assert_eq!(p.env.get("VKD3D_CONFIG"), Some("dxr"));
    assert_eq!(p.env.get("PROTON_USE_WINED3D"), Some("1"));
    assert_eq!(p.env.get("WINEDLLOVERRIDES"), Some("d3d9=n,b"));
}

#[test]
fn umu_cache_is_writable_only_in_trusted_network_sessions() {
    let mut w = SandboxWorld::worked_example();
    w.set_kind(SessionKind::Prepare, Some(NetworkAccess::Full));
    let p = ok(&w, &offline());
    let c = p.mount_at(Path::new("/home/sam/.cache/umu")).unwrap();
    assert!(
        matches!(&c.kind, MountKind::Bind { mode: MountMode::Rw, src, .. } if *src == w.paths.umu_cache_dir)
    );
    assert_eq!(
        p.mount_at(Path::new("/tmp")).map(|m| m.kind.clone()),
        Some(MountKind::Tmpfs {
            dst: "/tmp".into(),
            size: Some(64 << 20),
            perms: Some(0o1777)
        })
    );
    let g = ok(&SandboxWorld::worked_example(), &offline());
    assert!(g.mount_at(Path::new("/home/sam/.cache/umu")).is_none());
}

// ---- system, /etc, devices, sysfs, sockets ---------------------------------------------

#[test]
fn system_layouts() {
    let deb = ok(
        &SandboxWorld::new(hosts::debian_stable_old_bwrap()),
        &offline(),
    );
    assert!(lines(&deb).contains(&"--ro-bind /bin /bin".to_owned()));
    assert!(lines(&deb).contains(&"--ro-bind /srv/gl-libs /srv/gl-libs".to_owned()));
    let nix = ok(&SandboxWorld::new(hosts::nixos()), &offline());
    for l in [
        "--ro-bind /nix /nix",
        "--ro-bind /run/current-system /run/current-system",
        "--ro-bind-try /etc/profiles/per-user/sam /etc/profiles/per-user/sam",
    ] {
        assert!(lines(&nix).contains(&l.to_owned()), "{l}");
    }
    // Closure under /nix is covered by the system bind.
    assert!(
        nix.mount_at(Path::new("/nix/store/abc-umu-launcher-1.4.4"))
            .is_none()
    );
    let mut w = SandboxWorld::worked_example();
    w.umu.closure = vec!["/home/sam/.local/share/pipx/venvs/umu-launcher".into()];
    let p = ok(&w, &offline());
    let i = index_of(&p, "/home/sam/.local/share/pipx/venvs/umu-launcher");
    assert!(i > index_of(&p, "/home/sam"));
}

#[test]
fn etc_destinations_are_closed_allow_list() {
    for (_, host) in hosts::all() {
        for net in [NetworkAccess::None, NetworkAccess::Full] {
            let mut w = SandboxWorld::new(host.clone());
            w.host
                .layout
                .etc
                .insert("shadow".into(), zaun_core::host::HostEntry::File);
            w.host
                .layout
                .etc
                .insert("sudoers".into(), zaun_core::host::HostEntry::File);
            let mut p = offline();
            p.network = net;
            let pl = ok(&w, &p);
            for m in pl.mounts_in(Section::Etc) {
                assert!(is_allowed_destination(&m.1.kind), "{:?}", m.1.kind);
            }
        }
    }
    assert!(EtcItem::all_names().contains(&"zoneinfo"));
}

#[test]
fn stand_in_machine_id_is_deterministic_and_per_game() {
    let a: zaun_core::ids::GameId = hosts::GAME_ID.parse().unwrap();
    let b: zaun_core::ids::GameId = "0192f0c2-1c3a-7d4e-8f00-000000000002".parse().unwrap();
    let x = stand_in_machine_id(Some(hosts::MACHINE_ID), a);
    assert_eq!(x, stand_in_machine_id(Some(hosts::MACHINE_ID), a));
    assert_ne!(x, stand_in_machine_id(Some(hosts::MACHINE_ID), b));
    assert_eq!(x.len(), 32);
    assert_ne!(x, hosts::MACHINE_ID);
}

#[test]
fn keyboards_readable_never_binds_dev_input() {
    let w = SandboxWorld::new(hosts::input_group_user());
    for c in [ControllerAccess::Detected, ControllerAccess::AllInput] {
        let mut p = offline();
        p.devices.controllers = c;
        let pl = ok(&w, &p);
        assert!(pl.mount_at(Path::new("/dev/input")).is_none());
        assert!(pl.mount_at(Path::new("/dev/input/event3")).is_none());
        assert!(pl.mount_at(Path::new("/dev/hidraw0")).is_none());
    }
    let mut p = offline();
    p.devices.controllers = ControllerAccess::None;
    let pl = ok(&w, &p);
    assert!(pl.mounts.iter().all(
        |m| !m.kind.dst().starts_with("/dev/input") && !m.kind.dst().starts_with("/dev/hidraw")
    ));
    assert!(pl.mount_at(Path::new("/run/udev")).is_none());
}

#[test]
fn never_devices_are_never_emitted() {
    for (_, host) in hosts::all() {
        let w = SandboxWorld::new(host);
        let mut p = standard(&w.host);
        p.devices.alsa_devices = true;
        p.devices.camera = true;
        let pl = ok(&w, &p);
        for d in [
            "/dev/uinput",
            "/dev/kvm",
            "/dev/fuse",
            "/dev/mem",
            "/dev/kmsg",
            "/dev/console",
            "/dev/shm",
            "/dev/mqueue",
        ] {
            assert!(pl.mount_at(Path::new(d)).is_none(), "{d}");
        }
    }
}

#[test]
fn sysfs_masks_follow_their_binds() {
    let w = SandboxWorld::new(hosts::fedora_nvidia());
    let p = ok(&w, &offline());
    let last_bind = index_of(&p, "/sys/devices");
    for d in [
        "/sys/class/net",
        "/sys/devices/virtual/net",
        "/sys/devices/virtual/dmi",
        "/sys/class/dmi",
        "/sys/devices/pci0000:00/0000:00:1f.6/net",
    ] {
        assert!(index_of(&p, d) > last_bind, "{d}");
    }
    let mut on = online();
    on.hardening.mask_hardware_ids = false;
    let pl = ok(&w, &on);
    assert!(pl.mount_at(Path::new("/sys/class/net")).is_none());
}

#[test]
fn only_documented_runtime_dir_entries() {
    for (_, host) in hosts::all() {
        let w = SandboxWorld::new(host);
        let pl = ok(&w, &standard(&w.host));
        for m in &pl.mounts {
            let d = m.kind.dst();
            if d.starts_with("/run/user/1000") && d != Path::new("/run/user/1000") {
                let name = d
                    .strip_prefix("/run/user/1000")
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                assert!(
                    [
                        "wayland-0",
                        "pulse",
                        "pulse/native",
                        "pipewire-0",
                        "zaun-xauth"
                    ]
                    .contains(&name.as_str()),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn no_display_fails_with_sbx_020() {
    let mut w = SandboxWorld::new(hosts::x11_only());
    let mut p = offline();
    p.display.x11 = SocketAccess::Deny;
    let e = plan(&p, &w.ctx(), &w.host).unwrap_err();
    assert!(e.iter().any(|i| i.code == IssueCode::NoDisplay));
    w.host.display.x11 = None;
    assert!(plan(&offline(), &w.ctx(), &w.host).is_err());
}

#[test]
fn proton_log_directory() {
    let mut w = SandboxWorld::worked_example();
    w.launch.compat.proton_log = true;
    let p = ok(&w, &offline());
    let m = p.mount_at(Path::new("/run/zaun/proton-log")).unwrap();
    assert!(
        matches!(&m.kind, MountKind::Bind { mode: MountMode::Rw, src, .. } if *src == w.session.log_dir.join("proton"))
    );
    assert_eq!(p.env.get("PROTON_LOG_DIR"), Some("/run/zaun/proton-log"));
    assert!(p.host_actions.contains(&HostAction::CreateDir {
        path: w.session.log_dir.join("proton"),
        mode: 0o700
    }));
}

// ---- home, runtime, prefix, game, grants ------------------------------------------------

#[test]
fn home_by_kind_and_conveniences() {
    let mut w = SandboxWorld::worked_example();
    w.launch.compat.mangohud = true;
    let p = ok(&w, &offline());
    assert!(matches!(
        &p.mount_at(Path::new("/home/sam")).unwrap().kind,
        MountKind::Bind {
            mode: MountMode::Rw,
            ..
        }
    ));
    assert!(
        p.mount_at(Path::new("/home/sam/.config/MangoHud"))
            .is_some()
    );
    assert!(
        p.mount_at(Path::new("/home/sam/.fonts")).is_none(),
        "absent font dir skipped"
    );
    assert!(
        p.mounts
            .iter()
            .all(|m| !m.kind.dst().starts_with("/home/sam/.config/protonfixes"))
    );
    w.set_kind(SessionKind::Provision, None);
    let p = ok(&w, &offline());
    assert!(matches!(
        &p.mount_at(Path::new("/home/sam")).unwrap().kind,
        MountKind::Tmpfs {
            perms: Some(0o700),
            ..
        }
    ));
}

#[test]
fn isolated_runtime_recreates_symlinks_and_never_binds_var_or_marker() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    let rt = Path::new("/home/sam/.local/share/umu/steamrt3");
    assert!(
        matches!(&p.mount_at(&rt.join("umu")).unwrap().kind, MountKind::Symlink { target, .. } if target == Path::new("_v2-entry-point"))
    );
    assert!(p.mount_at(&rt.join("var")).is_none());
    assert!(matches!(
        p.mount_at(&rt.join(".installed.ok")).unwrap().kind,
        MountKind::FileData { .. }
    ));
    assert_eq!(
        p.env.get("PRESSURE_VESSEL_VARIABLE_DIR"),
        Some("/run/zaun/pv-var")
    );
}

#[test]
fn shared_runtime_requires_audit_and_proton_stays_read_only() {
    let mut w = SandboxWorld::worked_example();
    w.runtime.isolation = RuntimeIsolationMode::Shared;
    let p = ok(&w, &offline());
    assert!(p.audit_after.is_some());
    let proton = p
        .mounts
        .iter()
        .find(|m| m.kind.src() == Some(w.runtime.proton_path_host.as_path()))
        .unwrap();
    assert!(!proton.kind.writes_host());
    assert!(p.mount_at(Path::new("/run/zaun/pv-var")).is_none());
}

#[test]
fn external_proton_is_bound_at_identical_path() {
    let mut w = SandboxWorld::worked_example();
    w.runtime.origin = RuntimeOrigin::External;
    w.runtime.proton_path_host = "/usr/share/steam/compatibilitytools.d/proton-ge".into();
    w.runtime.proton_path_sandbox = w.runtime.proton_path_host.clone();
    let p = ok(&w, &offline());
    assert_eq!(
        p.env.get("PROTONPATH"),
        Some("/usr/share/steam/compatibilitytools.d/proton-ge")
    );
    assert!(lines(&p).contains(&"--ro-bind /usr/share/steam/compatibilitytools.d/proton-ge /usr/share/steam/compatibilitytools.d/proton-ge".to_owned()));
}

#[test]
fn trusted_session_sees_all_compat_tools_read_write() {
    let mut w = SandboxWorld::worked_example();
    w.set_kind(SessionKind::Provision, None);
    let p = ok(&w, &offline());
    let c = p
        .mount_at(Path::new(
            "/home/sam/.local/share/Steam/compatibilitytools.d",
        ))
        .unwrap();
    assert!(c.kind.writes_host());
    assert_eq!(p.env.get("PROTONPATH"), Some("GE-Proton"));
    assert_eq!(p.env.get("WINEPREFIX"), Some("/tmp/zaun-prefix"));
    assert!(
        p.mounts.iter().all(|m| m
            .kind
            .src()
            .is_none_or(|s| !s.starts_with(&w.paths.games_dir))),
        "Provision never mounts a game"
    );
}

#[test]
fn external_save_is_mounted_after_prefix() {
    let w = SandboxWorld::worked_example();
    let mut pol = offline();
    pol.save_data = SaveDataLocation::External {
        dir: "/mnt/saves/hk".into(),
    };
    let p = ok(&w, &pol);
    let prefix = w.game_paths.prefix.display().to_string();
    let save = format!("{prefix}/drive_c/users/steamuser");
    assert!(index_of(&p, &save) > index_of(&p, &prefix));
    assert!(p.host_actions.contains(&HostAction::CreateDir {
        path: save.into(),
        mode: 0o700
    }));
}

#[test]
fn game_folder_modes_and_kinds() {
    let w = SandboxWorld::worked_example();
    let root = "/mnt/games/Hollow Knight";
    for (mode, check) in [
        (GameDirAccess::ReadOnly, "ro"),
        (GameDirAccess::ReadWrite, "rw"),
        (GameDirAccess::Protected, "overlay"),
    ] {
        let mut pol = offline();
        pol.game_dir = mode;
        let p = ok(&w, &pol);
        let m = &p.mount_at(Path::new(root)).unwrap().kind;
        match check {
            "ro" => assert!(matches!(
                m,
                MountKind::Bind {
                    mode: MountMode::Ro,
                    ..
                }
            )),
            "rw" => assert!(matches!(
                m,
                MountKind::Bind {
                    mode: MountMode::Rw,
                    ..
                }
            )),
            _ => assert!(matches!(m, MountKind::OverlayRw { .. })),
        }
    }
    let mut prep = SandboxWorld::worked_example();
    prep.set_kind(SessionKind::Prepare, None);
    let p = ok(&prep, &offline());
    assert!(p.mount_at(Path::new(root)).is_none());
    assert_eq!(p.chdir, PathBuf::from("/home/sam"));
}

#[test]
fn loose_file_binds_only_the_file_and_works_in_home() {
    let mut w = SandboxWorld::worked_example();
    w.loose_exe();
    let mut pol = offline();
    pol.game_dir = GameDirAccess::FileOnly;
    let p = ok(&w, &pol);
    assert!(matches!(
        &p.mount_at(Path::new("/home/sam/Downloads/setup.exe"))
            .unwrap()
            .kind,
        MountKind::Bind {
            mode: MountMode::Ro,
            ..
        }
    ));
    assert_eq!(p.chdir, PathBuf::from("/home/sam"));
}

#[test]
fn nested_grants_parent_before_child_and_drive_letters() {
    let w = SandboxWorld::worked_example();
    let mut pol = offline();
    pol.folders.push(grant(
        "/mnt/data/shared/inner",
        FolderAccess::ReadWrite,
        Some('e'),
    ));
    pol.folders
        .push(grant("/mnt/data/shared", FolderAccess::ReadOnly, Some('d')));
    pol.removable = RemovableAccess::ReadOnly;
    let p = ok(&w, &pol);
    assert!(index_of(&p, "/mnt/data/shared") < index_of(&p, "/mnt/data/shared/inner"));
    assert!(index_of(&p, "/run/media/sam") > index_of(&p, "/mnt/games/Hollow Knight"));
    let dos = p
        .host_actions
        .iter()
        .find_map(|a| match a {
            HostAction::EnsureDosDevices { links, .. } => Some(links.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        dos.iter().map(|l| l.letter).collect::<Vec<_>>(),
        vec!['d', 'e']
    );
}

#[test]
fn working_directory_choices() {
    let mut w = SandboxWorld::worked_example();
    w.launch.working_dir = WorkingDir::GameRoot;
    assert_eq!(
        ok(&w, &offline()).chdir,
        PathBuf::from("/mnt/games/Hollow Knight")
    );
    w.launch.working_dir = WorkingDir::Custom("/mnt/games/Hollow Knight/hollow_knight_Data".into());
    assert_eq!(
        ok(&w, &offline()).chdir,
        PathBuf::from("/mnt/games/Hollow Knight/hollow_knight_Data")
    );
    w.launch.working_dir = WorkingDir::Custom("/etc".into());
    let e = plan(&offline(), &w.ctx(), &w.host).unwrap_err();
    assert!(e.iter().any(|i| i.code == IssueCode::WorkingDirOutside));
}

// ---- invariants (SANDBOX-017) -----------------------------------------------------------

fn broken(tamper: impl FnOnce(&mut SandboxPlan)) -> Vec<IssueCode> {
    let w = SandboxWorld::worked_example();
    match zaun_core::sandbox::planner::plan_with(&offline(), &w.ctx(), &w.host, tamper) {
        Ok(_) => Vec::new(),
        Err(i) => i.iter().map(|i| i.code).collect(),
    }
}

#[test]
fn deliberately_broken_sections_are_caught() {
    assert!(broken(|p| p.namespaces.pid = false).contains(&IssueCode::InvNamespaces));
    assert!(broken(|p| p.namespaces.net = false).contains(&IssueCode::InvNamespaces));
    assert!(broken(|p| p.namespaces.die_with_parent = false).contains(&IssueCode::InvLifecycle));
    assert!(broken(|p| p.env.clear = false).contains(&IssueCode::InvEnv));
    assert!(
        broken(|p| {
            p.env.set.insert(
                "UMU_ZENITY".into(),
                EnvEntry {
                    value: "1".into(),
                    origin: EnvOrigin::Fixed,
                },
            );
        })
        .contains(&IssueCode::InvEnv)
    );
    assert!(
        broken(|p| {
            p.env.set.insert(
                "HOME".into(),
                EnvEntry {
                    value: "/".into(),
                    origin: EnvOrigin::Game,
                },
            );
        })
        .contains(&IssueCode::InvEnv)
    );
    let ssh = |p: &mut SandboxPlan| {
        let mut m = p.mounts.last().unwrap().clone();
        m.kind = MountKind::Bind {
            src: "/home/sam/.ssh".into(),
            dst: "/home/sam/.ssh".into(),
            mode: MountMode::Ro,
            try_: false,
        };
        m.origin.section = Section::Grants;
        m.origin.policy_field = Some(PolicyField::Folder("/home/sam/.ssh".into()));
        p.mounts.push(m);
    };
    assert!(broken(ssh).contains(&IssueCode::InvDenied));
    assert!(
        broken(|p| {
            for m in &mut p.mounts {
                if let MountKind::Bind { src, mode, .. } = &mut m.kind
                    && src.ends_with("umu")
                    && src.to_string_lossy().contains("runtimes")
                {
                    *mode = MountMode::Rw;
                }
            }
        })
        .contains(&IssueCode::InvRuntimeWritable)
    );
    assert!(
        broken(|p| {
            let m = p.mounts.last().unwrap().clone();
            let mut m2 = m;
            m2.kind = MountKind::Bind {
                src:
                    "/home/sam/.local/share/zaun/games/0192f0c2-1c3a-7d4e-8f00-000000000099/prefix"
                        .into(),
                dst: "/x".into(),
                mode: MountMode::Rw,
                try_: false,
            };
            p.mounts.push(m2);
        })
        .contains(&IssueCode::InvOtherGame)
    );
    assert!(
        broken(|p| {
            for m in &mut p.mounts {
                if let MountKind::Bind { src, mode, .. } = &mut m.kind
                    && src.ends_with("GE-Proton10-25")
                {
                    *mode = MountMode::Rw;
                }
            }
        })
        .contains(&IssueCode::InvProtonWritable)
    );
    assert!(
        broken(|p| {
            let mut m = p.mounts.last().unwrap().clone();
            m.kind = MountKind::Bind {
                src: "/run/user/1000/bus".into(),
                dst: "/run/user/1000/bus".into(),
                mode: MountMode::Ro,
                try_: false,
            };
            p.mounts.push(m);
        })
        .contains(&IssueCode::InvSocket)
    );
    assert!(
        broken(|p| {
            let mut m = p.mounts.last().unwrap().clone();
            m.kind = MountKind::Tmpfs {
                dst: "/home".into(),
                size: None,
                perms: None,
            };
            p.mounts.push(m);
        })
        .contains(&IssueCode::InvDestination)
    );
    assert!(
        broken(|p| {
            let mut m = p.mounts.last().unwrap().clone();
            m.kind = MountKind::Dir {
                dst: "relative".into(),
                perms: None,
            };
            p.mounts.push(m);
        })
        .contains(&IssueCode::InvDestination)
    );
    assert!(broken(|p| p.command[1] = OsString::from("--other")).contains(&IssueCode::InvCommand));
    assert!(broken(|_| {}).is_empty());
}

#[test]
fn invariants_hold_for_every_preset_on_every_host() {
    for (_, host) in hosts::all() {
        let w = SandboxWorld::new(host);
        for p in [offline(), online(), standard(&w.host)] {
            let pl = ok(&w, &p);
            assert!(plan_invariants(&pl, &p, &w.ctx(), &w.host).is_empty());
        }
    }
}

// ---- renderer (SANDBOX-018) -------------------------------------------------------------

#[test]
fn renderer_perms_precede_and_descriptors_match() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    let r = render(&p).unwrap();
    for (i, a) in r.args.iter().enumerate() {
        if a == "--perms" {
            let next = r.args[i + 2].to_string_lossy().into_owned();
            assert!(
                ["--tmpfs", "--dir", "--ro-bind-data", "--file", "--size"].contains(&next.as_str()),
                "{next}"
            );
        }
    }
    let data_fds: Vec<i32> = p
        .mounts
        .iter()
        .filter_map(|m| match m.kind {
            MountKind::FileData { fd, .. } => Some(fd),
            _ => None,
        })
        .collect();
    assert_eq!(data_fds, vec![8, 9, 10, 11, 12]);
    let payload_fds: Vec<i32> = r.fds.iter().map(|f| f.fd).collect();
    assert_eq!(payload_fds, vec![7, 8, 9, 10, 11, 12]);
    let argv = r.argv(Path::new("/usr/bin/bwrap"));
    assert_eq!(
        &argv[..4],
        &["/usr/bin/bwrap", "--args", "3", "--"].map(OsString::from)
    );
}

#[test]
fn hostile_paths_survive_and_nul_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let mut w = SandboxWorld::worked_example();
    let root = PathBuf::from("/mnt/games/We\"ird\n -name $x");
    let exe = root.join("-game.exe");
    w.fs.file(&exe, b"MZ");
    w.root = Some(root.clone());
    w.exe = exe.clone();
    w.umu_args = vec![exe.clone().into_os_string()];
    let p = ok(&w, &offline());
    let r = render(&p).unwrap();
    assert!(r.args.iter().any(|a| a == root.as_os_str()));
    assert_eq!(r.command.last().unwrap(), exe.as_os_str());
    let payload = r.args_payload();
    assert_eq!(payload.iter().filter(|b| **b == 0).count(), r.args.len());
    let mut bad = p.clone();
    bad.chdir = PathBuf::from(std::ffi::OsStr::from_bytes(b"/a\0b"));
    assert!(render(&bad).is_err());
}

// ---- explain (SANDBOX-019) --------------------------------------------------------------

#[test]
fn explain_per_preset() {
    let w = SandboxWorld::worked_example();
    let keys = |p: &SandboxPolicy| {
        explain(&ok(&w, p), p)
            .into_iter()
            .map(|s| s.key)
            .collect::<Vec<_>>()
    };
    let off = keys(&offline());
    assert!(off.contains(&"access.network.none".to_owned()));
    assert!(off.contains(&"access.folders.none".to_owned()));
    assert!(off.contains(&"access.identity.machine_id_masked".to_owned()));
    let on = keys(&online());
    assert!(on.contains(&"access.network.full".to_owned()));
    let st = keys(&standard(&w.host));
    assert!(st.contains(&"access.identity.machine_id_real".to_owned()));
    assert!(st.contains(&"access.devices.all_input".to_owned()));
    let mut g = offline();
    g.folders
        .push(grant("/home/sam/Music", FolderAccess::ReadOnly, None));
    let with = keys(&g);
    assert!(
        !with.contains(&"access.folders.none".to_owned()),
        "the reassurance line only when true"
    );
    assert!(with.contains(&"access.folders.read_only".to_owned()));
    let d = explain_diff(
        &explain(&ok(&w, &offline()), &offline()),
        &explain(&ok(&w, &g), &g),
    );
    assert!(d.added.iter().any(|s| s.key == "access.folders.read_only"));
    assert!(d.removed.iter().any(|s| s.key == "access.folders.none"));
}

// ---- host actions (SANDBOX-020) ---------------------------------------------------------

#[test]
fn host_actions_are_listed_in_plans() {
    let w = SandboxWorld::worked_example();
    let p = ok(&w, &offline());
    assert!(p.host_actions.contains(&HostAction::CreateDir {
        path: w.game_paths.private_home.clone(),
        mode: 0o700
    }));
    assert!(p.host_actions.contains(&HostAction::AcquireLock {
        path: w.game_paths.prefix_lock.clone()
    }));
    assert!(p.host_actions.contains(&HostAction::WriteMemfd { fd: 3 }));
    assert_eq!(
        p.host_actions.last(),
        Some(&HostAction::MarkTainted { game: w.game_id })
    );
    let mut prep = SandboxWorld::worked_example();
    prep.set_kind(SessionKind::Prepare, None);
    assert!(
        !ok(&prep, &offline())
            .host_actions
            .iter()
            .any(|a| matches!(a, HostAction::MarkTainted { .. }))
    );
    // Planning is pure: the filesystem is unchanged (dry runs carry but never execute actions).
    let before = w.fs.paths();
    let _ = ok(&w, &offline());
    assert_eq!(w.fs.paths(), before);
}

// ---- sound and the microphone (ADR-0026) ------------------------------------------------

fn statement_keys(w: &SandboxWorld, p: &SandboxPolicy) -> Vec<String> {
    explain(&ok(w, p), p).into_iter().map(|s| s.key).collect()
}

#[test]
fn sound_without_the_microphone_is_the_sessions_own_server() {
    let w = SandboxWorld::worked_example();
    let p = offline();
    assert_eq!(p.devices.audio, AudioAccess::Playback);
    let g = ok(&w, &p);
    let dir = w.ctx().session.run_dir.join("sound");
    assert!(g.host_actions.iter().any(|a| matches!(
        a,
        HostAction::StartSoundServer { dir: d, remote, .. }
            if *d == dir && remote == Path::new("/run/user/1000/pipewire-0")
    )));
    let pulse = &g.mounts[index_of(&g, "/run/user/1000/pulse/native")];
    assert_eq!(pulse.kind.src(), Some(dir.join("native").as_path()));
    // The host's sockets record whatever their server is asked to: neither is bound.
    for m in &g.mounts {
        let src = m.kind.src().map(Path::to_owned).unwrap_or_default();
        assert!(!src.ends_with("pipewire-0"), "{m:?}");
        assert_ne!(src, Path::new("/run/user/1000/pulse/native"), "{m:?}");
    }
    assert!(plan_invariants(&g, &p, &w.ctx(), &w.host).is_empty());
    let keys = statement_keys(&w, &p);
    assert!(keys.contains(&"access.devices.sound".to_owned()));
    assert!(!keys.iter().any(|k| k.contains("microphone")), "{keys:?}");
}

#[test]
fn the_microphone_is_the_hosts_sockets_and_is_said() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    p.devices.audio = AudioAccess::Full;
    let g = ok(&w, &p);
    assert!(
        !g.host_actions
            .iter()
            .any(|a| matches!(a, HostAction::StartSoundServer { .. }))
    );
    let pulse = &g.mounts[index_of(&g, "/run/user/1000/pulse/native")];
    assert_eq!(
        pulse.kind.src(),
        Some(Path::new("/run/user/1000/pulse/native"))
    );
    assert_eq!(pulse.origin.policy_field, Some(PolicyField::Microphone));
    index_of(&g, "/run/user/1000/pipewire-0");
    let keys = statement_keys(&w, &p);
    assert!(keys.contains(&"access.devices.sound".to_owned()));
    assert!(keys.contains(&"access.devices.microphone".to_owned()));
}

#[test]
fn a_host_that_cannot_withhold_the_microphone_gives_no_sound() {
    for host in [hosts::no_pipewire(), {
        let mut h = hosts::arch_mesa();
        h.audio.pipewire_program = None;
        h
    }] {
        let w = SandboxWorld::new(host);
        let g = ok(&w, &offline());
        assert!(
            g.notes
                .iter()
                .any(|n| n.key == "note.audio.no_playback_only")
        );
        assert!(
            !g.mounts
                .iter()
                .any(|m| m.kind.dst().starts_with("/run/user/1000/pulse"))
        );
        assert_eq!(g.env.get("PULSE_SERVER"), None);
        assert!(
            !statement_keys(&w, &offline())
                .iter()
                .any(|k| k.contains("sound"))
        );
    }
}

#[test]
fn direct_sound_leaves_out_capture_devices_without_the_microphone() {
    let w = SandboxWorld::worked_example();
    let mut p = offline();
    p.devices.alsa_devices = true;
    let g = ok(&w, &p);
    let snd: Vec<&Path> = g
        .mounts
        .iter()
        .map(|m| m.kind.dst())
        .filter(|d| d.starts_with("/dev/snd"))
        .collect();
    assert_eq!(
        snd,
        ["controlC0", "pcmC0D0p", "seq", "timer"]
            .map(|n| Path::new("/dev/snd").join(n))
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>()
    );
    p.devices.audio = AudioAccess::Full;
    let g = ok(&w, &p);
    index_of(&g, "/dev/snd");
}

// ---- the restricted Wayland socket (ADR-0029) -------------------------------------------

/// The compositor's own socket on the fixtures.
const COMPOSITOR: &str = "/run/user/1000/wayland-0";

fn context_actions(g: &SandboxPlan) -> Vec<&HostAction> {
    g.host_actions
        .iter()
        .filter(|a| matches!(a, HostAction::CreateWaylandContext { .. }))
        .collect()
}

fn binds_compositor(g: &SandboxPlan) -> bool {
    g.mounts
        .iter()
        .any(|m| m.kind.src() == Some(Path::new(COMPOSITOR)))
}

#[test]
fn a_compositor_with_security_contexts_gives_the_game_a_restricted_socket() {
    let w = SandboxWorld::new(hosts::nixos());
    for p in [offline(), online(), standard(&w.host)] {
        let g = ok(&w, &p);
        let socket = w.ctx().session.run_dir.join("wayland");
        let actions = context_actions(&g);
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(matches!(
            actions[0],
            HostAction::CreateWaylandContext { display, socket: s, app_id, instance_id }
                if display == Path::new(COMPOSITOR)
                    && *s == socket
                    && *app_id == w.game_id.to_string()
                    && *instance_id == w.ctx().session.id.to_string()
        ));
        // The game finds it under the usual name; the compositor's own socket is not bound.
        let m = &g.mounts[index_of(&g, COMPOSITOR)];
        assert_eq!(m.kind.src(), Some(socket.as_path()));
        assert_eq!(m.origin.policy_field, Some(PolicyField::Wayland));
        assert!(!binds_compositor(&g));
        assert_eq!(g.env.get("WAYLAND_DISPLAY"), Some("wayland-0"));
        assert!(
            !g.notes
                .iter()
                .any(|n| n.key == "note.display.no_wayland_context")
        );
        assert!(plan_invariants(&g, &p, &w.ctx(), &w.host).is_empty());
        let st = explain(&g, &p)
            .into_iter()
            .find(|s| s.key == "access.display.wayland")
            .unwrap();
        assert!(
            st.flags.iter().any(|f| f.starts_with("security context")),
            "{st:?}"
        );
    }
}

#[test]
fn without_security_contexts_the_compositor_socket_is_bound_and_noted() {
    let w = SandboxWorld::worked_example();
    assert!(!w.host.display.wayland.as_ref().unwrap().security_context);
    let p = offline();
    let g = ok(&w, &p);
    assert!(context_actions(&g).is_empty());
    let m = &g.mounts[index_of(&g, COMPOSITOR)];
    assert_eq!(m.kind.src(), Some(Path::new(COMPOSITOR)));
    assert!(
        g.notes
            .iter()
            .any(|n| n.key == "note.display.no_wayland_context")
    );
    let st = explain(&g, &p)
        .into_iter()
        .find(|s| s.key == "access.display.wayland")
        .unwrap();
    assert!(
        st.flags.iter().any(|f| f.contains("no security context")),
        "{st:?}"
    );
}

#[test]
fn a_session_directory_too_long_for_a_socket_falls_back_to_the_compositor_socket() {
    let mut w = SandboxWorld::new(hosts::nixos());
    w.session.run_dir = PathBuf::from("/run/user/1000").join("x".repeat(120));
    let g = ok(&w, &offline());
    assert!(context_actions(&g).is_empty());
    assert!(binds_compositor(&g));
    assert!(
        g.notes
            .iter()
            .any(|n| n.key == "note.display.no_wayland_context")
    );
}

#[test]
fn wayland_turned_off_creates_no_context() {
    let w = SandboxWorld::new(hosts::nixos());
    let mut p = offline();
    p.display.wayland = SocketAccess::Deny;
    let g = ok(&w, &p);
    assert!(context_actions(&g).is_empty());
    assert!(
        !g.mounts
            .iter()
            .any(|m| m.kind.dst() == Path::new(COMPOSITOR))
    );
    assert!(
        !g.notes
            .iter()
            .any(|n| n.key == "note.display.no_wayland_context")
    );
}

#[test]
fn the_compositor_socket_next_to_the_restricted_one_is_caught() {
    let w = SandboxWorld::new(hosts::nixos());
    let issues = match zaun_core::sandbox::planner::plan_with(&offline(), &w.ctx(), &w.host, |p| {
        let mut m = p.mounts.last().unwrap().clone();
        m.kind = MountKind::Bind {
            src: COMPOSITOR.into(),
            dst: "/run/user/1000/wayland-9".into(),
            mode: MountMode::Ro,
            try_: false,
        };
        p.mounts.push(m);
    }) {
        Ok(_) => Vec::new(),
        Err(i) => i.iter().map(|i| i.code).collect(),
    };
    assert!(issues.contains(&IssueCode::InvSocket), "{issues:?}");
}
