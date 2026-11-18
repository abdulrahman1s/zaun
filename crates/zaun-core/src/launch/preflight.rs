//! Preflight: the checks of LOGIC_SPECS §3 "Validating", each with a typed failure that maps
//! to the code of the failure table.
//!
//! Hard failures stop the launch ([`PreflightFailure`]). Findings the orchestrator turns
//! into prompts (a changed fingerprint → `ExeChanged`, a moved grant target →
//! `GrantTargetChanged`) or into a dismissible warning (low disk space, `ZN-FS-002`) are
//! returned in the [`PreflightReport`]. Re-hashing a changed executable is the
//! orchestrator's job (it needs the identity module).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsErrorKind, Metadata, MountTable};
use crate::host::HostFacts;
use crate::model::{Executable, Game, RuntimeIsolationMode, SessionKind};
use crate::sandbox::{IssueSeverity, PolicyIssue, ValidationInputs, validate_policy};

/// Deadline of each filesystem check (LOGIC_SPECS §3 timeouts).
pub const STAT_DEADLINE: Duration = Duration::from_secs(5);
/// Free space below which a launch warns (PERSISTENCE §8 "Before launching").
pub const MIN_FREE_LAUNCH: u64 = 512 * 1024 * 1024;
/// Free space below which a launch warns when Isolated mode will copy the runtime in full.
pub const MIN_FREE_ISOLATED_COPY: u64 = 2_560 * 1024 * 1024;
/// Free space a Provision session needs on the store's filesystem (blocking).
pub const MIN_FREE_PROVISION_STORE: u64 = 3 * 1024 * 1024 * 1024;
/// Free space a Provision session needs on the cache's filesystem (blocking).
pub const MIN_FREE_PROVISION_CACHE: u64 = 1024 * 1024 * 1024;

/// Whether a session already holds a prefix lock. The real implementation is
/// `session::FlockLocks` (the supervisor's `flock`); [`NoLocks`] reports every lock free and
/// is for tests only.
pub trait PrefixLocks: Send + Sync {
    /// Whether `lock` is held by a live session.
    fn is_held(&self, lock: &Path) -> bool;
}

/// No session holds anything (tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLocks;

impl PrefixLocks for NoLocks {
    fn is_held(&self, _: &Path) -> bool {
        false
    }
}

/// Everything preflight reads.
pub struct PreflightInput<'a> {
    /// The game.
    pub game: &'a Game,
    /// The executable to run (must belong to the game).
    pub exe: &'a Executable,
    /// Session kind.
    pub kind: SessionKind,
    /// Host facts.
    pub host: &'a HostFacts,
    /// Policy validation inputs (built from the planned context).
    pub validation: ValidationInputs<'a>,
    /// Filesystem used for the deadline-bound checks.
    pub fs: Arc<dyn Fs>,
    /// Mount table (to tell a disconnected drive from a missing file).
    pub mounts: &'a MountTable,
    /// Host path of `zaun-init`.
    pub init_binary: &'a Path,
    /// The game's prefix lock.
    pub prefix_lock: &'a Path,
    /// Prefix lock probe.
    pub locks: &'a dyn PrefixLocks,
    /// Data directory (free-space check).
    pub data_dir: &'a Path,
    /// Runtime isolation of the launch and whether the copy will be a reflink.
    pub isolation: Option<(RuntimeIsolationMode, Option<bool>)>,
    /// Deadline per filesystem check.
    pub deadline: Duration,
}

/// A failure that stops the launch.
#[derive(Debug, thiserror::Error)]
pub enum PreflightFailure {
    /// The executable row does not belong to the game, or the rows fail validation.
    #[error("game and executable rows are inconsistent: {0}")]
    RowsInconsistent(String),
    /// The executable is gone.
    #[error("executable missing: {0}")]
    ExeMissing(PathBuf),
    /// The drive holding the executable is not connected.
    #[error("drive not connected: {0}")]
    DriveDisconnected(PathBuf),
    /// The executable cannot be read.
    #[error("executable unreadable: {0}")]
    ExeUnreadable(PathBuf),
    /// A filesystem check did not finish in time.
    #[error("filesystem check timed out: {0}")]
    Timeout(PathBuf),
    /// The policy is not valid on this host.
    #[error("policy invalid on this host")]
    PolicyInvalid(Vec<PolicyIssue>),
    /// A host requirement is missing.
    #[error("host requirement missing: {0}")]
    Host(ErrorCode),
    /// A session is already running on this prefix.
    #[error("a session is already running on this prefix")]
    AlreadyRunning,
    /// Not enough space for a Provision session.
    #[error("not enough free space: {available} < {needed} on {path}")]
    NoSpace {
        /// Filesystem checked.
        path: PathBuf,
        /// Free bytes.
        available: u64,
        /// Required bytes.
        needed: u64,
    },
}

impl PreflightFailure {
    /// The error code of the LOGIC_SPECS §3 failure table.
    pub fn code(&self) -> ErrorCode {
        match self {
            PreflightFailure::RowsInconsistent(_) => ErrorCode::LibInvalidField,
            PreflightFailure::ExeMissing(_) => ErrorCode::LibExeNotFound,
            PreflightFailure::DriveDisconnected(_) => ErrorCode::LibDriveDisconnected,
            PreflightFailure::ExeUnreadable(_) => ErrorCode::LibExeUnreadable,
            PreflightFailure::Timeout(_) => ErrorCode::FsTimeout,
            PreflightFailure::PolicyInvalid(issues) => issues
                .iter()
                .find(|i| i.is_blocking())
                .map_or(ErrorCode::SbxGrantMissing, |i| i.code.error_code()),
            PreflightFailure::Host(code) => *code,
            PreflightFailure::AlreadyRunning => ErrorCode::SesAlreadyRunning,
            PreflightFailure::NoSpace { .. } => ErrorCode::FsLowSpace,
        }
    }
}

impl From<PreflightFailure> for Error {
    fn from(f: PreflightFailure) -> Self {
        let code = f.code();
        let mut e = Error::new(code);
        match &f {
            PreflightFailure::ExeMissing(p)
            | PreflightFailure::DriveDisconnected(p)
            | PreflightFailure::ExeUnreadable(p)
            | PreflightFailure::Timeout(p) => e = e.with("path", p.display()),
            PreflightFailure::PolicyInvalid(issues) => {
                if let Some(i) = issues.iter().find(|i| i.is_blocking()) {
                    e = e.with("issue", i.code.as_str());
                }
            }
            _ => {}
        }
        e.with_source(f)
    }
}

/// The executable's state compared with its stored fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintCheck {
    /// Size and modification time match.
    Unchanged,
    /// Size or modification time changed: re-hash, then prompt `ExeChanged` if the identity
    /// differs.
    Changed {
        /// Current size.
        size: u64,
        /// Current modification time.
        mtime_ns: i64,
    },
}

/// A grant whose path now resolves somewhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantChange {
    /// Path as chosen.
    pub path: PathBuf,
    /// Canonical path stored with the grant.
    pub old: PathBuf,
    /// Canonical path now.
    pub new: PathBuf,
}

/// What preflight found besides hard failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightReport {
    /// Fingerprint comparison.
    pub fingerprint: FingerprintCheck,
    /// Grants to confirm (prompt `GrantTargetChanged`).
    pub grant_changes: Vec<GrantChange>,
    /// Non-blocking policy issues.
    pub warnings: Vec<PolicyIssue>,
    /// Free space is low (`ZN-FS-002`, dismissible): `(available, wanted)`.
    pub low_space: Option<(u64, u64)>,
}

/// The blocking free-space check before a Provision session (PERSISTENCE §8): at least
/// 3 GiB on the store's filesystem and 1 GiB on the cache's.
pub fn provision_space(
    fs: &Arc<dyn Fs>,
    store: &Path,
    cache: &Path,
    deadline: Duration,
) -> Result<(), PreflightFailure> {
    for (path, needed) in [
        (store, MIN_FREE_PROVISION_STORE),
        (cache, MIN_FREE_PROVISION_CACHE),
    ] {
        let f = Arc::clone(fs);
        let p = path.to_path_buf();
        match with_deadline(deadline, move || f.free_space(&p)) {
            None => return Err(PreflightFailure::Timeout(path.to_path_buf())),
            Some(Ok(available)) if available < needed => {
                return Err(PreflightFailure::NoSpace {
                    path: path.to_path_buf(),
                    available,
                    needed,
                });
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Run `f` on a helper thread and give up after `deadline`.
fn with_deadline<T: Send + 'static>(
    deadline: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("preflight-stat".into())
        .spawn(move || {
            let _ = tx.send(f());
        });
    match spawned {
        Ok(_) => rx.recv_timeout(deadline).ok(),
        Err(_) => None,
    }
}

/// `metadata_follow(path)` with a deadline.
pub fn stat_with_deadline(
    fs: &Arc<dyn Fs>,
    path: &Path,
    deadline: Duration,
) -> Option<crate::fsx::FsResult<Metadata>> {
    let fs = Arc::clone(fs);
    let p = path.to_path_buf();
    with_deadline(deadline, move || fs.metadata_follow(&p))
}

/// Host requirements of a session of `kind`, in the order of the Doctor checks.
pub fn host_requirements(host: &HostFacts, kind: SessionKind) -> Result<(), ErrorCode> {
    let Some(bwrap) = &host.bwrap else {
        return Err(ErrorCode::HostBwrapNotFound);
    };
    if !bwrap.missing_required().is_empty() {
        return Err(ErrorCode::HostBwrapMissingOption);
    }
    if host.problem(ErrorCode::HostBwrapSetuidOnly).is_some() {
        return Err(ErrorCode::HostBwrapSetuidOnly);
    }
    if !bwrap.userns_ok {
        return Err(ErrorCode::HostUsernsUnavailable);
    }
    if !bwrap.nested_userns_ok {
        return Err(ErrorCode::HostNestedUsernsUnavailable);
    }
    let Some(umu) = &host.umu else {
        return Err(ErrorCode::HostUmuNotFound);
    };
    if !umu.version_accepted() {
        return Err(ErrorCode::HostUmuTooOld);
    }
    if host.problem(ErrorCode::HostUmuClosureRejected).is_some() {
        return Err(ErrorCode::HostUmuClosureRejected);
    }
    if kind.is_untrusted()
        && host.display.wayland.is_none()
        && host.display.x11_bindable().is_none()
    {
        return Err(ErrorCode::HostNoDisplay);
    }
    Ok(())
}

fn exe_failure(input: &PreflightInput<'_>, path: &Path, kind: FsErrorKind) -> PreflightFailure {
    match kind {
        FsErrorKind::NotFound | FsErrorKind::NotADirectory => {
            // A recorded mount point that is no longer mounted means the drive is away.
            let drive_gone = input.exe.mount_point.as_deref().is_some_and(|mp| {
                input
                    .mounts
                    .mount_point_of(path)
                    .map(|m| m.mount_point.as_path())
                    != Some(mp)
            });
            if drive_gone {
                PreflightFailure::DriveDisconnected(path.to_path_buf())
            } else {
                PreflightFailure::ExeMissing(path.to_path_buf())
            }
        }
        FsErrorKind::TimedOut => PreflightFailure::Timeout(path.to_path_buf()),
        _ => PreflightFailure::ExeUnreadable(path.to_path_buf()),
    }
}

/// Run the checks in the order of LOGIC_SPECS §3.
pub fn preflight(input: &PreflightInput<'_>) -> Result<PreflightReport, PreflightFailure> {
    // 1. Rows consistent.
    if !input.game.executables.iter().any(|e| e.id == input.exe.id) {
        return Err(PreflightFailure::RowsInconsistent(
            "the executable does not belong to the game".into(),
        ));
    }
    input
        .game
        .validate()
        .map_err(|e| PreflightFailure::RowsInconsistent(e.to_string()))?;

    // 2. Executable present (deadline).
    let path = input.exe.canonical_path.as_path();
    let meta = match stat_with_deadline(&input.fs, path, input.deadline) {
        None => return Err(PreflightFailure::Timeout(path.to_path_buf())),
        Some(Err(e)) => return Err(exe_failure(input, path, e.kind)),
        Some(Ok(m)) if !m.is_file() => return Err(PreflightFailure::ExeUnreadable(path.into())),
        Some(Ok(m)) => m,
    };

    // 3. Fingerprint.
    let fp = input.exe.fingerprint;
    let fingerprint = if fp.size == meta.size && fp.mtime_ns == meta.mtime_ns {
        FingerprintCheck::Unchanged
    } else {
        FingerprintCheck::Changed {
            size: meta.size,
            mtime_ns: meta.mtime_ns,
        }
    };

    // 4. Policy valid against current facts.
    let issues = validate_policy(&input.game.policy, &input.validation);
    if issues.iter().any(PolicyIssue::is_blocking) {
        return Err(PreflightFailure::PolicyInvalid(issues));
    }
    let warnings = issues
        .into_iter()
        .filter(|i| i.severity == IssueSeverity::Warning)
        .collect();

    // 5. Grant targets unchanged.
    let mut grant_changes = Vec::new();
    for g in &input.game.policy.folders {
        let fs = Arc::clone(&input.fs);
        let p = g.path.clone();
        match with_deadline(input.deadline, move || fs.canonicalize(&p)) {
            None => return Err(PreflightFailure::Timeout(g.path.clone())),
            Some(Ok(now)) if now != g.canonical => grant_changes.push(GrantChange {
                path: g.path.clone(),
                old: g.canonical.clone(),
                new: now,
            }),
            // A missing folder is a policy issue reported by step 4.
            Some(_) => {}
        }
    }

    // 6. Host requirements and the init shim.
    host_requirements(input.host, input.kind).map_err(PreflightFailure::Host)?;
    match stat_with_deadline(&input.fs, input.init_binary, input.deadline) {
        Some(Ok(m)) if m.is_file() && m.mode & 0o111 != 0 => {}
        _ => return Err(PreflightFailure::Host(ErrorCode::HostInitMissing)),
    }

    // 7. Prefix lock free.
    if input.locks.is_held(input.prefix_lock) {
        return Err(PreflightFailure::AlreadyRunning);
    }

    // 8. Disk space on the data filesystem.
    let wanted = match input.isolation {
        Some((RuntimeIsolationMode::Isolated, reflink)) if reflink != Some(true) => {
            MIN_FREE_ISOLATED_COPY
        }
        _ => MIN_FREE_LAUNCH,
    };
    let fs = Arc::clone(&input.fs);
    let dir = input.data_dir.to_path_buf();
    let low_space = match with_deadline(input.deadline, move || fs.free_space(&dir)) {
        Some(Ok(avail)) if avail < wanted => Some((avail, wanted)),
        _ => None,
    };
    Ok(PreflightReport {
        fingerprint,
        grant_changes,
        warnings,
        low_space,
    })
}
