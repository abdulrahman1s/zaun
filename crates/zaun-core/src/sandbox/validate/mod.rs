//! Validation of policies (SECURITY_MODEL §7.1–7.3) and of finished plans (§7.4).
//!
//! [`validate_policy`] runs when a policy is edited, when a game is saved, and at the start
//! of every planning call; [`plan_invariants`] runs on every finished plan. A plan with a
//! blocking issue is never executed.

mod advanced;
mod deny;
mod env_rules;
mod invariants;
mod issue;

use std::path::{Path, PathBuf};

pub use advanced::{AdvancedOption, advanced_options, effective_developer_mode};
pub use deny::{DenyGroup, DenyHit, DenyList, Relation, relation};
pub use env_rules::{UserVarRule, classify_user_var, is_never_set};
pub use invariants::plan_invariants;
pub use issue::{IssueCode, IssueSeverity, PolicyIssue, any_blocking};

use super::context::SandboxContext;
use crate::clock::{Clock, MonoInstant, Timestamp};
use crate::consts::MAX_ENV_VALUE_BYTES;
use crate::fsx::{FileType, Fs, MountTable, WalkControl, WalkLimits, safe_walk};
use crate::host::{HostFacts, SessionType};
use crate::model::{
    CompatOptions, ControllerAccess, DisplayIsolation, EnvVar, FolderAccess, GameDirAccess,
    GameFlags, NetworkAccess, PolicyException, SandboxPolicy, SaveDataLocation, SessionKind,
    SocketAccess, UserDir, WindowSystem, WorkingDir,
};
use crate::paths::{AppPaths, normalize};

/// Read-write grants on directories with more entries than this (depth ≤ 2) are broad.
pub const BROAD_GRANT_ENTRIES: usize = 10_000;
/// Minimum length of an exception reason (DEVELOPER_MODE §7).
pub const MIN_EXCEPTION_REASON_CHARS: usize = 10;

/// Pseudo-filesystems that may never be granted (§7.1 rule 4).
pub const PSEUDO_FILESYSTEMS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "cgroup2",
    "cgroup",
    "securityfs",
    "debugfs",
    "tracefs",
    "bpf",
];

/// Filesystems that cannot serve as an overlay lower layer reliably.
pub const NON_OVERLAY_LOWER_FS: &[&str] = &[
    "overlay", "fuse", "fuseblk", "vfat", "exfat", "ntfs", "ntfs3", "nfs", "nfs4", "cifs", "smb3",
    "9p",
];

/// Device nodes that are never exposed, not even through `ExtraDevice` (SECURITY_MODEL §5.3,
/// BUBBLEWRAP_SANDBOX §4.5).
pub const NEVER_DEVICES: &[&str] = &[
    "/dev/uinput",
    "/dev/kvm",
    "/dev/fuse",
    "/dev/mem",
    "/dev/kmem",
    "/dev/port",
    "/dev/kmsg",
    "/dev/console",
    "/dev/shm",
    "/dev/mqueue",
];

/// What validation needs to know besides the policy.
#[derive(Clone, Copy)]
pub struct ValidationInputs<'a> {
    /// Session kind the policy is validated for (`Game` when editing).
    pub kind: SessionKind,
    /// Filesystem (read-only use).
    pub fs: &'a dyn Fs,
    /// Mount table.
    pub mounts: &'a MountTable,
    /// Real home.
    pub home: &'a Path,
    /// User ID.
    pub uid: u32,
    /// User name.
    pub user_name: &'a str,
    /// Zaun locations.
    pub paths: &'a AppPaths,
    /// Developer Mode.
    pub developer_mode: bool,
    /// Game folder.
    pub game_root: Option<&'a Path>,
    /// Executable.
    pub exe: Option<&'a Path>,
    /// Private home of the game.
    pub private_home: Option<&'a Path>,
    /// Prefix of the game.
    pub prefix: Option<&'a Path>,
    /// XDG user directories.
    pub user_dirs: &'a [(UserDir, PathBuf)],
    /// Other games' folders.
    pub other_game_roots: &'a [PathBuf],
    /// Compatibility options.
    pub compat: Option<&'a CompatOptions>,
    /// Working directory choice.
    pub working_dir: Option<&'a WorkingDir>,
    /// Global then per-game user variables.
    pub user_env: [&'a [EnvVar]; 2],
    /// Host facts.
    pub host: &'a HostFacts,
}

impl<'a> ValidationInputs<'a> {
    /// The inputs of a planning call.
    pub fn from_context(ctx: &SandboxContext<'a>, host: &'a HostFacts) -> Self {
        Self {
            kind: ctx.kind,
            fs: ctx.fs,
            mounts: ctx.mounts,
            home: ctx.home,
            uid: ctx.uid,
            user_name: ctx.user_name,
            paths: ctx.paths,
            developer_mode: ctx.developer_mode,
            game_root: ctx.game.and_then(|g| g.root),
            exe: ctx.game.map(|g| g.exe),
            private_home: ctx.game.map(|g| g.private_home),
            prefix: ctx.game.map(|g| g.prefix),
            user_dirs: ctx.user_dirs,
            other_game_roots: ctx.other_game_roots,
            compat: Some(ctx.launch.compat),
            working_dir: Some(ctx.launch.working_dir),
            user_env: [ctx.launch.global_env, ctx.launch.game_env],
            host,
        }
    }

    /// The deny-list for this user.
    pub fn deny_list(&self) -> DenyList {
        DenyList::new(
            self.home,
            self.uid,
            self.user_name,
            self.paths,
            Some(self.mounts),
        )
    }
}

/// What kind of user path is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRole {
    /// A folder grant.
    Grant,
    /// The game folder.
    GameDir,
    /// The external save folder.
    SaveDir,
    /// An `ExtraReadOnlyPath` exception.
    Exception,
}

/// A clock that is never consulted (walks run without a deadline; `sandbox` must not read
/// the clock).
struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(0)
    }
    fn mono(&self) -> MonoInstant {
        MonoInstant::ZERO
    }
}

/// Validate a user-chosen path (§7.1 normalisation and §7.2 deny-list). Returns the fresh
/// canonical path when the path is usable (possibly with warnings).
pub fn check_user_path(
    inputs: &ValidationInputs<'_>,
    deny: &DenyList,
    path: &Path,
    stored_canonical: Option<&Path>,
    role: PathRole,
    issues: &mut Vec<PolicyIssue>,
) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let p = Some(path.to_path_buf());
    if path.as_os_str().as_bytes().contains(&0) {
        issues.push(PolicyIssue::error(
            IssueCode::PathInvalid,
            p,
            "sec.path.nul|The path contains a NUL byte",
        ));
        return None;
    }
    if !path.is_absolute() {
        issues.push(PolicyIssue::error(
            IssueCode::PathNotAbsolute,
            p,
            "sec.path.relative|The path must be absolute",
        ));
        return None;
    }
    if let Some(stored) = stored_canonical
        && normalize(stored) != stored
    {
        issues.push(PolicyIssue::error(
            IssueCode::PathInvalid,
            Some(stored.to_path_buf()),
            "sec.path.not_normalised|The stored path is not normalised",
        ));
        return None;
    }
    let canonical = match inputs.fs.canonicalize(path) {
        Ok(c) => normalize(&c),
        Err(_) => {
            issues.push(PolicyIssue::error(
                IssueCode::PathMissing,
                p,
                "sec.path.missing|The folder does not exist",
            ));
            return None;
        }
    };
    let want_dir = !matches!(role, PathRole::Exception);
    match inputs.fs.metadata_follow(&canonical) {
        Ok(md) if want_dir && md.file_type != FileType::Dir => {
            issues.push(PolicyIssue::error(
                IssueCode::PathMissing,
                Some(canonical),
                "sec.path.not_dir|The path is not a folder",
            ));
            return None;
        }
        Ok(_) => {}
        Err(_) => {
            issues.push(PolicyIssue::error(
                IssueCode::PathMissing,
                Some(canonical),
                "sec.path.missing|The folder does not exist",
            ));
            return None;
        }
    }
    if let Some(fs_type) = inputs.mounts.fs_type_of(&canonical)
        && PSEUDO_FILESYSTEMS.contains(&fs_type)
    {
        issues.push(PolicyIssue::error(
            IssueCode::PathPseudoFs,
            Some(canonical),
            "sec.path.pseudo_fs|System information folders cannot be shared",
        ));
        return None;
    }
    let mut ok = true;
    let mut to_check = vec![canonical.clone()];
    if let Some(stored) = stored_canonical
        && stored != canonical
    {
        to_check.push(stored.to_path_buf());
        issues.push(
            PolicyIssue::warning(
                IssueCode::PathTargetChanged,
                Some(path.to_path_buf()),
                "sec.path.target_changed|The folder now points somewhere else; confirm it",
            )
            .acknowledged_by(GameFlags::BROAD_GRANT_ACK),
        );
    }
    for candidate in &to_check {
        for hit in deny.check(candidate) {
            if role == PathRole::Exception && inputs.developer_mode && hit.group.bypassable() {
                issues.push(PolicyIssue::warning(
                    IssueCode::PathDenyBypassed,
                    Some(candidate.clone()),
                    format!(
                        "sec.path.deny_bypassed|Lets this program read {}",
                        hit.entry.display()
                    ),
                ));
                continue;
            }
            ok = false;
            issues.push(PolicyIssue::error(
                IssueCode::PathDenied,
                Some(candidate.clone()),
                format!(
                    "sec.path.denied.{:?}|{} cannot be shared ({:?} of {})",
                    hit.group,
                    candidate.display(),
                    hit.relation,
                    hit.entry.display()
                )
                .to_lowercase(),
            ));
        }
    }
    ok.then_some(canonical)
}

/// Count entries below `dir` up to depth 2, stopping just above the threshold.
fn is_broad(fs: &dyn Fs, dir: &Path) -> bool {
    let limits = WalkLimits {
        max_entries: BROAD_GRANT_ENTRIES + 1,
        max_depth: 2,
        deadline: None,
    };
    let mut count = 0usize;
    let _ = safe_walk(fs, dir, limits, &NoClock, |_| {
        count += 1;
        WalkControl::Continue
    });
    count > BROAD_GRANT_ENTRIES
}

fn is_mount_root(mounts: &MountTable, dir: &Path) -> bool {
    mounts.mounts.iter().any(|m| m.mount_point == dir)
}

/// Validate a policy (§7.1–7.3 and CONFIGURATION §3 cross-field rules).
pub fn validate_policy(policy: &SandboxPolicy, inputs: &ValidationInputs<'_>) -> Vec<PolicyIssue> {
    let mut issues = Vec::new();
    if policy.schema_version > crate::consts::POLICY_SCHEMA_VERSION {
        issues.push(PolicyIssue::error(
            IssueCode::SchemaTooNew,
            None,
            "sec.policy.too_new|These settings were written by a newer version",
        ));
        return issues;
    }
    if policy.display.isolation == DisplayIsolation::Gamescope {
        issues.push(PolicyIssue::error(
            IssueCode::ReservedValue,
            None,
            "sec.policy.reserved|Nested compositor isolation is not available yet",
        ));
    }
    if let Err(e) = policy.validate()
        && e.problem != crate::model::Problem::Reserved
    {
        issues.push(PolicyIssue::error(
            IssueCode::PolicyMalformed,
            None,
            format!("sec.policy.malformed|{}: {:?}", e.field, e.problem),
        ));
    }
    let deny = inputs.deny_list();
    let untrusted = inputs.kind.is_untrusted();

    // Game folder.
    let mut root_canonical = None;
    if untrusted {
        match (inputs.game_root, policy.game_dir) {
            (None, GameDirAccess::FileOnly) => {}
            (None, _) => issues.push(PolicyIssue::error(
                IssueCode::GameDirKindMismatch,
                None,
                "sec.game.no_folder|A single program can only use \"this file only\"",
            )),
            (Some(root), _) => {
                if policy.game_dir == GameDirAccess::FileOnly {
                    issues.push(PolicyIssue::error(
                        IssueCode::GameDirKindMismatch,
                        Some(root.to_path_buf()),
                        "sec.game.file_only_with_folder|\"This file only\" is for single programs",
                    ));
                }
                root_canonical =
                    check_user_path(inputs, &deny, root, None, PathRole::GameDir, &mut issues);
                if let Some(root) = &root_canonical {
                    game_dir_warnings(policy, inputs, root, &mut issues);
                }
            }
        }
        if let (Some(exe), Some(root)) = (inputs.exe, inputs.game_root)
            && policy.game_dir != GameDirAccess::FileOnly
            && !normalize(exe).starts_with(normalize(root))
        {
            issues.push(PolicyIssue::error(
                IssueCode::ExeOutsideRoot,
                Some(exe.to_path_buf()),
                "sec.game.exe_outside|The program is not inside the game folder",
            ));
        }
        if policy.game_dir == GameDirAccess::Protected && !inputs.host.overlay_ok() {
            issues.push(PolicyIssue::error(
                IssueCode::OverlayUnsupported,
                inputs.game_root.map(Path::to_path_buf),
                "sec.game.overlay_unsupported|Protected game folders are not supported here",
            ));
        }
    }

    if untrusted
        && policy.removable != crate::model::RemovableAccess::None
        && let Some(root) = &root_canonical
        && [
            Path::new("/run/media").join(inputs.user_name),
            Path::new("/media").join(inputs.user_name),
        ]
        .iter()
        .any(|m| root.starts_with(m))
    {
        issues.push(PolicyIssue::error(
            IssueCode::PathCoversGameDir,
            Some(root.clone()),
            "sec.removable.covers_game|The game is on a removable drive; removable-media access would hide the game folder",
        ));
    }

    // Grants.
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut rw_outside_game = false;
    if untrusted {
        for grant in &policy.folders {
            let Some(c) = check_user_path(
                inputs,
                &deny,
                &grant.path,
                Some(&grant.canonical),
                PathRole::Grant,
                &mut issues,
            ) else {
                continue;
            };
            dedupe_and_cover(
                &grant.canonical,
                root_canonical.as_deref(),
                &mut seen,
                &mut issues,
            );
            if grant.access == FolderAccess::ReadWrite {
                let inside_game = root_canonical.as_deref().is_some_and(|r| c.starts_with(r));
                if !inside_game {
                    rw_outside_game = true;
                }
                if is_mount_root(inputs.mounts, &c) {
                    issues.push(
                        PolicyIssue::warning(
                            IssueCode::MountRootGrant,
                            Some(c.clone()),
                            "sec.grant.mount_root|This gives write access to a whole drive",
                        )
                        .acknowledged_by(GameFlags::BROAD_GRANT_ACK),
                    );
                } else if is_broad(inputs.fs, &c) {
                    issues.push(
                        PolicyIssue::warning(
                            IssueCode::BroadGrant,
                            Some(c.clone()),
                            "sec.grant.broad|This folder contains very many files",
                        )
                        .acknowledged_by(GameFlags::BROAD_GRANT_ACK),
                    );
                }
            }
        }
    }

    // Save folder (all kinds that mount the prefix).
    if let SaveDataLocation::External { dir } = &policy.save_data
        && inputs.kind != SessionKind::Provision
        && let Some(c) = check_user_path(inputs, &deny, dir, None, PathRole::SaveDir, &mut issues)
    {
        if let Some(root) = &root_canonical
            && c.starts_with(root)
            && matches!(
                policy.game_dir,
                GameDirAccess::ReadOnly | GameDirAccess::Protected
            )
        {
            issues.push(PolicyIssue::error(
                IssueCode::SaveDirConflict,
                Some(c.clone()),
                "sec.save.inside_protected|Save data cannot live inside a protected game folder",
            ));
        }
        if inputs.paths.games_dir.starts_with(&c) || c.starts_with(&inputs.paths.games_dir) {
            issues.push(PolicyIssue::error(
                IssueCode::SaveDirConflict,
                Some(c.clone()),
                "sec.save.other_game|Save data cannot live in Zaun's game storage",
            ));
        }
        if untrusted && !root_canonical.as_deref().is_some_and(|r| c.starts_with(r)) {
            rw_outside_game = true;
        }
    }

    // Exceptions.
    for exception in &policy.exceptions {
        if exception.reason().trim().chars().count() < MIN_EXCEPTION_REASON_CHARS {
            issues.push(PolicyIssue::error(
                IssueCode::ExceptionReason,
                None,
                "sec.exception.reason|Describe why this exception is needed (10 characters or more)",
            ));
        }
        match exception {
            PolicyException::ExtraReadOnlyPath {
                path, canonical, ..
            } => {
                if !inputs.developer_mode {
                    issues.push(needs_dev_mode(path));
                    continue;
                }
                if untrusted
                    && check_user_path(
                        inputs,
                        &deny,
                        path,
                        Some(canonical),
                        PathRole::Exception,
                        &mut issues,
                    )
                    .is_some()
                {
                    dedupe_and_cover(canonical, root_canonical.as_deref(), &mut seen, &mut issues);
                }
            }
            PolicyException::ExtraDevice { path, .. } => {
                if !inputs.developer_mode {
                    issues.push(needs_dev_mode(path));
                    continue;
                }
                check_extra_device(inputs.fs, path, &mut issues);
            }
            PolicyException::PassEnv { name, .. } => match classify_user_var(name) {
                UserVarRule::Allowed => {}
                UserVarRule::Dangerous if inputs.developer_mode => {}
                UserVarRule::Dangerous => issues.push(PolicyIssue::error(
                    IssueCode::EnvDangerous,
                    None,
                    format!("sec.env.dangerous|{name} needs Developer Mode"),
                )),
                UserVarRule::Reserved => issues.push(PolicyIssue::error(
                    IssueCode::EnvReserved,
                    None,
                    format!("sec.env.reserved|{name} is controlled by Zaun"),
                )),
                UserVarRule::Invalid => issues.push(PolicyIssue::error(
                    IssueCode::EnvInvalid,
                    None,
                    format!("sec.env.invalid|{name} is not a valid variable name"),
                )),
            },
        }
    }

    // User variables (UMU_INTEGRATION §4 table D).
    for var in inputs.user_env.iter().flat_map(|v| v.iter()) {
        check_user_var(var, inputs.developer_mode, &mut issues);
    }

    // Cross-field and breadth warnings.
    let effective_network = if untrusted {
        policy.network
    } else {
        NetworkAccess::None
    };
    if effective_network == NetworkAccess::Full && rw_outside_game {
        issues.push(PolicyIssue::warning(
            IssueCode::NetworkWithWritableGrant,
            None,
            "sec.net.writable_grant|This program can change your folders and use the internet",
        ));
    }
    if untrusted
        && policy.devices.controllers == ControllerAccess::AllInput
        && inputs.host.input.keyboards_readable
    {
        issues.push(PolicyIssue::warning(
            IssueCode::AllInputUnsafe,
            None,
            "sec.input.unsafe|Your account can read keyboards directly; all-device mode would expose them",
        ));
    }
    if inputs.host.display.session_type == SessionType::X11
        && policy.display.x11 == SocketAccess::Auto
    {
        issues.push(
            PolicyIssue::warning(
                IssueCode::X11Session,
                None,
                "sec.display.x11|On X11 a program can see and control other windows",
            )
            .acknowledged_by(GameFlags::X11_NOTICE_ACK),
        );
    }
    if policy.display.wayland == SocketAccess::Deny && policy.display.x11 == SocketAccess::Deny {
        issues.push(PolicyIssue::error(
            IssueCode::NoDisplay,
            None,
            "sec.display.none|The program needs a display to open a window",
        ));
    }
    if let Some(compat) = inputs.compat {
        let denied = match compat.window_system {
            WindowSystem::Wayland => policy.display.wayland == SocketAccess::Deny,
            WindowSystem::X11 => policy.display.x11 == SocketAccess::Deny,
            WindowSystem::Auto => false,
        };
        if denied {
            issues.push(PolicyIssue::error(
                IssueCode::WindowSystemDenied,
                None,
                "sec.display.window_system|The chosen window system is not allowed",
            ));
        }
    }
    issues
}

fn needs_dev_mode(path: &Path) -> PolicyIssue {
    PolicyIssue::error(
        IssueCode::ExceptionNeedsDevMode,
        Some(path.to_path_buf()),
        "sec.exception.dev_mode|Exceptions need Developer Mode",
    )
}

fn dedupe_and_cover(
    canonical: &Path,
    root: Option<&Path>,
    seen: &mut Vec<PathBuf>,
    issues: &mut Vec<PolicyIssue>,
) {
    if seen.iter().any(|s| s == canonical) {
        issues.push(PolicyIssue::error(
            IssueCode::PathDuplicate,
            Some(canonical.to_path_buf()),
            "sec.grant.duplicate|This folder is listed twice",
        ));
    }
    seen.push(canonical.to_path_buf());
    if let Some(root) = root
        && root.starts_with(canonical)
    {
        issues.push(PolicyIssue::error(
            IssueCode::PathCoversGameDir,
            Some(canonical.to_path_buf()),
            "sec.grant.covers_game|This folder contains the game folder; change the game folder access instead",
        ));
    }
}

fn game_dir_warnings(
    policy: &SandboxPolicy,
    inputs: &ValidationInputs<'_>,
    root: &Path,
    issues: &mut Vec<PolicyIssue>,
) {
    if inputs.user_dirs.iter().any(|(_, d)| normalize(d) == root) {
        issues.push(
            PolicyIssue::warning(
                IssueCode::GameDirUserDir,
                Some(root.to_path_buf()),
                "sec.game.user_dir|The game folder is one of your personal folders",
            )
            .acknowledged_by(GameFlags::BROAD_GRANT_ACK),
        );
    }
    if inputs
        .other_game_roots
        .iter()
        .any(|o| normalize(o) != root && normalize(o).starts_with(root))
    {
        issues.push(
            PolicyIssue::warning(
                IssueCode::GameDirContainsGame,
                Some(root.to_path_buf()),
                "sec.game.contains_game|The game folder contains another game",
            )
            .acknowledged_by(GameFlags::BROAD_GRANT_ACK),
        );
    }
    if policy.game_dir == GameDirAccess::Protected
        && let Some(t) = inputs.mounts.fs_type_of(root)
        && NON_OVERLAY_LOWER_FS.contains(&t)
    {
        issues.push(PolicyIssue::warning(
            IssueCode::OverlayLowerFs,
            Some(root.to_path_buf()),
            "sec.game.overlay_fs|This drive may not work with a protected game folder",
        ));
    }
}

/// Check an `ExtraDevice` path: a character device under `/dev`, not on the never-list, not
/// a block device or TTY.
pub fn check_extra_device(fs: &dyn Fs, path: &Path, issues: &mut Vec<PolicyIssue>) {
    let p = normalize(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let never = NEVER_DEVICES.iter().any(|d| p.starts_with(d))
        || name.starts_with("tty")
        || name == "console";
    let is_char = fs
        .metadata(&p)
        .is_ok_and(|m| m.file_type == FileType::CharDevice);
    if never || !is_char || !p.starts_with("/dev") || p == Path::new("/dev") {
        issues.push(PolicyIssue::error(
            IssueCode::ExceptionDevice,
            Some(p),
            "sec.exception.device|This device cannot be shared",
        ));
    }
}

/// Check one user variable against table D.
pub fn check_user_var(var: &EnvVar, developer_mode: bool, issues: &mut Vec<PolicyIssue>) {
    if var.value.len() > MAX_ENV_VALUE_BYTES || var.value.contains('\0') {
        issues.push(PolicyIssue::error(
            IssueCode::EnvInvalid,
            None,
            format!(
                "sec.env.invalid_value|The value of {} is not allowed",
                var.name
            ),
        ));
    }
    match classify_user_var(&var.name) {
        UserVarRule::Allowed => {}
        UserVarRule::Dangerous if developer_mode => {}
        UserVarRule::Dangerous => issues.push(PolicyIssue::error(
            IssueCode::EnvDangerous,
            None,
            format!("sec.env.dangerous|{} needs Developer Mode", var.name),
        )),
        UserVarRule::Reserved => issues.push(PolicyIssue::error(
            IssueCode::EnvReserved,
            None,
            format!("sec.env.reserved|{} is controlled by Zaun", var.name),
        )),
        UserVarRule::Invalid => issues.push(PolicyIssue::error(
            IssueCode::EnvInvalid,
            None,
            format!("sec.env.invalid|{} is not a valid variable name", var.name),
        )),
    }
}
