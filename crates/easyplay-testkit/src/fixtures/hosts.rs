//! `HostFacts` presets (MODULES §11, TESTING_STRATEGY §4.1) and [`SandboxWorld`], a complete
//! fixture for sandbox planning: an in-memory filesystem with the runtime store, a game, the
//! user's home, plus every context input.
//!
//! [`arch_mesa`] with [`SandboxWorld::worked_example`] is the host of BUBBLEWRAP_SANDBOX §6.
//! The presets are written by hand in the shape `host::probe_all` produces.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use easyplay_core::clock::Timestamp;
use easyplay_core::fsx::MountTable;
use easyplay_core::host::{
    BwrapFacts, BwrapFlag, DeviceNode, EtcLink, GpuVendor, HostEntry, HostFacts, InputClass,
    InputNode, InputNodeKind, LayoutKind, ResolverStrategy, SessionType, ToolVersion, UmuFacts,
    WaylandSocket, X11Display,
};
use easyplay_core::ids::{GameId, RuntimeName, SessionId};
use easyplay_core::model::{
    EnvVar, LaunchConfig, NetworkAccess, ResolvedRuntime, RuntimeIsolationMode, RuntimeOrigin,
    SessionKind, SlrVariant, UserDir,
};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::sandbox::{GameContext, GamePaths, LaunchInputs, SandboxContext, SessionDirs};

use crate::MemFs;

/// Host machine ID used by every preset.
pub const MACHINE_ID: &str = "4c4c4544004e3410804bb2c04f4e3532";
/// Install ID (Provision stand-in machine ID).
pub const INSTALL_ID: &str = "0123456789abcdef0123456789abcdef";
/// The worked example's game ID (`G`).
pub const GAME_ID: &str = "0192f0c2-1c3a-7d4e-8f00-000000000001";
/// The worked example's session ID (`S`).
pub const SESSION_ID: &str = "0192f0c2-1c3a-7d4e-8f00-0000000000a5";
/// Platform directory of the fixture SLR.
pub const PLATFORM: &str = "sniper_platform_3.0.20261110.268881";
/// Proton build of the fixture.
pub const PROTON: &str = "GE-Proton10-25";

fn link(target: &str, resolved: &str) -> HostEntry {
    HostEntry::Symlink {
        target: target.into(),
        resolved: Some(resolved.into()),
    }
}

fn set_etc(h: &mut HostFacts, entries: &[(&str, HostEntry)]) {
    for (k, v) in entries {
        if let HostEntry::Symlink {
            target,
            resolved: Some(r),
        } = v
        {
            h.etc_links.links.insert(
                (*k).to_owned(),
                EtcLink {
                    target: target.clone(),
                    chain: vec![Path::new("/etc").join(k), r.clone()],
                    resolved: Some(r.clone()),
                    is_dir: false,
                    via_etc_static: target.starts_with("/etc/static"),
                },
            );
        }
        h.layout.etc.insert((*k).to_owned(), v.clone());
    }
}

fn set_top(h: &mut HostFacts, entries: &[(&str, Option<&str>)]) {
    h.layout.top_level = entries
        .iter()
        .map(|(k, t)| {
            (
                PathBuf::from(*k),
                match t {
                    Some(t) => HostEntry::Symlink {
                        target: (*t).into(),
                        resolved: Some(Path::new("/").join(t)),
                    },
                    None => HostEntry::Dir,
                },
            )
        })
        .collect();
}

fn all_flags() -> BTreeSet<BwrapFlag> {
    BwrapFlag::ALL
        .iter()
        .copied()
        .filter(|f| *f != BwrapFlag::DisableUserns)
        .collect()
}

fn node(path: &str, kind: InputNodeKind, class: InputClass, readable: bool) -> InputNode {
    InputNode {
        path: path.into(),
        kind,
        dev: (13, 0),
        classes: [class].into_iter().collect(),
        readable,
        siblings: Vec::new(),
    }
}

/// Arch Linux, Mesa GPU, GNOME Wayland session with Xwayland, btrfs (BUBBLEWRAP_SANDBOX §6).
pub fn arch_mesa() -> HostFacts {
    let mut h = HostFacts::empty(Timestamp::from_millis(1_794_480_000_000));
    h.os.id = Some("arch".into());
    h.machine_id = Some(MACHINE_ID.into());
    h.bwrap = Some(BwrapFacts {
        path: "/usr/bin/bwrap".into(),
        version: Some(ToolVersion::new(0, 11, 0)),
        flags: all_flags(),
        setuid: false,
        userns_ok: true,
        nested_userns_ok: true,
        overlay_ok: true,
    });
    h.umu = Some(UmuFacts {
        path: "/usr/bin/umu-run".into(),
        version: Some(ToolVersion::new(1, 4, 4)),
        closure: Vec::new(),
        wrapper: None,
    });
    h.layout.kind = LayoutKind::Fhs;
    h.layout.has_usr = true;
    h.layout.has_opt = true;
    set_top(
        &mut h,
        &[
            ("/bin", Some("usr/bin")),
            ("/sbin", Some("usr/bin")),
            ("/lib", Some("usr/lib")),
            ("/lib64", Some("usr/lib")),
        ],
    );
    set_etc(
        &mut h,
        &[
            ("ld.so.cache", HostEntry::File),
            ("ld.so.conf", HostEntry::File),
            ("ld.so.conf.d", HostEntry::Dir),
            ("fonts", HostEntry::Dir),
            (
                "os-release",
                link("../usr/lib/os-release", "/usr/lib/os-release"),
            ),
            (
                "localtime",
                link(
                    "../usr/share/zoneinfo/Europe/Berlin",
                    "/usr/share/zoneinfo/Europe/Berlin",
                ),
            ),
            ("vulkan", HostEntry::Dir),
            ("pulse", HostEntry::Dir),
            ("alsa", HostEntry::Dir),
            ("machine-id", HostEntry::File),
            ("hosts", HostEntry::File),
            ("host.conf", HostEntry::File),
            ("gai.conf", HostEntry::File),
            ("ssl", HostEntry::Dir),
            ("ca-certificates", HostEntry::Dir),
        ],
    );
    h.layout.etc.insert(
        "resolv.conf".into(),
        link(
            "../run/systemd/resolve/stub-resolv.conf",
            "/run/systemd/resolve/stub-resolv.conf",
        ),
    );
    h.layout.resolver = ResolverStrategy::Symlink {
        target: "../run/systemd/resolve/stub-resolv.conf".into(),
        bind_dir: "/run/systemd/resolve".into(),
    };
    h.layout.path_env = "/usr/local/bin:/usr/bin:/bin".into();
    h.gpu.vendors = vec![GpuVendor::Amd];
    h.gpu.dri_present = true;
    h.gpu.render_nodes = vec![DeviceNode {
        path: "/dev/dri/renderD128".into(),
        accessible: true,
    }];
    h.gpu.ntsync = true;
    h.gpu.has_32bit_driver = Some(true);
    h.video_nodes = vec!["/dev/video0".into(), "/dev/video1".into()];
    h.display.session_type = SessionType::Wayland;
    h.display.desktop = Some("GNOME".into());
    h.display.xdg_runtime_dir = Some("/run/user/1000".into());
    h.display.wayland = Some(WaylandSocket {
        name: "wayland-0".into(),
        path: "/run/user/1000/wayland-0".into(),
    });
    h.display.x11 = Some(X11Display {
        number: 0,
        socket: "/tmp/.X11-unix/X0".into(),
        filesystem_socket: true,
        authority: Some("/run/user/1000/.mutter-Xwaylandauth.A1B2C3".into()),
    });
    h.audio.pulse_socket = Some("/run/user/1000/pulse/native".into());
    h.audio.pipewire_socket = Some("/run/user/1000/pipewire-0".into());
    h.audio.alsa_devices = true;
    h.input.nodes = vec![
        node(
            "/dev/hidraw0",
            InputNodeKind::Hidraw,
            InputClass::Keyboard,
            false,
        ),
        node(
            "/dev/hidraw4",
            InputNodeKind::Hidraw,
            InputClass::Joystick,
            true,
        ),
        node(
            "/dev/input/event21",
            InputNodeKind::Event,
            InputClass::Joystick,
            true,
        ),
        node(
            "/dev/input/event3",
            InputNodeKind::Event,
            InputClass::Keyboard,
            false,
        ),
        node(
            "/dev/input/event5",
            InputNodeKind::Event,
            InputClass::Mouse,
            false,
        ),
        node(
            "/dev/input/js0",
            InputNodeKind::Js,
            InputClass::Joystick,
            true,
        ),
    ];
    h.input.keyboards_readable = false;
    h.input.udev_db = true;
    h.sysfs.net_device_dirs = vec!["/sys/devices/pci0000:00/0000:00:1f.6/net".into()];
    h.fs.reflink_ok = Some(true);
    h.fs.store_device = Some(2);
    h.fs.store_fs_type = Some("btrfs".into());
    h
}

/// Fedora with the NVIDIA proprietary driver.
pub fn fedora_nvidia() -> HostFacts {
    let mut h = arch_mesa();
    h.os.id = Some("fedora".into());
    set_top(
        &mut h,
        &[
            ("/bin", Some("usr/bin")),
            ("/sbin", Some("usr/sbin")),
            ("/lib", Some("usr/lib")),
            ("/lib64", Some("usr/lib64")),
        ],
    );
    h.layout.etc.remove("ssl");
    h.layout.etc.remove("ca-certificates");
    set_etc(
        &mut h,
        &[
            ("pki", HostEntry::Dir),
            ("nvidia", HostEntry::Dir),
            ("OpenCL", HostEntry::Dir),
            (
                "localtime",
                link(
                    "../usr/share/zoneinfo/America/New_York",
                    "/usr/share/zoneinfo/America/New_York",
                ),
            ),
        ],
    );
    h.gpu.vendors = vec![GpuVendor::Nvidia];
    h.gpu.nvidia_nodes = [
        "/dev/nvidia-caps",
        "/dev/nvidia-modeset",
        "/dev/nvidia-uvm",
        "/dev/nvidia-uvm-tools",
        "/dev/nvidia0",
        "/dev/nvidiactl",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    h.gpu.ntsync = false;
    h.sysfs.net_device_dirs = vec![
        "/sys/devices/pci0000:00/0000:00:1c.0/0000:03:00.0/net".into(),
        "/sys/devices/pci0000:00/0000:00:1f.6/net".into(),
    ];
    h
}

/// Ubuntu LTS with AppArmor, Debian-family layout (alternatives, lib32, libx32), ext4.
pub fn ubuntu_lts_apparmor() -> HostFacts {
    let mut h = arch_mesa();
    h.os.id = Some("ubuntu".into());
    h.os.id_like = vec!["debian".into()];
    set_top(
        &mut h,
        &[
            ("/bin", Some("usr/bin")),
            ("/sbin", Some("usr/sbin")),
            ("/lib", Some("usr/lib")),
            ("/lib32", Some("usr/lib32")),
            ("/lib64", Some("usr/lib64")),
            ("/libx32", Some("usr/libx32")),
        ],
    );
    set_etc(
        &mut h,
        &[
            ("alternatives", HostEntry::Dir),
            ("timezone", HostEntry::File),
            ("ca-certificates.conf", HostEntry::File),
            ("glvnd", HostEntry::Dir),
            ("drirc", HostEntry::File),
        ],
    );
    h.gpu.vendors = vec![GpuVendor::Intel];
    h.gpu.ntsync = false;
    h.fs.reflink_ok = Some(false);
    h.fs.store_fs_type = Some("ext4".into());
    h
}

/// Debian stable with an old bubblewrap (no overlay support), not merged `/usr`.
pub fn debian_stable_old_bwrap() -> HostFacts {
    let mut h = ubuntu_lts_apparmor();
    h.os.id = Some("debian".into());
    h.os.id_like = Vec::new();
    if let Some(b) = &mut h.bwrap {
        b.version = Some(ToolVersion::new(0, 8, 0));
        for f in [
            BwrapFlag::OverlaySrc,
            BwrapFlag::Overlay,
            BwrapFlag::TmpOverlay,
            BwrapFlag::RoOverlay,
        ] {
            b.flags.remove(&f);
        }
        b.overlay_ok = false;
    }
    set_top(
        &mut h,
        &[
            ("/bin", None),
            ("/sbin", None),
            ("/lib", None),
            ("/lib64", None),
        ],
    );
    h.layout.extra_lib_roots = vec!["/srv/gl-libs".into()];
    h.layout.etc.insert("resolv.conf".into(), HostEntry::File);
    h.layout.resolver = ResolverStrategy::File;
    h
}

/// NixOS: `/nix`, `/run/current-system`, `/etc` links through `/etc/static`.
pub fn nixos() -> HostFacts {
    let mut h = arch_mesa();
    h.os.id = Some("nixos".into());
    h.umu = Some(UmuFacts {
        path: "/run/current-system/sw/bin/umu-run".into(),
        version: Some(ToolVersion::new(1, 4, 4)),
        closure: vec!["/nix/store/abc-umu-launcher-1.4.4".into()],
        wrapper: None,
    });
    h.layout.kind = LayoutKind::Nix;
    h.layout.etc.clear();
    h.etc_links.links.clear();
    set_top(&mut h, &[("/bin", None)]);
    let store = "/nix/store/x1-etc/etc";
    let via_static = |name: &str| link(&format!("/etc/static/{name}"), &format!("{store}/{name}"));
    set_etc(
        &mut h,
        &[
            ("fonts", via_static("fonts")),
            ("os-release", via_static("os-release")),
            (
                "localtime",
                link(
                    "/etc/zoneinfo/Europe/Berlin",
                    "/nix/store/t1-tzdata/share/zoneinfo/Europe/Berlin",
                ),
            ),
            ("pulse", via_static("pulse")),
            ("machine-id", HostEntry::File),
            ("hosts", via_static("hosts")),
            ("resolv.conf", HostEntry::File),
            ("ssl", via_static("ssl")),
        ],
    );
    // `zoneinfo` is not on the layout allow-list; the probe records it in `etc_links` only.
    h.etc_links.links.insert(
        "zoneinfo".into(),
        EtcLink {
            target: "/etc/static/zoneinfo".into(),
            chain: vec![
                "/etc/zoneinfo".into(),
                "/etc/static/zoneinfo".into(),
                "/nix/store/t1-tzdata/share/zoneinfo".into(),
            ],
            resolved: Some("/nix/store/t1-tzdata/share/zoneinfo".into()),
            is_dir: true,
            via_etc_static: true,
        },
    );
    h.layout.resolver = ResolverStrategy::File;
    h.layout.path_env = "/run/current-system/sw/bin:/usr/bin:/bin".into();
    h.layout.passthrough_env = [
        "LD_LIBRARY_PATH",
        "LIBGL_DRIVERS_PATH",
        "LOCALE_ARCHIVE",
        "TZDIR",
        "XDG_DATA_DIRS",
        "XDG_CONFIG_DIRS",
        "VK_DRIVER_FILES",
        "SSL_CERT_FILE",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    h
}

/// No unprivileged user namespaces (the planner still plans; the probe reports it).
pub fn no_userns() -> HostFacts {
    let mut h = ubuntu_lts_apparmor();
    if let Some(b) = &mut h.bwrap {
        b.userns_ok = false;
        b.nested_userns_ok = false;
    }
    h
}

/// X11 session, no Wayland socket; authority file in the real home.
pub fn x11_only() -> HostFacts {
    let mut h = arch_mesa();
    h.display.session_type = SessionType::X11;
    h.display.wayland = None;
    h.display.x11 = Some(X11Display {
        number: 1,
        socket: "/tmp/.X11-unix/X1".into(),
        filesystem_socket: true,
        authority: Some("/home/sam/.Xauthority".into()),
    });
    h
}

/// PulseAudio only (no PipeWire socket).
pub fn no_pipewire() -> HostFacts {
    let mut h = arch_mesa();
    h.audio.pipewire_socket = None;
    h
}

/// The user is in the `input` group: keyboards and mice are readable.
pub fn input_group_user() -> HostFacts {
    let mut h = arch_mesa();
    for n in &mut h.input.nodes {
        n.readable = true;
    }
    h.input.keyboards_readable = true;
    h
}

/// Every preset with its name.
pub fn all() -> Vec<(&'static str, HostFacts)> {
    vec![
        ("arch_mesa", arch_mesa()),
        ("fedora_nvidia", fedora_nvidia()),
        ("ubuntu_lts_apparmor", ubuntu_lts_apparmor()),
        ("debian_stable_old_bwrap", debian_stable_old_bwrap()),
        ("nixos", nixos()),
        ("no_userns", no_userns()),
        ("x11_only", x11_only()),
        ("no_pipewire", no_pipewire()),
        ("input_group_user", input_group_user()),
    ]
}

/// A preset by name.
pub fn by_name(name: &str) -> HostFacts {
    all()
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, h)| h)
        .unwrap_or_else(|| panic!("unknown host preset {name}"))
}

// ---- world ---------------------------------------------------------------------------

/// SLR top-level entries of the fixture (names; `umu` is a link to `_v2-entry-point`).
pub const SLR_FILES: &[&str] = &[
    "README.md",
    "VERSIONS.txt",
    "_v2-entry-point",
    "mtree.txt.gz",
    "run",
    "run-in-sniper",
    "toolmanifest.vdf",
    ".installed.ok",
];
/// SLR top-level directories of the fixture.
pub const SLR_DIRS: &[&str] = &[
    "pressure-vessel",
    "pressure-vessel-arm64",
    "steampipe",
    "var",
];

/// Everything a planning call needs, owned.
pub struct SandboxWorld {
    /// Filesystem.
    pub fs: MemFs,
    /// Locations.
    pub paths: AppPaths,
    /// Host facts.
    pub host: HostFacts,
    /// Mount table.
    pub mounts: MountTable,
    /// Game.
    pub game_id: GameId,
    /// Game paths.
    pub game_paths: GamePaths,
    /// Game folder.
    pub root: Option<PathBuf>,
    /// Executable.
    pub exe: PathBuf,
    /// Session.
    pub session: SessionDirs,
    /// Runtime.
    pub runtime: ResolvedRuntime,
    /// Host environment.
    pub host_env: BTreeMap<String, String>,
    /// Launch configuration.
    pub launch: LaunchConfig,
    /// Global user variables.
    pub global_env: Vec<EnvVar>,
    /// umu arguments.
    pub umu_args: Vec<OsString>,
    /// Init shim on the host.
    pub init: PathBuf,
    /// XDG user directories.
    pub user_dirs: Vec<(UserDir, PathBuf)>,
    /// Other games' folders.
    pub other_roots: Vec<PathBuf>,
    /// Session kind.
    pub kind: SessionKind,
    /// Network override.
    pub network_override: Option<NetworkAccess>,
    /// Developer Mode.
    pub developer_mode: bool,
    /// Seccomp available.
    pub seccomp_available: bool,
    /// Proton codename (trusted sessions).
    pub proton_codename: Option<String>,
    /// umu ID.
    pub umu_id: Option<String>,
    /// Store.
    pub store: Option<String>,
    /// Verbose logging.
    pub verbose: bool,
    /// `umu-run` facts used by the context (defaults to `host.umu`).
    pub umu: UmuFacts,
}

/// The home of every world.
pub const HOME: &str = "/home/sam";

impl SandboxWorld {
    /// A world on `host` with the worked example's user, game and runtime.
    pub fn new(host: HostFacts) -> Self {
        let env = PathEnv::new(1000)
            .with("HOME", HOME)
            .with("XDG_RUNTIME_DIR", "/run/user/1000");
        let paths = AppPaths::resolve(&env).expect("paths");
        let game_id: GameId = GAME_ID.parse().expect("game id");
        let session_id: SessionId = SESSION_ID.parse().expect("session id");
        let fs = MemFs::new();
        let root = PathBuf::from("/mnt/games/Hollow Knight");
        let exe = root.join("hollow_knight.exe");
        fs.mount("/proc", 10, "proc")
            .mount("/sys", 11, "sysfs")
            .mount("/dev", 12, "devtmpfs")
            .mount("/mnt/games", 2, "btrfs")
            .mount("/run/media/sam/USB", 20, "vfat")
            .file(&exe, b"MZ")
            .file(root.join("UnityPlayer.dll"), b"MZ")
            .dir(root.join("hollow_knight_Data"))
            .dir("/home/sam/Documents")
            .dir("/home/sam/Music")
            .dir("/home/sam/Downloads")
            .dir("/home/sam/Games/Mods")
            .dir("/home/sam/.ssh")
            .dir("/home/sam/.config/gh")
            .dir("/home/sam/.local/share/fonts")
            .dir("/home/sam/.local/bin")
            .dir("/mnt/data/shared/inner")
            .dir("/mnt/saves/hk")
            .dir("/run/media/sam/USB/Games")
            .link("/home/sam/sshlink", "/home/sam/.ssh")
            .char_device("/dev/hidraw7", 0xF007)
            .char_device("/dev/uinput", 0xA0DF)
            .dir(paths.home_dir(game_id))
            .dir(paths.prefix_dir(game_id))
            .dir(paths.session_var_root.join(SESSION_ID));
        let slr = paths.slr_dir(SlrVariant::Steamrt3);
        for f in SLR_FILES {
            fs.file(slr.join(f), b"x");
        }
        for d in SLR_DIRS {
            fs.dir(slr.join(d));
        }
        fs.link(slr.join("umu"), "_v2-entry-point");
        fs.file(slr.join(PLATFORM).join("files/.ref"), b"");
        let proton = paths.compat_tools_dir.join(PROTON);
        fs.file(proton.join("proton"), b"#!/usr/bin/env python3");
        let mounts = MountTable::parse(fs.mountinfo().as_bytes());
        let runtime = ResolvedRuntime {
            name: RuntimeName::new(PROTON).expect("name"),
            proton_path_host: proton,
            proton_path_sandbox: PathBuf::from(HOME)
                .join(".local/share/Steam/compatibilitytools.d")
                .join(PROTON),
            origin: RuntimeOrigin::Store,
            slr_variant: SlrVariant::Steamrt3,
            slr_dir: slr,
            platform_dir: PLATFORM.into(),
            isolation: if host.fs.reflink_ok == Some(true) {
                RuntimeIsolationMode::Isolated
            } else {
                RuntimeIsolationMode::Shared
            },
        };
        let host_env: BTreeMap<String, String> = [
            ("LANG", "en_US.UTF-8"),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
            ("XDG_SESSION_TYPE", "wayland"),
            ("HOME", "/home/sam"),
            ("PATH", "/home/sam/.local/bin:/usr/bin"),
            ("AWS_SECRET_ACCESS_KEY", "decoy"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("SSH_AUTH_SOCK", "/run/user/1000/ssh-agent"),
            ("DXVK_CONFIG_FILE", "/home/sam/dxvk.conf"),
            ("PROTON_USE_WINED3D", "1"),
            ("MY_TOOL_SETTING", "value"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        let umu = host.umu.clone().expect("preset has umu");
        Self {
            game_paths: GamePaths::new(&paths, game_id),
            session: SessionDirs::new(&paths, session_id),
            fs,
            paths,
            host,
            mounts,
            game_id,
            root: Some(root),
            exe: exe.clone(),
            runtime,
            host_env,
            launch: LaunchConfig::default(),
            global_env: Vec::new(),
            umu_args: vec![exe.into_os_string()],
            init: "/usr/libexec/easyplay/easyplay-init".into(),
            user_dirs: vec![
                (UserDir::Documents, "/home/sam/Documents".into()),
                (UserDir::Music, "/home/sam/Music".into()),
                (UserDir::Downloads, "/home/sam/Downloads".into()),
            ],
            other_roots: vec!["/mnt/games/Celeste".into()],
            kind: SessionKind::Game,
            network_override: None,
            developer_mode: false,
            seccomp_available: true,
            proton_codename: None,
            umu_id: None,
            store: None,
            verbose: false,
            umu,
        }
    }

    /// The world of BUBBLEWRAP_SANDBOX §6.
    pub fn worked_example() -> Self {
        Self::new(arch_mesa())
    }

    /// Switch to a loose executable in `~/Downloads`.
    pub fn loose_exe(&mut self) {
        let exe = PathBuf::from("/home/sam/Downloads/setup.exe");
        self.fs.file(&exe, b"MZ");
        self.root = None;
        self.exe = exe.clone();
        self.umu_args = vec![exe.into_os_string()];
    }

    /// Set up a session kind with the arguments `umu::command` would produce.
    pub fn set_kind(&mut self, kind: SessionKind, network: Option<NetworkAccess>) {
        self.kind = kind;
        self.network_override = network;
        if matches!(kind, SessionKind::Prepare | SessionKind::Provision) {
            self.umu_args = vec![OsString::new()];
        }
        if kind == SessionKind::Provision {
            self.proton_codename = Some("GE-Proton".into());
        }
    }

    /// The `SandboxContext` borrowing this world.
    pub fn ctx(&self) -> SandboxContext<'_> {
        let game = (self.kind != SessionKind::Provision).then(|| GameContext {
            id: self.game_id,
            private_home: &self.game_paths.private_home,
            prefix: &self.game_paths.prefix,
            overlay_upper: &self.game_paths.overlay_upper,
            overlay_work: &self.game_paths.overlay_work,
            root: self.root.as_deref(),
            exe: &self.exe,
            umu_id: self.umu_id.as_deref(),
            store: self.store.as_deref(),
            prefix_lock: &self.game_paths.prefix_lock,
        });
        SandboxContext {
            kind: self.kind,
            uid: 1000,
            gid: 1000,
            user_name: "sam",
            group_name: "sam",
            home: Path::new(HOME),
            game,
            runtime: (self.kind != SessionKind::Provision).then_some(&self.runtime),
            proton_codename: self.proton_codename.as_deref(),
            session: &self.session,
            paths: &self.paths,
            init_binary: &self.init,
            umu: &self.umu,
            network_override: self.network_override,
            launch: LaunchInputs {
                working_dir: &self.launch.working_dir,
                compat: &self.launch.compat,
                global_env: &self.global_env,
                game_env: &self.launch.env,
                umu_args: &self.umu_args,
                verbose_log: self.verbose,
            },
            host_env: &self.host_env,
            install_id: INSTALL_ID,
            developer_mode: self.developer_mode,
            seccomp_available: self.seccomp_available,
            user_dirs: &self.user_dirs,
            other_game_roots: &self.other_roots,
            mounts: &self.mounts,
            fs: &self.fs,
        }
    }

    /// Path placeholders for golden files: `D` (data), `G` (game ID), `S` (session ID).
    pub fn replacements(&self) -> Vec<(PathBuf, &'static str)> {
        vec![
            (self.paths.data_dir.clone(), "D"),
            (PathBuf::from(GAME_ID), "G"),
            (PathBuf::from(SESSION_ID), "S"),
        ]
    }
}
