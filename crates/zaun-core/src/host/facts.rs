//! `HostFacts`: everything the probe learned about the machine, as immutable data.
//!
//! Pure data with serde (cached as `<cache>/host-facts.json`). No probing logic lives here:
//! the probe-to-facts mapping is in the sibling files (`bwrap_probe.rs`, `layout.rs`, …) so
//! that this type can be shared by the sandbox planner without pulling in any I/O.
//!
//! Field names follow MODULES §4 (`BwrapFacts { path, version, flags, setuid, userns_ok,
//! nested_userns_ok, overlay_ok }`, `UmuFacts { path, version, closure }`, `FsLayout`,
//! `GpuFacts`, `DisplayFacts`, `AudioFacts`, `InputFacts`). Everything else is an addition
//! needed by BUBBLEWRAP_SANDBOX §4 and documented on the field.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub use super::etc_links::{EtcLink, EtcLinkFacts};
use crate::clock::Timestamp;
use crate::error::ErrorCode;

/// Version of the serialised [`HostFacts`] document. Bump on any incompatible change; the
/// cache is then simply re-probed. 3: the compositor's security-context support (ADR-0029).
pub const HOST_FACTS_FORMAT: u32 = 3;

/// Everything known about the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    /// [`HOST_FACTS_FORMAT`] of the writer.
    pub format: u32,
    /// When the probe ran.
    pub probed_at: Timestamp,
    /// `/etc/os-release` (distribution-specific hints).
    pub os: OsRelease,
    /// Kernel switches that decide whether user namespaces work.
    pub kernel: KernelFacts,
    /// bubblewrap; `None` when no binary was found (`ZN-HOST-001`).
    pub bwrap: Option<BwrapFacts>,
    /// umu-launcher; `None` when `umu-run` was not found (`ZN-HOST-010`).
    pub umu: Option<UmuFacts>,
    /// Filesystem layout (BUBBLEWRAP_SANDBOX §4.2–4.3).
    pub layout: FsLayout,
    /// Fully resolved targets of symlinked `/etc` entries (NixOS `/etc/static` chains).
    /// Separate from `layout` on purpose (see `etc_links.rs`).
    pub etc_links: EtcLinkFacts,
    /// GPU device nodes (§4.5).
    pub gpu: GpuFacts,
    /// Display sockets (§4.7).
    pub display: DisplayFacts,
    /// Sound sockets (§4.7).
    pub audio: AudioFacts,
    /// Input devices (SECURITY_MODEL §5.4).
    pub input: InputFacts,
    /// Sysfs paths the planner masks (§4.6).
    pub sysfs: SysfsFacts,
    /// The runtime store's filesystem (RUNTIME_MANAGEMENT §7).
    pub fs: StoreFsFacts,
    /// `/etc/machine-id` (32 lower-case hex digits) when readable: input of the stand-in
    /// machine ID (BUBBLEWRAP_SANDBOX §4.3). Added by the sandbox planner (SANDBOX-007).
    #[serde(default)]
    pub machine_id: Option<String>,
    /// `/dev/video*` character devices, sorted (BUBBLEWRAP_SANDBOX §4.5 camera). Added by the
    /// sandbox planner (SANDBOX-008).
    #[serde(default)]
    pub video_nodes: Vec<PathBuf>,
    /// Blocking problems and warnings found by the probe, in probe order.
    pub problems: Vec<HostProblem>,
    /// Non-blocking remarks ("PipeWire socket not found", …).
    pub notes: Vec<String>,
}

impl HostFacts {
    /// Facts describing a machine where nothing was found (the starting point of the probe
    /// and of test presets).
    pub fn empty(probed_at: Timestamp) -> Self {
        Self {
            format: HOST_FACTS_FORMAT,
            probed_at,
            os: OsRelease::default(),
            kernel: KernelFacts::default(),
            bwrap: None,
            umu: None,
            layout: FsLayout::default(),
            etc_links: EtcLinkFacts::default(),
            gpu: GpuFacts::default(),
            display: DisplayFacts::default(),
            audio: AudioFacts::default(),
            input: InputFacts::default(),
            sysfs: SysfsFacts::default(),
            fs: StoreFsFacts::default(),
            machine_id: None,
            video_nodes: Vec::new(),
            problems: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Whether a sandbox can be started at all: bwrap present with the required options and
    /// working user namespaces.
    pub fn can_sandbox(&self) -> bool {
        self.bwrap
            .as_ref()
            .is_some_and(|b| b.missing_required().is_empty() && b.userns_ok)
    }

    /// Whether `Protected` game folders are available (`overlay_ok`).
    pub fn overlay_ok(&self) -> bool {
        self.bwrap.as_ref().is_some_and(|b| b.overlay_ok)
    }

    /// The problem with `code`, if the probe recorded one.
    pub fn problem(&self, code: ErrorCode) -> Option<&HostProblem> {
        self.problems.iter().find(|p| p.code == code)
    }
}

// ---- versions -----------------------------------------------------------------------------

/// A `major.minor.patch` tool version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolVersion {
    /// Major.
    pub major: u32,
    /// Minor.
    pub minor: u32,
    /// Patch.
    pub patch: u32,
}

impl ToolVersion {
    /// A version.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for ToolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ToolVersion {
    type Err = String;
    /// `X`, `X.Y` or `X.Y.Z`, optionally followed by a non-digit suffix (`1.4.4rc1` →
    /// `1.4.4`).
    fn from_str(s: &str) -> Result<Self, String> {
        let mut parts = [0u32; 3];
        let mut n = 0;
        for piece in s.trim().split('.') {
            if n == 3 {
                break;
            }
            let digits: String = piece.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                break;
            }
            parts[n] = digits.parse().map_err(|_| s.to_owned())?;
            n += 1;
            if digits.len() != piece.len() {
                break;
            }
        }
        if n == 0 {
            return Err(s.to_owned());
        }
        Ok(Self::new(parts[0], parts[1], parts[2]))
    }
}

impl Serialize for ToolVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ToolVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ---- os and kernel --------------------------------------------------------------------------

/// The fields of `os-release` that select hints.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsRelease {
    /// `ID` (`arch`, `debian`, `ubuntu`, `fedora`, `nixos`, …).
    pub id: Option<String>,
    /// `ID_LIKE`, split on whitespace.
    pub id_like: Vec<String>,
    /// `VERSION_ID`.
    pub version_id: Option<String>,
    /// `PRETTY_NAME`.
    pub pretty_name: Option<String>,
}

impl OsRelease {
    /// Whether `id` is the distribution or one it is like.
    pub fn is_like(&self, id: &str) -> bool {
        self.id.as_deref() == Some(id) || self.id_like.iter().any(|l| l == id)
    }
}

/// Kernel switches read from `/proc/sys` (absent files are `None`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelFacts {
    /// `/proc/sys/kernel/unprivileged_userns_clone` (Debian-patched kernels).
    pub unprivileged_userns_clone: Option<u64>,
    /// `/proc/sys/user/max_user_namespaces`.
    pub max_user_namespaces: Option<u64>,
    /// `/proc/sys/kernel/apparmor_restrict_unprivileged_userns` (Ubuntu 23.10+).
    pub apparmor_restrict_unprivileged_userns: Option<u64>,
}

// ---- bubblewrap -----------------------------------------------------------------------------

macro_rules! bwrap_flags {
    ($( $(#[$m:meta])* $v:ident = $s:literal, $req:literal; )*) => {
        /// A `bwrap` command-line option Zaun knows about.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum BwrapFlag {
            $( $(#[$m])* $v, )*
        }

        impl BwrapFlag {
            /// Every known option, in declaration order.
            pub const ALL: &'static [BwrapFlag] = &[$(BwrapFlag::$v),*];

            /// The option as written on the command line, e.g. `--ro-bind`.
            pub const fn as_str(self) -> &'static str {
                match self { $(BwrapFlag::$v => $s,)* }
            }

            /// Whether Zaun cannot sandbox anything without it (`ZN-HOST-002`).
            pub const fn is_required(self) -> bool {
                match self { $(BwrapFlag::$v => $req,)* }
            }

            /// Parse [`BwrapFlag::as_str`].
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(BwrapFlag::$v),)* _ => None }
            }
        }
    };
}

bwrap_flags! {
    /// `--args FD`.
    Args = "--args", true;
    /// `--unshare-user`.
    UnshareUser = "--unshare-user", true;
    /// `--unshare-ipc`.
    UnshareIpc = "--unshare-ipc", true;
    /// `--unshare-pid`.
    UnsharePid = "--unshare-pid", true;
    /// `--unshare-net`.
    UnshareNet = "--unshare-net", true;
    /// `--unshare-uts`.
    UnshareUts = "--unshare-uts", true;
    /// `--unshare-cgroup-try`.
    UnshareCgroupTry = "--unshare-cgroup-try", true;
    /// `--hostname`.
    Hostname = "--hostname", true;
    /// `--chdir`.
    Chdir = "--chdir", true;
    /// `--clearenv`.
    Clearenv = "--clearenv", true;
    /// `--setenv`.
    Setenv = "--setenv", true;
    /// `--unsetenv`.
    Unsetenv = "--unsetenv", false;
    /// `--sync-fd`.
    SyncFd = "--sync-fd", true;
    /// `--bind`.
    Bind = "--bind", true;
    /// `--bind-try`.
    BindTry = "--bind-try", true;
    /// `--dev-bind`.
    DevBind = "--dev-bind", true;
    /// `--dev-bind-try`.
    DevBindTry = "--dev-bind-try", true;
    /// `--ro-bind`.
    RoBind = "--ro-bind", true;
    /// `--ro-bind-try`.
    RoBindTry = "--ro-bind-try", true;
    /// `--remount-ro`.
    RemountRo = "--remount-ro", false;
    /// `--overlay-src`.
    OverlaySrc = "--overlay-src", false;
    /// `--overlay`.
    Overlay = "--overlay", false;
    /// `--tmp-overlay`.
    TmpOverlay = "--tmp-overlay", false;
    /// `--ro-overlay`.
    RoOverlay = "--ro-overlay", false;
    /// `--proc`.
    Proc = "--proc", true;
    /// `--dev`.
    Dev = "--dev", true;
    /// `--tmpfs`.
    Tmpfs = "--tmpfs", true;
    /// `--dir`.
    Dir = "--dir", true;
    /// `--file`.
    File = "--file", true;
    /// `--bind-data`.
    BindData = "--bind-data", false;
    /// `--ro-bind-data`.
    RoBindData = "--ro-bind-data", true;
    /// `--symlink`.
    Symlink = "--symlink", true;
    /// `--seccomp`.
    Seccomp = "--seccomp", true;
    /// `--add-seccomp-fd`.
    AddSeccompFd = "--add-seccomp-fd", false;
    /// `--info-fd`.
    InfoFd = "--info-fd", false;
    /// `--json-status-fd`.
    JsonStatusFd = "--json-status-fd", true;
    /// `--new-session`.
    NewSession = "--new-session", true;
    /// `--die-with-parent`.
    DieWithParent = "--die-with-parent", true;
    /// `--perms`.
    Perms = "--perms", true;
    /// `--size`.
    Size = "--size", true;
    /// `--chmod`.
    Chmod = "--chmod", false;
    /// `--level-prefix`.
    LevelPrefix = "--level-prefix", false;
    /// `--disable-userns` (never emitted; recorded for the Doctor screen).
    DisableUserns = "--disable-userns", false;
}

impl fmt::Display for BwrapFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for BwrapFlag {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BwrapFlag {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        BwrapFlag::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown flag {s}")))
    }
}

/// What the probe learned about bubblewrap (MODULES §4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BwrapFacts {
    /// Absolute path of the binary used at launch (never looked up through `PATH` later).
    pub path: PathBuf,
    /// Parsed `bwrap --version`; `None` when unparsable.
    pub version: Option<ToolVersion>,
    /// Options listed by `bwrap --help`.
    pub flags: BTreeSet<BwrapFlag>,
    /// The binary is setuid root.
    pub setuid: bool,
    /// `bwrap --unshare-user --ro-bind / / true` succeeded.
    pub userns_ok: bool,
    /// A `bwrap` inside a `bwrap` could create a user namespace (pressure-vessel needs it).
    pub nested_userns_ok: bool,
    /// `--overlay-src` + `--tmp-overlay` worked.
    pub overlay_ok: bool,
}

impl BwrapFacts {
    /// Whether an option is supported.
    pub fn has(&self, flag: BwrapFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Required options missing from `flags`, in declaration order.
    pub fn missing_required(&self) -> Vec<BwrapFlag> {
        BwrapFlag::ALL
            .iter()
            .copied()
            .filter(|f| f.is_required() && !self.flags.contains(f))
            .collect()
    }
}

// ---- umu ------------------------------------------------------------------------------------

/// Minimum accepted umu-launcher version (UMU_INTEGRATION §1).
pub const UMU_MIN_VERSION: ToolVersion = ToolVersion::new(1, 4, 0);
/// The verified umu-launcher version.
pub const UMU_VERIFIED_VERSION: ToolVersion = ToolVersion::new(1, 4, 4);

/// What the probe learned about `umu-run` (MODULES §4, UMU_INTEGRATION §2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UmuFacts {
    /// Absolute path of the candidate as found (the program Zaun executes).
    pub path: PathBuf,
    /// Parsed `umu-run --version`; `None` when it could not be run or parsed.
    pub version: Option<ToolVersion>,
    /// Directories outside the system-bound roots that must be visible read-only at the
    /// identical path inside the sandbox, sorted.
    pub closure: Vec<PathBuf>,
    /// Set when `path` is a wrapper script that hands over to another program (Nix
    /// `makeWrapper`, nixpak, …) rather than umu's own Python entry point.
    pub wrapper: Option<UmuWrapper>,
}

/// A wrapper in front of the real `umu-run`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UmuWrapper {
    /// The programs visited by following `exec` lines and interpreters, starting with the
    /// real path of the candidate.
    pub chain: Vec<PathBuf>,
    /// The chain contains a sandbox launcher (bwrap, nixpak, flatpak, firejail). A sandbox
    /// inside Zaun's sandbox usually fails the nested-namespace step; the Doctor screen
    /// asks the user to point `paths.umu_run` at the unwrapped program.
    pub sandboxed: bool,
    /// The chain ends in umu's Python entry point.
    pub reaches_python: bool,
    /// The wrapper is nixpkgs' `buildFHSEnv` environment (`umu-launcher` on NixOS). It uses
    /// bwrap too, but works nested inside Zaun's sandbox: it re-binds the outer view and
    /// provides the FHS tree pressure-vessel needs (TECHNICAL_RESEARCH F-228), so it is not
    /// reported as a problem.
    #[serde(default)]
    pub fhs_env: bool,
}

impl UmuWrapper {
    /// A sandbox wrapper that usually fails inside Zaun's sandbox (nixpkgs' FHS
    /// environment is the known exception).
    pub fn breaks_nesting(&self) -> bool {
        self.sandboxed && !self.fhs_env
    }
}

impl UmuFacts {
    /// Version at least [`UMU_MIN_VERSION`].
    pub fn version_accepted(&self) -> bool {
        self.version.is_some_and(|v| v >= UMU_MIN_VERSION)
    }
}

// ---- filesystem layout ----------------------------------------------------------------------

/// The family of filesystem layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    /// Filesystem Hierarchy Standard (with or without merged `/usr`).
    #[default]
    Fhs,
    /// NixOS (`/nix/store`, `/run/current-system`).
    Nix,
    /// Guix System (`/gnu/store`).
    Guix,
}

/// One host path as seen with `lstat`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum HostEntry {
    /// A real directory.
    Dir,
    /// A regular file.
    File,
    /// A symbolic link.
    Symlink {
        /// The link text as stored (`readlink`).
        target: PathBuf,
        /// The canonical path it resolves to, when it resolves.
        resolved: Option<PathBuf>,
    },
    /// Anything else (device, socket, FIFO).
    Other,
}

impl HostEntry {
    /// A symlink's stored target.
    pub fn link_target(&self) -> Option<&Path> {
        match self {
            HostEntry::Symlink { target, .. } => Some(target),
            _ => None,
        }
    }
}

/// How `/etc/resolv.conf` is provided on the host (BUBBLEWRAP_SANDBOX §4.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ResolverStrategy {
    /// No `resolv.conf`.
    #[default]
    Absent,
    /// A regular file: bind it.
    File,
    /// A symlink: recreate it and bind the directory holding its target read-only.
    Symlink {
        /// Link text.
        target: PathBuf,
        /// Canonical directory containing the resolved target (e.g. `/run/systemd/resolve`).
        bind_dir: PathBuf,
    },
}

/// The host's directory layout (BUBBLEWRAP_SANDBOX §4.2–4.3, UMU_INTEGRATION §4 table B).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsLayout {
    /// Layout family.
    pub kind: LayoutKind,
    /// `/bin`, `/sbin`, `/lib`, `/lib32`, `/lib64`, `/libx32` as present on the host (absent
    /// entries are not in the map). A symlink means merged `/usr` for that entry.
    pub top_level: BTreeMap<PathBuf, HostEntry>,
    /// `/usr` is a directory.
    pub has_usr: bool,
    /// `/opt` exists.
    pub has_opt: bool,
    /// Library roots from `/etc/ld.so.conf*` outside `/usr`, `/lib*` and `/opt`, canonical,
    /// existing, sorted.
    pub extra_lib_roots: Vec<PathBuf>,
    /// Each `/etc` allow-list entry that exists on the host, by file name.
    pub etc: BTreeMap<String, HostEntry>,
    /// How DNS configuration is provided.
    pub resolver: ResolverStrategy,
    /// Layout-specific host variables to pass through (UMU_INTEGRATION §4 table B, last row);
    /// empty for FHS.
    pub passthrough_env: Vec<String>,
    /// `PATH` for the sandbox (table A).
    pub path_env: String,
    /// NixOS/Guix: optional trees present on the host (`/run/opengl-driver`,
    /// `/run/opengl-driver-32`, `/etc/static`, `/etc/profiles/per-user/<user>`,
    /// `/run/current-system`), canonical where they are symlinks.
    pub system_trees: BTreeMap<PathBuf, HostEntry>,
}

impl FsLayout {
    /// Whether `/<name>` is a merged-`/usr` symlink.
    pub fn is_merged(&self, top: &str) -> bool {
        matches!(
            self.top_level.get(Path::new(top)),
            Some(HostEntry::Symlink { .. })
        )
    }

    /// Roots that a system bind makes visible (used by `umu::locate` and the deny checks).
    pub fn system_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = ["/usr", "/opt", "/bin", "/sbin", "/lib", "/lib32"]
            .iter()
            .chain(["/lib64", "/libx32"].iter())
            .map(PathBuf::from)
            .collect();
        match self.kind {
            LayoutKind::Nix => roots.extend(["/nix", "/run/current-system"].map(PathBuf::from)),
            LayoutKind::Guix => roots.extend(["/gnu", "/run/current-system"].map(PathBuf::from)),
            LayoutKind::Fhs => {}
        }
        roots.extend(self.extra_lib_roots.iter().cloned());
        roots
    }
}

// ---- devices and sockets --------------------------------------------------------------------

/// A device node and whether the user can open it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceNode {
    /// Absolute path.
    pub path: PathBuf,
    /// `access(R_OK | W_OK)` (render nodes) or `access(R_OK)` (input).
    pub accessible: bool,
}

/// A GPU vendor from `/sys/class/drm/*/device/vendor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    /// `0x1002`.
    Amd,
    /// `0x8086`.
    Intel,
    /// `0x10de`.
    Nvidia,
    /// Anything else.
    Other,
}

/// GPU facts (BUBBLEWRAP_SANDBOX §4.5; Doctor D-12).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuFacts {
    /// `/dev/dri` exists.
    pub dri_present: bool,
    /// `/dev/dri/renderD*`, sorted.
    pub render_nodes: Vec<DeviceNode>,
    /// `/dev/dri/card*`, sorted.
    pub card_nodes: Vec<DeviceNode>,
    /// Present NVIDIA nodes (`nvidiactl`, `nvidia-modeset`, `nvidia-uvm`, `nvidia-uvm-tools`,
    /// `nvidia0…N`, `nvidia-caps`), sorted.
    pub nvidia_nodes: Vec<PathBuf>,
    /// `/dev/ntsync` exists.
    pub ntsync: bool,
    /// Vendors of the DRM devices, sorted, unique.
    pub vendors: Vec<GpuVendor>,
    /// A 32-bit graphics driver library is in the `ldconfig -p` cache; `None` when that
    /// could not be determined (e.g. NixOS, where drivers come from `/run/opengl-driver-32`).
    pub has_32bit_driver: Option<bool>,
}

/// The kind of graphical session (`XDG_SESSION_TYPE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    /// Wayland.
    Wayland,
    /// X11.
    X11,
    /// A text console.
    Tty,
    /// Unset or unrecognised.
    #[default]
    Unknown,
}

/// A Wayland compositor socket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaylandSocket {
    /// `WAYLAND_DISPLAY` (a name relative to `XDG_RUNTIME_DIR`).
    pub name: String,
    /// Absolute host path of the socket.
    pub path: PathBuf,
    /// Inode of the socket when it was probed (0: unknown). A restarted compositor makes a
    /// new socket, so revalidation notices that the facts below describe another one.
    #[serde(default)]
    pub inode: u64,
    /// The compositor offers `wp_security_context_v1` to Zaun: each session gets a
    /// restricted socket instead of this one (ADR-0029).
    #[serde(default)]
    pub security_context: bool,
}

/// A local X11 display (`DISPLAY=:n`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct X11Display {
    /// Display number (`:0` → 0).
    pub number: u32,
    /// `/tmp/.X11-unix/X<n>`.
    pub socket: PathBuf,
    /// The filesystem socket exists. The abstract socket `@/tmp/.X11-unix/X<n>` is scoped to
    /// the network namespace, so a sandbox with `--unshare-net` can reach the server only
    /// through this filesystem socket; without it X11 is unavailable to Offline games.
    pub filesystem_socket: bool,
    /// The authority file (`XAUTHORITY` or `~/.Xauthority`) when it exists.
    pub authority: Option<PathBuf>,
}

/// Display facts (BUBBLEWRAP_SANDBOX §4.7; Doctor D-11).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayFacts {
    /// Session type.
    pub session_type: SessionType,
    /// `XDG_CURRENT_DESKTOP`.
    pub desktop: Option<String>,
    /// The host's `XDG_RUNTIME_DIR`.
    pub xdg_runtime_dir: Option<PathBuf>,
    /// The Wayland socket, when it exists.
    pub wayland: Option<WaylandSocket>,
    /// The local X11 display named by `DISPLAY` (check `filesystem_socket` before binding).
    pub x11: Option<X11Display>,
}

impl DisplayFacts {
    /// The X11 display when its filesystem socket can be bound.
    pub fn x11_bindable(&self) -> Option<&X11Display> {
        self.x11.as_ref().filter(|x| x.filesystem_socket)
    }

    /// Whether any display socket can be provided (`ZN-HOST-020` otherwise).
    pub fn any(&self) -> bool {
        self.wayland.is_some() || self.x11_bindable().is_some()
    }
}

/// Sound facts (BUBBLEWRAP_SANDBOX §4.7; Doctor D-13).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFacts {
    /// `$XRD/pulse/native` when it exists (PipeWire's Pulse server provides it too).
    pub pulse_socket: Option<PathBuf>,
    /// `$XRD/pipewire-0` when it exists.
    pub pipewire_socket: Option<PathBuf>,
    /// `/dev/snd` exists.
    pub alsa_devices: bool,
    /// The `pipewire` program, which serves a session's sound server that refuses
    /// recording (ADR-0026).
    #[serde(default)]
    pub pipewire_program: Option<PathBuf>,
    /// The character devices directly under `/dev/snd`, sorted.
    #[serde(default)]
    pub snd_nodes: Vec<PathBuf>,
}

impl AudioFacts {
    /// Whether sound can be given without the microphone: the session's own sound server
    /// needs the `pipewire` program and the host's PipeWire socket to play through.
    pub fn playback_only(&self) -> bool {
        self.pipewire_program.is_some() && self.pipewire_socket.is_some()
    }
}

/// Whether `node` (a `/dev/snd` entry) records: ALSA names capture PCMs `pcmC<card>D<dev>c`.
pub fn is_capture_node(node: &Path) -> bool {
    node.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("pcmC") && n.ends_with('c'))
}

// ---- input ----------------------------------------------------------------------------------

/// The kind of input node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputNodeKind {
    /// `/dev/input/eventN`.
    Event,
    /// `/dev/input/jsN`.
    Js,
    /// `/dev/hidrawN`.
    Hidraw,
}

/// A device class from udev `ID_INPUT_*` properties or the sysfs capability fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputClass {
    /// `ID_INPUT_JOYSTICK`.
    Joystick,
    /// `ID_INPUT_KEYBOARD`.
    Keyboard,
    /// `ID_INPUT_KEY` (some keys, e.g. a power button or media keys).
    Key,
    /// `ID_INPUT_MOUSE`.
    Mouse,
    /// `ID_INPUT_TOUCHPAD`.
    Touchpad,
    /// `ID_INPUT_TOUCHSCREEN`.
    Touchscreen,
    /// `ID_INPUT_TABLET`.
    Tablet,
    /// `ID_INPUT_ACCELEROMETER`.
    Accelerometer,
    /// `ID_INPUT_SWITCH`.
    Switch,
}

impl InputClass {
    /// Classes that can observe typing or pointer movement (SECURITY_MODEL §5.4).
    pub const fn is_sensitive(self) -> bool {
        matches!(
            self,
            InputClass::Keyboard | InputClass::Mouse | InputClass::Touchpad
        )
    }
}

/// One input node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputNode {
    /// Absolute path.
    pub path: PathBuf,
    /// Node kind.
    pub kind: InputNodeKind,
    /// `(major, minor)` device numbers.
    pub dev: (u32, u32),
    /// Device classes (empty when unknown). For `hidraw`, the classes of the input devices
    /// that share its HID parent.
    pub classes: BTreeSet<InputClass>,
    /// The user can open it for reading.
    pub readable: bool,
    /// For `eventN` and `jsN` belonging to the same input device: the sibling node paths
    /// (so the planner binds `event21` together with `js0`).
    pub siblings: Vec<PathBuf>,
}

impl InputNode {
    /// Classified as a joystick or game pad.
    pub fn is_joystick(&self) -> bool {
        self.classes.contains(&InputClass::Joystick)
    }
}

/// Input facts (SECURITY_MODEL §5.4; Doctor D-14).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFacts {
    /// Every `event*`, `js*` and `hidraw*` node, sorted by path.
    pub nodes: Vec<InputNode>,
    /// Any keyboard-, mouse- or touchpad-class node that is not part of a game pad is readable
    /// by the user (for example because the user is in the `input` group). `AllInput` is then
    /// refused.
    pub keyboards_readable: bool,
    /// `/run/udev/data` exists (classification used udev rather than sysfs alone).
    pub udev_db: bool,
}

impl InputFacts {
    /// Readable joystick-class nodes (the `Detected` controller set).
    pub fn joysticks(&self) -> impl Iterator<Item = &InputNode> {
        self.nodes.iter().filter(|n| n.is_joystick() && n.readable)
    }
}

// ---- sysfs and store filesystem -------------------------------------------------------------

/// Sysfs paths for the masks of BUBBLEWRAP_SANDBOX §4.6.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysfsFacts {
    /// For each physical interface in `/sys/class/net`, the canonical `…/net` directory that
    /// contains it (e.g. `/sys/devices/pci0000:00/0000:00:1f.6/net`), sorted, unique. Virtual
    /// interfaces (under `/sys/devices/virtual/net`) are masked separately and not listed.
    pub net_device_dirs: Vec<PathBuf>,
}

/// The runtime store's filesystem (RUNTIME_MANAGEMENT §7).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreFsFacts {
    /// Device ID of the store directory; the reflink result is valid for this device only.
    pub store_device: Option<u64>,
    /// Filesystem type from mount info (`btrfs`, `ext4`, …).
    pub store_fs_type: Option<String>,
    /// `FICLONE` works between `STORE` and `STORE/session-var`; `None` when not probed.
    pub reflink_ok: Option<bool>,
}

// ---- problems -------------------------------------------------------------------------------

/// Distribution-specific guidance attached to a namespace failure (ERROR_HANDLING §4.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum HostHint {
    /// A sysctl that must be set, e.g. `kernel.unprivileged_userns_clone=1`.
    Sysctl {
        /// `name=value`.
        setting: String,
    },
    /// Ubuntu-style AppArmor restriction of unprivileged user namespaces.
    AppArmor {
        /// The sysctl that relaxes it globally (with the trade-off stated by the catalogue).
        sysctl: String,
        /// The profile to install instead (`PKG-006`).
        profile: String,
    },
    /// bwrap is setuid; overlays and some options are unavailable.
    SetuidBwrap,
    /// `umu-run` is itself a sandbox wrapper; point `paths.umu_run` at the real program.
    UmuWrapper,
}

/// A problem found by the probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProblem {
    /// Stable code (`ZN-HOST-*`).
    pub code: ErrorCode,
    /// Technical detail: the failing command, its stderr, the missing option.
    pub detail: String,
    /// Guidance chosen from the distribution.
    pub hint: Option<HostHint>,
}

impl HostProblem {
    /// A problem without a hint.
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            hint: None,
        }
    }
}
