//! Plan data types (MODULES §5, BUBBLEWRAP_SANDBOX §2 and §8) and the [`PlanBuilder`].
//!
//! A [`SandboxPlan`] is pure data: every namespace flag, mount, variable, descriptor and host
//! action of one sandbox. It serialises losslessly (paths that are not UTF-8 use the `b:`
//! escape form of DATA_MODEL §1).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::validate::PolicyIssue;
use crate::ids::GameId;
use crate::model::{SessionKind, SlrVariant};

/// Version of the plan's serialised form.
pub const SANDBOX_PLAN_FORMAT: u32 = 1;

/// Descriptor carrying the `--args` payload (BUBBLEWRAP_SANDBOX §3).
pub const FD_ARGS: i32 = 3;
/// `--json-status-fd`.
pub const FD_JSON_STATUS: i32 = 4;
/// `--sync-fd`.
pub const FD_SYNC: i32 = 5;
/// Init shim status pipe.
pub const FD_INIT_STATUS: i32 = 6;
/// `--seccomp` program.
pub const FD_SECCOMP: i32 = 7;
/// First descriptor of `FileData` payloads.
pub const FD_FIRST_DATA: i32 = 8;

/// Path of the init shim inside the sandbox.
pub const INIT_PATH: &str = "/run/easyplay/init";
/// In-sandbox directory for EasyPlay's own mounts.
pub const RUN_EASYPLAY: &str = "/run/easyplay";
/// Pressure-vessel variable directory inside the sandbox (Isolated mode).
pub const PV_VAR_PATH: &str = "/run/easyplay/pv-var";
/// Proton log directory inside the sandbox.
pub const PROTON_LOG_PATH: &str = "/run/easyplay/proton-log";
/// Throw-away prefix of Provision sessions.
pub const PROVISION_PREFIX: &str = "/tmp/easyplay-prefix";
/// `/tmp` size for trusted network sessions (BUBBLEWRAP_SANDBOX §4.7).
pub const SMALL_TMP_BYTES: u64 = 64 * 1024 * 1024;

/// The finished plan of one sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPlan {
    /// [`SANDBOX_PLAN_FORMAT`].
    pub format: u32,
    /// Session kind the plan was built for.
    pub kind: SessionKind,
    /// Namespaces and lifecycle options.
    pub namespaces: Namespaces,
    /// `--hostname` value, when masked.
    pub hostname: Option<String>,
    /// Mounts in emission order.
    pub mounts: Vec<Mount>,
    /// Environment.
    pub env: EnvPlan,
    /// Descriptor payloads (seccomp and `FileData` contents) in descriptor order.
    pub fds: Vec<FdPayload>,
    /// `--chdir`.
    #[serde(with = "codec::path")]
    pub chdir: PathBuf,
    /// In-sandbox command: `[init, "--status-fd", "6", "--", umu-run, args…]`.
    #[serde(with = "codec::os_vec")]
    pub command: Vec<OsString>,
    /// Work `session::spawn` completes on the host before `exec` (BUBBLEWRAP_SANDBOX §8).
    pub host_actions: Vec<HostAction>,
    /// Human-readable remarks ("PipeWire socket not found; …").
    pub notes: Vec<PlanNote>,
    /// Non-blocking issues (warnings) found while planning.
    pub issues: Vec<PolicyIssue>,
    /// SLR variant whose store must be audited after the session (Shared mode).
    pub audit_after: Option<SlrVariant>,
}

impl SandboxPlan {
    /// Mounts produced by `section`, with their indices.
    pub fn mounts_in(&self, section: Section) -> impl Iterator<Item = (usize, &Mount)> {
        self.mounts
            .iter()
            .enumerate()
            .filter(move |(_, m)| m.origin.section == section)
    }

    /// The first mount whose destination is `dst`.
    pub fn mount_at(&self, dst: &Path) -> Option<&Mount> {
        self.mounts.iter().find(|m| m.kind.dst() == dst)
    }
}

/// Namespaces and lifecycle options (BUBBLEWRAP_SANDBOX §4.1). The mount namespace is
/// implied by bubblewrap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Namespaces {
    /// `--unshare-user`.
    pub user: bool,
    /// `--unshare-ipc`.
    pub ipc: bool,
    /// `--unshare-pid`.
    pub pid: bool,
    /// `--unshare-uts`.
    pub uts: bool,
    /// `--unshare-cgroup-try`.
    pub cgroup_try: bool,
    /// `--unshare-net`.
    pub net: bool,
    /// `--die-with-parent`.
    pub die_with_parent: bool,
    /// `--new-session`.
    pub new_session: bool,
    /// `--json-status-fd`.
    pub json_status_fd: Option<i32>,
    /// `--sync-fd`.
    pub sync_fd: Option<i32>,
    /// `--seccomp`.
    pub seccomp_fd: Option<i32>,
}

/// Read-only or read-write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountMode {
    /// `--ro-bind`.
    Ro,
    /// `--bind`.
    Rw,
}

/// How a `FileData` payload is mounted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDataOp {
    /// `--ro-bind-data`: a read-only file.
    RoBindData,
    /// `--file`: a regular writable file (for files that are replaced by rename).
    File,
}

/// One planned mount: what and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    /// The operation.
    pub kind: MountKind,
    /// Which section emitted it and why.
    pub origin: MountOrigin,
}

/// A mount operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountKind {
    /// `--bind`, `--ro-bind` and their `-try` forms.
    Bind {
        /// Host path.
        #[serde(with = "codec::path")]
        src: PathBuf,
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
        /// Mode.
        mode: MountMode,
        /// `-try`: silently skipped when the source is missing.
        try_: bool,
    },
    /// `--dev-bind`, `--dev-bind-try`.
    DevBind {
        /// Host device path.
        #[serde(with = "codec::path")]
        src: PathBuf,
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
        /// `-try`.
        try_: bool,
    },
    /// `--tmpfs`, optionally preceded by `--perms` and `--size`.
    Tmpfs {
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
        /// `--size` in bytes.
        size: Option<u64>,
        /// `--perms`.
        perms: Option<u32>,
    },
    /// `--proc`.
    Proc {
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
    },
    /// `--dev`.
    Dev {
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
    },
    /// `--dir`, optionally preceded by `--perms`.
    Dir {
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
        /// `--perms`.
        perms: Option<u32>,
    },
    /// `--symlink`.
    Symlink {
        /// Link target, verbatim.
        #[serde(with = "codec::path")]
        target: PathBuf,
        /// Sandbox path of the link.
        #[serde(with = "codec::path")]
        dst: PathBuf,
    },
    /// `--ro-bind-data` / `--file` from a descriptor, preceded by `--perms`.
    FileData {
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
        /// Descriptor carrying the content (an entry of [`SandboxPlan::fds`]).
        fd: i32,
        /// `--perms`.
        perms: u32,
        /// Which option.
        op: FileDataOp,
    },
    /// `--tmp-overlay`: a writable overlay whose changes are discarded (reserved).
    OverlayTmp {
        /// Lower layer (`--overlay-src`).
        #[serde(with = "codec::path")]
        lower: PathBuf,
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
    },
    /// `--overlay-src <lower> --overlay <upper> <work> <dst>`.
    OverlayRw {
        /// Lower layer (the game folder).
        #[serde(with = "codec::path")]
        lower: PathBuf,
        /// Upper layer (changes).
        #[serde(with = "codec::path")]
        upper: PathBuf,
        /// Overlay work directory.
        #[serde(with = "codec::path")]
        work: PathBuf,
        /// Sandbox path.
        #[serde(with = "codec::path")]
        dst: PathBuf,
    },
}

impl MountKind {
    /// Sandbox destination.
    pub fn dst(&self) -> &Path {
        match self {
            MountKind::Bind { dst, .. }
            | MountKind::DevBind { dst, .. }
            | MountKind::Tmpfs { dst, .. }
            | MountKind::Proc { dst }
            | MountKind::Dev { dst }
            | MountKind::Dir { dst, .. }
            | MountKind::Symlink { dst, .. }
            | MountKind::FileData { dst, .. }
            | MountKind::OverlayTmp { dst, .. }
            | MountKind::OverlayRw { dst, .. } => dst,
        }
    }

    /// Host source of a bind or overlay (the lower layer for overlays).
    pub fn src(&self) -> Option<&Path> {
        match self {
            MountKind::Bind { src, .. } | MountKind::DevBind { src, .. } => Some(src),
            MountKind::OverlayTmp { lower, .. } | MountKind::OverlayRw { lower, .. } => Some(lower),
            _ => None,
        }
    }

    /// Whether the sandbox can write through this mount to host storage.
    pub fn writes_host(&self) -> bool {
        matches!(
            self,
            MountKind::Bind {
                mode: MountMode::Rw,
                ..
            } | MountKind::DevBind { .. }
        )
    }

    /// Whether the mount replaces what is at its destination (a mount, not just a directory
    /// or a link), and therefore hides earlier mounts below it.
    pub fn covers(&self) -> bool {
        !matches!(self, MountKind::Dir { .. } | MountKind::Symlink { .. })
    }

    /// Whether this is a bind of a host path (including device binds and overlays).
    pub fn is_host_bind(&self) -> bool {
        self.src().is_some()
    }

    /// Short kind name for tables (`bind`, `ro-bind`, `dev-bind`, `tmpfs`, …).
    pub fn label(&self) -> &'static str {
        match self {
            MountKind::Bind {
                mode: MountMode::Rw,
                ..
            } => "bind",
            MountKind::Bind { .. } => "ro-bind",
            MountKind::DevBind { .. } => "dev-bind",
            MountKind::Tmpfs { .. } => "tmpfs",
            MountKind::Proc { .. } => "proc",
            MountKind::Dev { .. } => "dev",
            MountKind::Dir { .. } => "dir",
            MountKind::Symlink { .. } => "symlink",
            MountKind::FileData { .. } => "file",
            MountKind::OverlayTmp { .. } | MountKind::OverlayRw { .. } => "overlay",
        }
    }
}

/// Planner section, in emission order (BUBBLEWRAP_SANDBOX §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// §4.1 namespaces and lifecycle.
    Namespaces,
    /// §4.2 system directories.
    System,
    /// §4.3 `/etc`.
    Etc,
    /// §4.4–4.5 `/proc` and `/dev`.
    Devices,
    /// §4.6 `/sys`.
    Sysfs,
    /// §4.7 `/tmp`, `/run`, sockets.
    TmpRun,
    /// §4.8 home.
    Home,
    /// §4.9–4.10 runtime store and Proton.
    RuntimeStore,
    /// §4.11 prefix and save data.
    Prefix,
    /// §4.12 game folder.
    Game,
    /// §4.13 grants and removable media.
    Grants,
    /// §4.14 umu closure and the init shim.
    Umu,
    /// §4.15 environment.
    Env,
    /// §4.16 working directory and command.
    Command,
}

impl Section {
    /// Every section in order.
    pub const ALL: [Section; 14] = [
        Section::Namespaces,
        Section::System,
        Section::Etc,
        Section::Devices,
        Section::Sysfs,
        Section::TmpRun,
        Section::Home,
        Section::RuntimeStore,
        Section::Prefix,
        Section::Game,
        Section::Grants,
        Section::Umu,
        Section::Env,
        Section::Command,
    ];
}

/// The policy field a mount or variable derives from (DEVELOPER_MODE §8).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyField {
    /// `network`.
    Network,
    /// `game_dir`.
    GameDir,
    /// One entry of `folders`, by canonical path.
    Folder(#[serde(with = "codec::path")] PathBuf),
    /// `save_data`.
    SaveData,
    /// `removable`.
    Removable,
    /// `devices.audio`.
    Audio,
    /// `devices.audio` `Full`: what records.
    Microphone,
    /// `devices.alsa_devices`.
    AlsaDevices,
    /// `devices.controllers`.
    Controllers,
    /// `devices.camera`.
    Camera,
    /// `display.wayland`.
    Wayland,
    /// `display.x11`.
    X11,
    /// `hardening.seccomp`.
    Seccomp,
    /// `hardening.mask_hostname`.
    MaskHostname,
    /// `hardening.mask_machine_id`.
    MaskMachineId,
    /// `hardening.mask_hardware_ids`.
    MaskHardwareIds,
    /// `hardening.runtime_isolation`.
    RuntimeIsolation,
    /// An `ExtraReadOnlyPath` exception, by canonical path.
    ExtraReadOnlyPath(#[serde(with = "codec::path")] PathBuf),
    /// An `ExtraDevice` exception, by path.
    ExtraDevice(#[serde(with = "codec::path")] PathBuf),
    /// A `PassEnv` exception, by name.
    PassEnv(String),
}

/// Why a mount exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountOrigin {
    /// The section that emitted it.
    pub section: Section,
    /// Short reason for the inspector.
    pub reason: Cow<'static, str>,
    /// The policy field it implements, if any.
    pub policy_field: Option<PolicyField>,
}

/// Where an environment variable comes from (DEVELOPER_MODE §3 *Environment*).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvOrigin {
    /// Set by the planner (UMU_INTEGRATION §4 table A).
    Fixed,
    /// Copied from the host (table B).
    Host,
    /// A compatibility option (table C), by option name.
    Option(String),
    /// A global user variable.
    Global,
    /// A per-game user variable.
    Game,
    /// A `PassEnv` exception.
    Exception,
}

/// One variable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    /// Value.
    pub value: String,
    /// Origin.
    pub origin: EnvOrigin,
}

/// The sandbox environment (BUBBLEWRAP_SANDBOX §4.15).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvPlan {
    /// `--clearenv` (always true).
    pub clear: bool,
    /// `--setenv`, sorted by name.
    pub set: BTreeMap<String, EnvEntry>,
    /// `--unsetenv` (unused while `clear` is set; kept for the format).
    pub unset: Vec<String>,
}

impl EnvPlan {
    /// The value of `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.set.get(name).map(|e| e.value.as_str())
    }
}

/// What a descriptor carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FdContent {
    /// Literal bytes (`FileData`).
    Bytes(Vec<u8>),
    /// The seccomp program, built by `sandbox::seccomp` when the descriptor is created.
    SeccompProgram,
}

/// One descriptor payload (BUBBLEWRAP_SANDBOX §3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FdPayload {
    /// Descriptor number in the child.
    pub fd: i32,
    /// For `FileData`: the sandbox path it is mounted at.
    #[serde(with = "codec::opt_path")]
    pub target: Option<PathBuf>,
    /// Content.
    pub content: FdContent,
}

/// A drive letter link in `<prefix>/dosdevices` (V-006).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DosDeviceLink {
    /// `d`..=`y`.
    pub letter: char,
    /// Absolute target path.
    #[serde(with = "codec::path")]
    pub target: PathBuf,
}

/// Work completed on the host before `exec`, each idempotent (BUBBLEWRAP_SANDBOX §8).
///
/// Executor contract (`session::spawn`): actions run in order; `CreateDir` uses `fsx` without
/// following symlinks and refuses an existing path that is not a directory owned by the
/// user; `AcquireLock` fails the launch when the lock is held; `WriteMemfd` creates a sealed
/// memfd from the plan's [`FdPayload`] with that number; `EnsureDosDevices` replaces exactly
/// the listed links; `MarkTainted` sets `games.prefix_tainted_at` once. Dry runs carry the
/// actions but never execute them (DEVELOPER_MODE §3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAction {
    /// Create a directory (and missing parents) with `mode`.
    CreateDir {
        /// Path.
        #[serde(with = "codec::path")]
        path: PathBuf,
        /// Mode bits.
        mode: u32,
    },
    /// Take the per-prefix lock.
    AcquireLock {
        /// Lock file.
        #[serde(with = "codec::path")]
        path: PathBuf,
    },
    /// Create the descriptor payload `fd` (`--args`, `--file`, `--ro-bind-data`, `--seccomp`).
    WriteMemfd {
        /// Descriptor number.
        fd: i32,
    },
    /// Maintain drive-letter links for grants.
    EnsureDosDevices {
        /// Prefix.
        #[serde(with = "codec::path")]
        prefix: PathBuf,
        /// Links.
        links: Vec<DosDeviceLink>,
    },
    /// Set `prefix_tainted_at` before spawning untrusted code.
    MarkTainted {
        /// The game.
        game: GameId,
    },
    /// Start the session's sound server, which plays and refuses recording (ADR-0026), and
    /// wait for its socket, [`sound_socket`]`(dir)`. The supervisor stops it when the
    /// session ends.
    StartSoundServer {
        /// The `pipewire` program.
        #[serde(with = "codec::path")]
        program: PathBuf,
        /// Its directory, created first: the configuration and the socket.
        #[serde(with = "codec::path")]
        dir: PathBuf,
        /// The host's PipeWire socket it plays through.
        #[serde(with = "codec::path")]
        remote: PathBuf,
        /// Where its output goes (in the session's log directory).
        #[serde(with = "codec::path")]
        log: PathBuf,
    },
    /// Create the session's restricted Wayland socket (ADR-0029): a listening socket at
    /// `socket` that the compositor at `display` serves with a security context, so it can
    /// withhold its privileged protocols from the game. The supervisor does this after the
    /// others, with the read end of the `--sync-fd` pipe as the context's close descriptor:
    /// the compositor stops serving the socket when the sandbox is gone.
    CreateWaylandContext {
        /// The compositor's socket.
        #[serde(with = "codec::path")]
        display: PathBuf,
        /// The restricted socket, in the session's runtime directory.
        #[serde(with = "codec::path")]
        socket: PathBuf,
        /// What the compositor sees as the application: the game's ID (the session kind
        /// when there is no game).
        app_id: String,
        /// What the compositor sees as the instance: the session's ID.
        instance_id: String,
    },
}

/// Longest path a Unix socket address holds (`sun_path` less its terminating zero).
pub const SOCKET_PATH_MAX: usize = 107;

/// The socket of the sound server started in `dir` ([`HostAction::StartSoundServer`]).
pub fn sound_socket(dir: &Path) -> PathBuf {
    dir.join("native")
}

/// A human-readable planner remark.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanNote {
    /// Stable message key, e.g. `note.audio.no_pipewire`.
    pub key: Cow<'static, str>,
    /// English text.
    pub text: String,
}

// ---- builder -------------------------------------------------------------------------

/// Error from [`PlanBuilder`] misuse (a planner bug).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuilderError {
    /// A section was started after a later one.
    #[error("section {next:?} started after {current:?}")]
    OutOfOrder {
        /// The section in progress.
        current: Section,
        /// The section requested.
        next: Section,
    },
    /// An entry was added before any section was started.
    #[error("entry added outside a section")]
    NoSection,
}

/// Accumulates plan entries section by section, enforcing the order of §4.
#[derive(Debug)]
pub struct PlanBuilder {
    current: Option<Section>,
    mounts: Vec<Mount>,
    env: BTreeMap<String, EnvEntry>,
    fds: Vec<FdPayload>,
    next_fd: i32,
    actions: Vec<HostAction>,
    notes: Vec<PlanNote>,
    issues: Vec<PolicyIssue>,
    error: Option<BuilderError>,
}

impl Default for PlanBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self {
            current: None,
            mounts: Vec::new(),
            env: BTreeMap::new(),
            fds: Vec::new(),
            next_fd: FD_FIRST_DATA,
            actions: Vec::new(),
            notes: Vec::new(),
            issues: Vec::new(),
            error: None,
        }
    }

    /// Start `section`. Sections must be started in increasing order.
    pub fn begin(&mut self, section: Section) -> Result<(), BuilderError> {
        if let Some(current) = self.current
            && section <= current
        {
            let e = BuilderError::OutOfOrder {
                current,
                next: section,
            };
            self.error.get_or_insert(e.clone());
            return Err(e);
        }
        self.current = Some(section);
        Ok(())
    }

    /// The section in progress.
    pub fn section(&self) -> Option<Section> {
        self.current
    }

    fn origin(&mut self, reason: Cow<'static, str>, field: Option<PolicyField>) -> MountOrigin {
        let section = match self.current {
            Some(s) => s,
            None => {
                self.error.get_or_insert(BuilderError::NoSection);
                Section::Namespaces
            }
        };
        MountOrigin {
            section,
            reason,
            policy_field: field,
        }
    }

    /// Add a mount.
    pub fn mount(
        &mut self,
        kind: MountKind,
        reason: impl Into<Cow<'static, str>>,
        field: Option<PolicyField>,
    ) {
        let origin = self.origin(reason.into(), field);
        self.mounts.push(Mount { kind, origin });
    }

    /// Add a `FileData` mount with `content`; allocates the next descriptor.
    pub fn file_data(
        &mut self,
        dst: PathBuf,
        content: Vec<u8>,
        perms: u32,
        op: FileDataOp,
        reason: impl Into<Cow<'static, str>>,
        field: Option<PolicyField>,
    ) {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.fds.push(FdPayload {
            fd,
            target: Some(dst.clone()),
            content: FdContent::Bytes(content),
        });
        self.mount(MountKind::FileData { dst, fd, perms, op }, reason, field);
    }

    /// Set a variable (a later call for the same name replaces the value).
    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>, origin: EnvOrigin) {
        self.env.insert(
            name.into(),
            EnvEntry {
                value: value.into(),
                origin,
            },
        );
    }

    /// The value of a variable set so far.
    pub fn env_value(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(|e| e.value.as_str())
    }

    /// Remove a variable set so far.
    pub fn env_remove(&mut self, name: &str) {
        self.env.remove(name);
    }

    /// Add a host action (duplicates are dropped).
    pub fn action(&mut self, action: HostAction) {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
    }

    /// Add a note.
    pub fn note(&mut self, key: &'static str, text: impl Into<String>) {
        self.notes.push(PlanNote {
            key: Cow::Borrowed(key),
            text: text.into(),
        });
    }

    /// Record an issue.
    pub fn issue(&mut self, issue: PolicyIssue) {
        self.issues.push(issue);
    }

    /// Issues recorded so far.
    pub fn issues(&self) -> &[PolicyIssue] {
        &self.issues
    }

    /// Mounts emitted so far.
    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    /// The first builder misuse, if any.
    pub fn misuse(&self) -> Option<&BuilderError> {
        self.error.as_ref()
    }

    /// Finish into the parts the planner assembles: mounts, env, fds, actions, notes, issues.
    pub fn finish(self) -> BuiltParts {
        BuiltParts {
            mounts: self.mounts,
            env: self.env,
            fds: self.fds,
            actions: self.actions,
            notes: self.notes,
            issues: self.issues,
            error: self.error,
        }
    }
}

/// What [`PlanBuilder::finish`] returns.
#[derive(Debug)]
pub struct BuiltParts {
    /// Mounts.
    pub mounts: Vec<Mount>,
    /// Environment.
    pub env: BTreeMap<String, EnvEntry>,
    /// `FileData` payloads.
    pub fds: Vec<FdPayload>,
    /// Host actions.
    pub actions: Vec<HostAction>,
    /// Notes.
    pub notes: Vec<PlanNote>,
    /// Issues.
    pub issues: Vec<PolicyIssue>,
    /// Builder misuse.
    pub error: Option<BuilderError>,
}

// ---- path codec ----------------------------------------------------------------------

/// Lossless text form of paths and OS strings (DATA_MODEL §1): valid UTF-8 not starting with
/// `b:` is stored verbatim; anything else as `b:` followed by the bytes with `\xNN` escapes
/// for non-printable, non-ASCII and backslash bytes.
pub mod codec {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    /// Encode.
    pub fn encode(s: &OsStr) -> String {
        if let Some(text) = s.to_str()
            && !text.starts_with("b:")
        {
            return text.to_owned();
        }
        let mut out = String::from("b:");
        for &b in s.as_bytes() {
            if b.is_ascii_graphic() && b != b'\\' || b == b' ' {
                out.push(char::from(b));
            } else {
                out.push_str(&format!("\\x{b:02x}"));
            }
        }
        out
    }

    /// Decode [`encode`] output.
    pub fn decode(s: &str) -> Result<OsString, String> {
        let Some(rest) = s.strip_prefix("b:") else {
            return Ok(OsString::from(s));
        };
        let bytes = rest.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                let hex = bytes
                    .get(i + 2..i + 4)
                    .filter(|_| bytes.get(i + 1) == Some(&b'x'))
                    .ok_or_else(|| format!("bad escape in {s:?}"))?;
                let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                out.push(u8::from_str_radix(hex, 16).map_err(|e| e.to_string())?);
                i += 4;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        Ok(OsString::from_vec(out))
    }

    /// `#[serde(with)]` for `PathBuf`.
    pub mod path {
        use std::path::PathBuf;

        use serde::{Deserialize, Deserializer, Serializer};

        /// Serialise.
        #[allow(clippy::ptr_arg)] // signature required by `#[serde(with)]`
        pub fn serialize<S: Serializer>(p: &PathBuf, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&super::encode(p.as_os_str()))
        }

        /// Deserialise.
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
            let text = String::deserialize(d)?;
            super::decode(&text)
                .map(PathBuf::from)
                .map_err(serde::de::Error::custom)
        }
    }

    /// `#[serde(with)]` for `Option<PathBuf>`.
    pub mod opt_path {
        use std::path::PathBuf;

        use serde::{Deserialize, Deserializer, Serializer};

        /// Serialise.
        pub fn serialize<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
            match p {
                Some(p) => s.serialize_some(&super::encode(p.as_os_str())),
                None => s.serialize_none(),
            }
        }

        /// Deserialise.
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
            let text = Option::<String>::deserialize(d)?;
            text.map(|t| super::decode(&t).map(PathBuf::from))
                .transpose()
                .map_err(serde::de::Error::custom)
        }
    }

    /// `#[serde(with)]` for `Vec<OsString>`.
    pub mod os_vec {
        use std::ffi::OsString;

        use serde::ser::SerializeSeq;
        use serde::{Deserialize, Deserializer, Serializer};

        /// Serialise.
        pub fn serialize<S: Serializer>(v: &[OsString], s: S) -> Result<S::Ok, S::Error> {
            let mut seq = s.serialize_seq(Some(v.len()))?;
            for item in v {
                seq.serialize_element(&super::encode(item))?;
            }
            seq.end()
        }

        /// Deserialise.
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<OsString>, D::Error> {
            let items = Vec::<String>::deserialize(d)?;
            items
                .iter()
                .map(|t| super::decode(t))
                .collect::<Result<_, _>>()
                .map_err(serde::de::Error::custom)
        }
    }
}
