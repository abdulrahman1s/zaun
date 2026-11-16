//! `explain(plan, policy)`: the plain-language access statements derived from the plan that
//! is actually executed (SECURITY_MODEL §10, SCREEN_SPECS §6, DEVELOPER_MODE §3).
//!
//! Statements are generated from the plan's mounts grouped by their `policy_field`, so every
//! policy-derived mount is covered by exactly one statement; flag-only statements name the
//! option that implements them. Each statement lists its mechanisms (mount indices, flags,
//! variables) for the inspector's *Access* tab.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan::{MountKind, MountMode, PolicyField, SandboxPlan, Section};
use super::validate::IssueCode;
use crate::model::{GameDirAccess, GrantOrigin, SandboxPolicy, UserDir};

/// Category, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessCategory {
    /// Internet and local network.
    Network,
    /// The game's own storage and folder.
    OwnFolder,
    /// The user's folders.
    YourFolders,
    /// Devices.
    Devices,
    /// Display.
    Display,
    /// Host identity.
    Identity,
    /// Weakened protections.
    HardeningChanges,
    /// Developer Mode exceptions.
    Exceptions,
}

/// How a statement is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementSeverity {
    /// Neutral or reassuring.
    Info,
    /// Access worth knowing about.
    Notice,
    /// A weaker protection.
    Warning,
    /// Exposes something on the deny-list (shown in red).
    Danger,
}

/// One access statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessStatement {
    /// Category.
    pub category: AccessCategory,
    /// Message key, e.g. `access.network.none`.
    pub key: String,
    /// Message arguments (`name`, `path`, `count`).
    pub args: BTreeMap<String, String>,
    /// English text.
    pub text: String,
    /// Optional detail line.
    pub detail: Option<String>,
    /// Severity.
    pub severity: StatementSeverity,
    /// Indices into `plan.mounts` that implement the statement.
    pub mounts: Vec<usize>,
    /// Options or namespace facts that implement it (`--unshare-net`).
    pub flags: Vec<String>,
    /// Variables that implement it.
    pub env: Vec<String>,
}

impl AccessStatement {
    fn new(
        category: AccessCategory,
        key: &str,
        text: impl Into<String>,
        severity: StatementSeverity,
    ) -> Self {
        Self {
            category,
            key: key.to_owned(),
            args: BTreeMap::new(),
            text: text.into(),
            detail: None,
            severity,
            mounts: Vec::new(),
            flags: Vec::new(),
            env: Vec::new(),
        }
    }

    fn arg(mut self, k: &str, v: impl Into<String>) -> Self {
        self.args.insert(k.to_owned(), v.into());
        self
    }

    fn mounts(mut self, m: Vec<usize>) -> Self {
        self.mounts = m;
        self
    }

    fn flag(mut self, f: &str) -> Self {
        self.flags.push(f.to_owned());
        self
    }

    fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }

    /// Whether the statement is backed by at least one mechanism.
    pub fn has_mechanism(&self) -> bool {
        !(self.mounts.is_empty() && self.flags.is_empty() && self.env.is_empty())
    }
}

fn display_name(path: &Path, origin: Option<GrantOrigin>) -> String {
    match origin {
        Some(GrantOrigin::WellKnown(d)) => match d {
            UserDir::Desktop => "Desktop",
            UserDir::Documents => "Documents",
            UserDir::Downloads => "Downloads",
            UserDir::Music => "Music",
            UserDir::Pictures => "Pictures",
            UserDir::Videos => "Videos",
        }
        .to_owned(),
        _ => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

/// Explain a plan.
pub fn explain(plan: &SandboxPlan, policy: &SandboxPolicy) -> Vec<AccessStatement> {
    use AccessCategory as C;
    use StatementSeverity as S;
    let mut groups: BTreeMap<PolicyField, Vec<usize>> = BTreeMap::new();
    for (i, m) in plan.mounts.iter().enumerate() {
        if let Some(f) = &m.origin.policy_field {
            groups.entry(f.clone()).or_default().push(i);
        }
    }
    let mut out: Vec<AccessStatement> = Vec::new();
    let ns = &plan.namespaces;

    // Network.
    let net_mounts = groups.remove(&PolicyField::Network).unwrap_or_default();
    if ns.net {
        out.push(
            AccessStatement::new(
                C::Network,
                "access.network.none",
                "No internet or local network",
                S::Info,
            )
            .mounts(net_mounts)
            .flag("--unshare-net"),
        );
    } else {
        out.push(
            AccessStatement::new(
                C::Network,
                "access.network.full",
                "Can use the internet and your local network",
                S::Notice,
            )
            .mounts(net_mounts)
            .flag("host network namespace"),
        );
    }

    // Own folder.
    let own: Vec<usize> = plan
        .mounts
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.origin.policy_field.is_none()
                && (m.origin.section == Section::Prefix
                    || (m.origin.section == Section::Home
                        && matches!(
                            m.kind,
                            MountKind::Bind {
                                mode: MountMode::Rw,
                                ..
                            } | MountKind::Tmpfs { .. }
                        )))
        })
        .map(|(i, _)| i)
        .collect();
    let home_mount: Vec<usize> = own
        .iter()
        .copied()
        .filter(|i| plan.mounts[*i].origin.section == Section::Home)
        .collect();
    out.push(
        AccessStatement::new(
            C::OwnFolder,
            "access.own.private_storage",
            "Has its own private storage",
            S::Info,
        )
        .mounts(own),
    );
    if let Some(m) = groups.remove(&PolicyField::GameDir) {
        let (key, text) = match policy.game_dir {
            GameDirAccess::FileOnly => ("access.own.file_only", "Can read only its program file"),
            GameDirAccess::ReadOnly => ("access.own.read_only", "Can read its game folder"),
            GameDirAccess::ReadWrite => (
                "access.own.read_write",
                "Can read and change its game folder",
            ),
            GameDirAccess::Protected => (
                "access.own.protected",
                "Can change its game folder; changes are kept separately",
            ),
        };
        let path = m
            .first()
            .map(|i| plan.mounts[*i].kind.dst().display().to_string())
            .unwrap_or_default();
        out.push(
            AccessStatement::new(C::OwnFolder, key, text, S::Info)
                .mounts(m)
                .arg("path", path),
        );
    }

    // Your folders.
    let mut personal = false;
    let folder_fields: Vec<PolicyField> = groups
        .keys()
        .filter(|f| matches!(f, PolicyField::Folder(_)))
        .cloned()
        .collect();
    for f in folder_fields {
        let PolicyField::Folder(path) = &f else {
            continue;
        };
        let m = groups.remove(&f).unwrap_or_default();
        let grant = policy.folders.iter().find(|g| &g.canonical == path);
        let rw = m.iter().any(|i| plan.mounts[*i].kind.writes_host());
        let name = display_name(path, grant.map(|g| g.origin));
        let (key, text) = if rw {
            (
                "access.folders.read_write",
                format!("Can read and change {name}"),
            )
        } else {
            ("access.folders.read_only", format!("Can read {name}"))
        };
        personal = true;
        out.push(
            AccessStatement::new(C::YourFolders, key, text, S::Notice)
                .mounts(m)
                .arg("name", name)
                .arg("path", path.display().to_string()),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::SaveData) {
        personal = true;
        let path = m
            .first()
            .and_then(|i| plan.mounts[*i].kind.src())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        out.push(
            AccessStatement::new(
                C::YourFolders,
                "access.folders.save_data",
                "Keeps its save data in your folder",
                S::Notice,
            )
            .mounts(m)
            .arg("path", path),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Removable) {
        personal = true;
        let rw = m.iter().any(|i| plan.mounts[*i].kind.writes_host());
        let (key, text) = if rw {
            (
                "access.folders.removable_rw",
                "Can read and change removable drives",
            )
        } else {
            ("access.folders.removable_ro", "Can read removable drives")
        };
        out.push(AccessStatement::new(C::YourFolders, key, text, S::Notice).mounts(m));
    }
    let exceptions_expose = groups
        .keys()
        .any(|f| matches!(f, PolicyField::ExtraReadOnlyPath(_)));
    if !personal && !exceptions_expose && !home_mount.is_empty() {
        out.push(
            AccessStatement::new(
                C::YourFolders,
                "access.folders.none",
                "Cannot see your personal files",
                S::Info,
            )
            .mounts(home_mount),
        );
    }

    // Devices.
    let sound = groups.remove(&PolicyField::Audio);
    let microphone = groups.remove(&PolicyField::Microphone);
    if sound.is_some() || microphone.is_some() {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.sound",
                "Can play sound",
                S::Info,
            )
            .mounts(sound.unwrap_or_default()),
        );
    }
    if let Some(m) = microphone {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.microphone",
                "Can use your microphone",
                S::Warning,
            )
            .mounts(m),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Controllers) {
        let all = m
            .iter()
            .any(|i| plan.mounts[*i].kind.dst() == Path::new("/dev/input"));
        let st = if all {
            AccessStatement::new(
                C::Devices,
                "access.devices.all_input",
                "Can use all game controllers, including ones connected later",
                S::Notice,
            )
        } else {
            let count = m
                .iter()
                .filter(|i| plan.mounts[**i].kind.dst().starts_with("/dev/input"))
                .count();
            AccessStatement::new(
                C::Devices,
                "access.devices.controllers",
                "Can use the game controllers connected now",
                S::Info,
            )
            .arg("count", count.to_string())
        };
        out.push(st.mounts(m));
    }
    if let Some(m) = groups.remove(&PolicyField::AlsaDevices) {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.alsa",
                "Can use sound devices directly",
                S::Notice,
            )
            .mounts(m),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Camera) {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.camera",
                "Can use your cameras",
                S::Warning,
            )
            .mounts(m),
        );
    }

    // Display.
    if let Some(m) = groups.remove(&PolicyField::Wayland) {
        out.push(
            AccessStatement::new(
                C::Display,
                "access.display.wayland",
                "Shows its windows on your desktop",
                S::Info,
            )
            .mounts(m),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::X11) {
        let mut st = AccessStatement::new(
            C::Display,
            "access.display.x11",
            "Uses the X11 display: it can see and control other X11 windows",
            S::Warning,
        )
        .mounts(m);
        if !ns.ipc {
            st = st.flag("shared IPC namespace (X11 shared memory)");
        }
        out.push(st);
    }

    // Identity.
    if let Some(m) = groups.remove(&PolicyField::MaskMachineId) {
        let masked = m
            .iter()
            .any(|i| matches!(plan.mounts[*i].kind, MountKind::FileData { .. }));
        out.push(
            if masked {
                AccessStatement::new(
                    C::Identity,
                    "access.identity.machine_id_masked",
                    "Sees a stand-in machine ID",
                    S::Info,
                )
            } else {
                AccessStatement::new(
                    C::Identity,
                    "access.identity.machine_id_real",
                    "Sees this computer's machine ID",
                    S::Notice,
                )
            }
            .mounts(m),
        );
    }
    if plan.hostname.is_some() {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hostname_masked",
                "Sees the computer name \"easyplay\"",
                S::Info,
            )
            .flag("--hostname"),
        );
    } else {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hostname_real",
                "Sees this computer's name",
                S::Notice,
            )
            .flag("--hostname absent"),
        );
    }
    let hw_masks = groups.remove(&PolicyField::MaskHardwareIds);
    let dmi_masked = hw_masks.as_ref().is_some_and(|m| {
        m.iter()
            .any(|i| plan.mounts[*i].kind.dst() == Path::new("/sys/devices/virtual/dmi"))
    });
    if dmi_masked {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hardware_masked",
                "Cannot read hardware identifiers",
                S::Info,
            )
            .mounts(hw_masks.unwrap_or_default()),
        );
    } else {
        let sys: Vec<usize> = plan
            .mounts_in(Section::Sysfs)
            .filter(|(_, m)| m.kind.is_host_bind())
            .map(|(i, _)| i)
            .chain(hw_masks.unwrap_or_default())
            .collect();
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hardware_visible",
                "Can read hardware identifiers",
                S::Notice,
            )
            .mounts(sys),
        );
    }

    // Hardening changes.
    if ns.seccomp_fd.is_none() {
        let st = if policy.hardening.seccomp == crate::model::SeccompMode::Off {
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.seccomp_off",
                "System-call filter is off",
                S::Warning,
            )
        } else {
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.seccomp_unavailable",
                "System-call filter is not available",
                S::Warning,
            )
        };
        out.push(st.flag("--seccomp absent"));
    }
    if plan.audit_after.is_some() {
        let m: Vec<usize> = plan
            .mounts_in(Section::RuntimeStore)
            .filter(|(_, m)| m.kind.writes_host())
            .map(|(i, _)| i)
            .collect();
        out.push(
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.runtime_shared",
                "Shares the compatibility layer with other games; it is checked after each session",
                S::Warning,
            )
            .mounts(m),
        );
    }

    // Exceptions (and anything left).
    let bypassed: Vec<&PathBuf> = plan
        .issues
        .iter()
        .filter(|i| i.code == IssueCode::PathDenyBypassed)
        .filter_map(|i| i.path.as_ref())
        .collect();
    for (field, m) in groups {
        let st = match &field {
            PolicyField::ExtraReadOnlyPath(p) => {
                let danger = bypassed.contains(&p);
                AccessStatement::new(
                    C::Exceptions,
                    "access.exceptions.read_path",
                    format!("Can read {}", p.display()),
                    if danger { S::Danger } else { S::Warning },
                )
                .arg("path", p.display().to_string())
                .detail(reason_of(policy, &field))
            }
            PolicyField::ExtraDevice(p) => AccessStatement::new(
                C::Exceptions,
                "access.exceptions.device",
                format!("Has direct access to the device {}", p.display()),
                S::Warning,
            )
            .arg("path", p.display().to_string())
            .detail(reason_of(policy, &field)),
            other => AccessStatement::new(
                C::Exceptions,
                "access.other",
                format!("{other:?}"),
                S::Notice,
            ),
        };
        out.push(st.mounts(m));
    }
    for e in &policy.exceptions {
        if let crate::model::PolicyException::PassEnv { name, reason } = e
            && plan.env.set.contains_key(name)
        {
            let mut st = AccessStatement::new(
                C::Exceptions,
                "access.exceptions.pass_env",
                format!("Receives the value of {name} from your session"),
                S::Warning,
            )
            .arg("name", name.clone())
            .detail(reason.clone());
            st.env.push(name.clone());
            out.push(st);
        }
    }
    out.sort_by_key(|s| s.category);
    out
}

fn reason_of(policy: &SandboxPolicy, field: &PolicyField) -> String {
    use crate::model::PolicyException as E;
    policy
        .exceptions
        .iter()
        .find_map(|e| match (e, field) {
            (
                E::ExtraReadOnlyPath {
                    canonical, reason, ..
                },
                PolicyField::ExtraReadOnlyPath(p),
            ) if canonical == p => Some(reason.clone()),
            (E::ExtraDevice { path, reason }, PolicyField::ExtraDevice(p)) if path == p => {
                Some(reason.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

// ---- Explanation from a policy alone ------------------------------------------------------

/// What [`explain_policy`] needs about the game and the user: the parts of a
/// [`SandboxContext`](super::SandboxContext) that change the statements. Everything else (session
/// directories, the resolved runtime, `umu-run`, the init shim) is synthesised.
#[derive(Clone, Copy)]
pub struct PolicyContext<'a> {
    /// The game (a draft uses any stable ID).
    pub game: crate::ids::GameId,
    /// Game folder; `None` for a loose program.
    pub root: Option<&'a Path>,
    /// The program.
    pub exe: &'a Path,
    /// Real home.
    pub home: &'a Path,
    /// User ID.
    pub uid: u32,
    /// Primary group ID.
    pub gid: u32,
    /// User name.
    pub user_name: &'a str,
    /// Primary group name.
    pub group_name: &'a str,
    /// EasyPlay's locations.
    pub paths: &'a crate::paths::AppPaths,
    /// The game's launch configuration (working folder, compatibility options, variables).
    pub launch: &'a crate::model::LaunchConfig,
    /// Global user variables.
    pub global_env: &'a [crate::model::EnvVar],
    /// The launcher's environment.
    pub host_env: &'a BTreeMap<String, String>,
    /// XDG user directories.
    pub user_dirs: &'a [(UserDir, PathBuf)],
    /// Other games' folders.
    pub other_game_roots: &'a [PathBuf],
    /// Mount table.
    pub mounts: &'a crate::fsx::MountTable,
    /// Existence checks (`stat`, `realpath`). Directory listings are never made: the breadth
    /// scan of a broad grant is not part of an explanation (it runs when the policy is saved).
    pub fs: &'a dyn crate::fsx::Fs,
    /// Developer Mode is on.
    pub developer_mode: bool,
    /// A seccomp program can be built.
    pub seccomp_available: bool,
    /// The runtime isolation the launch would use.
    pub isolation: crate::model::RuntimeIsolationMode,
}

/// A policy explained without a launch: statements and validation issues.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyExplanation {
    /// The statements `explain(plan)` gives for the plan of this policy.
    pub statements: Vec<AccessStatement>,
    /// Validation and plan issues (blocking ones included).
    pub issues: Vec<super::PolicyIssue>,
}

/// The synthetic runtime directory's name under the store.
const EXPLAIN_RUNTIME: &str = "explain";

/// Read-only view for planning without a launch: no directory is listed (the synthetic
/// runtime is empty, a breadth scan sees nothing) and the synthetic runtime exists.
struct ExplainFs<'a> {
    inner: &'a dyn crate::fsx::Fs,
    runtime: PathBuf,
}

impl ExplainFs<'_> {
    fn refuse<T>(path: &Path) -> crate::fsx::FsResult<T> {
        Err(crate::fsx::FsError::new(
            crate::fsx::FsOp::Write,
            path,
            crate::fsx::FsErrorKind::ReadOnly,
        ))
    }
}

impl crate::fsx::Fs for ExplainFs<'_> {
    fn metadata(&self, path: &Path) -> crate::fsx::FsResult<crate::fsx::Metadata> {
        self.inner.metadata(path)
    }
    fn metadata_follow(&self, path: &Path) -> crate::fsx::FsResult<crate::fsx::Metadata> {
        self.inner.metadata_follow(path)
    }
    fn read_link(&self, path: &Path) -> crate::fsx::FsResult<PathBuf> {
        self.inner.read_link(path)
    }
    fn read(&self, path: &Path, limit: u64) -> crate::fsx::FsResult<Vec<u8>> {
        self.inner.read(path, limit)
    }
    fn read_follow(&self, path: &Path, limit: u64) -> crate::fsx::FsResult<Vec<u8>> {
        self.inner.read_follow(path, limit)
    }
    fn read_range(&self, path: &Path, offset: u64, len: usize) -> crate::fsx::FsResult<Vec<u8>> {
        self.inner.read_range(path, offset, len)
    }
    fn open_read(&self, path: &Path) -> crate::fsx::FsResult<Box<dyn std::io::Read + Send>> {
        self.inner.open_read(path)
    }
    fn read_dir(&self, _path: &Path) -> crate::fsx::FsResult<Vec<crate::fsx::DirEntry>> {
        Ok(Vec::new())
    }
    fn canonicalize(&self, path: &Path) -> crate::fsx::FsResult<PathBuf> {
        self.inner.canonicalize(path)
    }
    fn create_dir(&self, path: &Path, _mode: u32) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn create_dir_all(&self, path: &Path, _mode: u32) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn write_new(&self, path: &Path, _data: &[u8], _mode: u32) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn atomic_write(&self, path: &Path, _data: &[u8], _mode: u32) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn rename(&self, from: &Path, _to: &Path) -> crate::fsx::FsResult<()> {
        Self::refuse(from)
    }
    fn remove_file(&self, path: &Path) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn remove_dir(&self, path: &Path) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn symlink(&self, _target: &Path, link: &Path) -> crate::fsx::FsResult<()> {
        Self::refuse(link)
    }
    fn set_mode(&self, path: &Path, _mode: u32) -> crate::fsx::FsResult<()> {
        Self::refuse(path)
    }
    fn free_space(&self, path: &Path) -> crate::fsx::FsResult<u64> {
        self.inner.free_space(path)
    }
    fn reflink_supported(&self, a: &Path, b: &Path) -> crate::fsx::FsResult<bool> {
        self.inner.reflink_supported(a, b)
    }
    fn exists(&self, path: &Path) -> bool {
        path.starts_with(&self.runtime) || self.inner.exists(path)
    }
}

/// Explain `policy` as it would run for a game, without a launch plan (the setup sheet and the
/// Protection tab, SCREEN_SPECS §6). The policy is planned for a Game session with a synthetic
/// runtime, session and `umu-run`, then [`explain`]ed: for the same policy, game and host the
/// statements equal those of the real launch plan (only their mount indices differ, since the
/// runtime is not listed). When planning is blocked, the statements are those of the policy
/// without the folders and exceptions the blocking issues name; `issues` lists everything.
pub fn explain_policy(
    policy: &SandboxPolicy,
    ctx: &PolicyContext<'_>,
    host: &crate::host::HostFacts,
) -> PolicyExplanation {
    use super::context::{GameContext, GamePaths, LaunchInputs, SandboxContext, SessionDirs};
    use crate::model::{ResolvedRuntime, RuntimeOrigin, SessionKind, SlrVariant};

    let variant = SlrVariant::Steamrt3;
    let slr_dir = ctx.paths.umu_store_dir.join(EXPLAIN_RUNTIME);
    let fs = ExplainFs {
        inner: ctx.fs,
        runtime: slr_dir.clone(),
    };
    let Ok(name) = crate::ids::RuntimeName::new(EXPLAIN_RUNTIME) else {
        return PolicyExplanation::default();
    };
    let runtime = ResolvedRuntime {
        name,
        proton_path_host: ctx.paths.compat_tools_dir.join(EXPLAIN_RUNTIME),
        proton_path_sandbox: ctx
            .home
            .join(".local/share/Steam/compatibilitytools.d")
            .join(EXPLAIN_RUNTIME),
        origin: RuntimeOrigin::Store,
        slr_variant: variant,
        slr_dir: slr_dir.clone(),
        platform_dir: PathBuf::from("sniper_platform_explain"),
        isolation: ctx.isolation,
    };
    let umu = host.umu.clone().unwrap_or_else(|| crate::host::UmuFacts {
        path: PathBuf::from("/usr/bin/umu-run"),
        version: None,
        closure: Vec::new(),
        wrapper: None,
    });
    let session = SessionDirs::new(
        ctx.paths,
        crate::ids::SessionId::from_uuid(uuid::Uuid::nil()),
    );
    let game_paths = GamePaths::new(ctx.paths, ctx.game);
    let umu_args = [ctx.exe.as_os_str().to_owned()];
    let init = ctx.paths.data_dir.join("easyplay-init");
    let sandbox_ctx = SandboxContext {
        kind: SessionKind::Game,
        uid: ctx.uid,
        gid: ctx.gid,
        user_name: ctx.user_name,
        group_name: ctx.group_name,
        home: ctx.home,
        game: Some(GameContext {
            id: ctx.game,
            private_home: &game_paths.private_home,
            prefix: &game_paths.prefix,
            overlay_upper: &game_paths.overlay_upper,
            overlay_work: &game_paths.overlay_work,
            root: ctx.root,
            exe: ctx.exe,
            umu_id: None,
            store: None,
            prefix_lock: &game_paths.prefix_lock,
        }),
        runtime: Some(&runtime),
        proton_codename: None,
        session: &session,
        paths: ctx.paths,
        init_binary: &init,
        umu: &umu,
        network_override: None,
        launch: LaunchInputs {
            working_dir: &ctx.launch.working_dir,
            compat: &ctx.launch.compat,
            global_env: ctx.global_env,
            game_env: &ctx.launch.env,
            umu_args: &umu_args,
            verbose_log: false,
        },
        host_env: ctx.host_env,
        install_id: "00000000000000000000000000000000",
        developer_mode: ctx.developer_mode,
        seccomp_available: ctx.seccomp_available,
        user_dirs: ctx.user_dirs,
        other_game_roots: ctx.other_game_roots,
        mounts: ctx.mounts,
        fs: &fs,
    };
    let synthetic = |i: &super::PolicyIssue| {
        i.code == IssueCode::RuntimeLayout
            && i.path.as_ref().is_some_and(|p| p.starts_with(&slr_dir))
    };
    match super::planner::plan(policy, &sandbox_ctx, host) {
        Ok(plan) => PolicyExplanation {
            statements: explain(&plan, policy),
            issues: plan.issues.into_iter().filter(|i| !synthetic(i)).collect(),
        },
        Err(issues) => {
            let issues: Vec<_> = issues.into_iter().filter(|i| !synthetic(i)).collect();
            let named: Vec<&PathBuf> = issues
                .iter()
                .filter(|i| i.is_blocking())
                .filter_map(|i| i.path.as_ref())
                .collect();
            let hit = |p: &Path| named.iter().any(|n| n.as_path() == p);
            let mut reduced = policy.clone();
            reduced
                .folders
                .retain(|g| !hit(&g.canonical) && !hit(&g.path));
            reduced.exceptions.retain(|e| match e {
                crate::model::PolicyException::ExtraReadOnlyPath {
                    canonical, path, ..
                } => !hit(canonical) && !hit(path),
                crate::model::PolicyException::ExtraDevice { path, .. } => !hit(path),
                _ => true,
            });
            if let crate::model::SaveDataLocation::External { dir } = &reduced.save_data
                && hit(dir)
            {
                reduced.save_data = crate::model::SaveDataLocation::Internal;
            }
            let statements = match super::planner::plan(&reduced, &sandbox_ctx, host) {
                Ok(plan) if reduced != *policy => explain(&plan, &reduced),
                _ => Vec::new(),
            };
            PolicyExplanation { statements, issues }
        }
    }
}

/// Statements added and removed between two explanations (for the permissions editor).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatementDiff {
    /// In `new` only.
    pub added: Vec<AccessStatement>,
    /// In `old` only.
    pub removed: Vec<AccessStatement>,
}

/// Compare by category, key and arguments.
pub fn diff(old: &[AccessStatement], new: &[AccessStatement]) -> StatementDiff {
    let same = |a: &AccessStatement, b: &AccessStatement| {
        a.category == b.category && a.key == b.key && a.args == b.args
    };
    StatementDiff {
        added: new
            .iter()
            .filter(|n| !old.iter().any(|o| same(o, n)))
            .cloned()
            .collect(),
        removed: old
            .iter()
            .filter(|o| !new.iter().any(|n| same(o, n)))
            .cloned()
            .collect(),
    }
}
