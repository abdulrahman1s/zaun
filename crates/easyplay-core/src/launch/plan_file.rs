//! The `LaunchPlan` file (DATA_MODEL §8): types, JSON read/write (mode 0600 in the session
//! runtime directory), format versioning, and the human rendering `plan.txt` (LOGGING §3).
//!
//! The file carries the rendered arguments, not the policy: the supervisor executes exactly
//! what the planner and the inspector showed.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsError};
use crate::ids::{ExecutableId, GameId, SessionId};
use crate::logging::Redactor;
use crate::model::{SessionContext, SessionKind, SlrVariant};
use crate::sandbox::plan::codec;
use crate::sandbox::{FdPayload, HostAction, plan::FdContent};

/// Version of the plan file written by this build.
pub const LAUNCH_PLAN_FORMAT: u32 = 1;
/// Largest plan file read back.
pub const MAX_PLAN_BYTES: u64 = 16 * 1024 * 1024;
/// Mode of the plan file.
pub const PLAN_FILE_MODE: u32 = 0o600;

/// A complete, validated launch: what `easyplay supervise --plan` executes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    /// [`LAUNCH_PLAN_FORMAT`].
    pub format: u32,
    /// Session metadata.
    pub session: PlanSession,
    /// Launch context recorded with the session row.
    pub context: SessionContext,
    /// Host path of `bwrap`.
    #[serde(with = "codec::path")]
    pub bwrap: PathBuf,
    /// Rendered sandbox.
    pub sandbox: PlanSandbox,
    /// Work completed on the host before `exec` (BUBBLEWRAP_SANDBOX §8).
    pub host_actions: Vec<HostAction>,
    /// Session paths.
    pub paths: PlanPaths,
    /// Audit to schedule after the session (Shared mode).
    pub audit_after: Option<AuditAfter>,
}

/// Session metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSession {
    /// Session ID.
    pub id: SessionId,
    /// Kind.
    pub kind: SessionKind,
    /// The game (`None` for Provision).
    pub game_id: Option<GameId>,
    /// Display name for the session row.
    pub game_name: String,
    /// The executable (`None` for Prepare, Provision and built-in tools).
    pub executable_id: Option<ExecutableId>,
    /// Whether the session may count towards statistics.
    pub counted_eligible: bool,
    /// Grace period of a graceful stop.
    pub stop_grace_secs: u8,
}

/// The rendered sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSandbox {
    /// `bwrap` options, passed NUL-separated through `--args 3`.
    #[serde(with = "codec::os_vec")]
    pub args: Vec<OsString>,
    /// Descriptor payloads.
    pub fds: Vec<PlanFd>,
    /// In-sandbox command after `--`.
    #[serde(with = "codec::os_vec")]
    pub command: Vec<OsString>,
}

/// What a descriptor carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FdKind {
    /// File content (`--ro-bind-data`, `--file`).
    #[default]
    Data,
    /// The seccomp program (`--seccomp`). Its content is built by the spawner until the
    /// seccomp builder exists; `content_b64` is then empty.
    Seccomp,
}

impl FdKind {
    fn is_data(&self) -> bool {
        *self == FdKind::Data
    }
}

/// One descriptor payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanFd {
    /// Descriptor number.
    pub fd: i32,
    /// Sandbox path the content appears at.
    #[serde(
        default,
        with = "codec::opt_path",
        skip_serializing_if = "Option::is_none"
    )]
    pub target: Option<PathBuf>,
    /// Kind.
    #[serde(default, skip_serializing_if = "FdKind::is_data")]
    pub kind: FdKind,
    /// Content, base64.
    pub content_b64: String,
}

impl PlanFd {
    /// From a sandbox plan payload.
    pub fn from_payload(p: &FdPayload) -> Self {
        let (kind, content_b64) = match &p.content {
            FdContent::Bytes(b) => (FdKind::Data, B64.encode(b)),
            FdContent::SeccompProgram => (FdKind::Seccomp, String::new()),
        };
        Self {
            fd: p.fd,
            target: p.target.clone(),
            kind,
            content_b64,
        }
    }

    /// The decoded content.
    pub fn content(&self) -> Result<Vec<u8>, PlanFileError> {
        B64.decode(&self.content_b64)
            .map_err(|e| PlanFileError::Invalid(format!("descriptor {}: {e}", self.fd)))
    }
}

/// Session paths (host).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPaths {
    /// Session log directory.
    #[serde(with = "codec::path")]
    pub log_dir: PathBuf,
    /// Session runtime directory (holds this file).
    #[serde(with = "codec::path")]
    pub runtime_dir: PathBuf,
    /// Pressure-vessel variable directory (Isolated mode).
    #[serde(default, with = "codec::opt_path")]
    pub session_var: Option<PathBuf>,
    /// The prefix lock (`None` for Provision).
    #[serde(default, with = "codec::opt_path")]
    pub prefix_lock: Option<PathBuf>,
}

/// An audit to schedule after the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditAfter {
    /// Variant whose store is audited.
    pub variant: SlrVariant,
}

/// Plan file errors.
#[derive(Debug, thiserror::Error)]
pub enum PlanFileError {
    /// A format this build does not know (`EP-SES-002`).
    #[error("unknown plan format {found} (this build writes {LAUNCH_PLAN_FORMAT})")]
    UnknownFormat {
        /// The file's format.
        found: u64,
    },
    /// Not valid JSON for the format.
    #[error("invalid plan file: {0}")]
    Invalid(String),
    /// Filesystem failure.
    #[error(transparent)]
    Fs(#[from] FsError),
}

impl From<PlanFileError> for Error {
    fn from(e: PlanFileError) -> Self {
        match e {
            PlanFileError::Fs(f) => f.into(),
            PlanFileError::UnknownFormat { found } => Error::new(ErrorCode::SesPlanVersionMismatch)
                .with("found", found)
                .with("expected", LAUNCH_PLAN_FORMAT),
            PlanFileError::Invalid(_) => {
                Error::new(ErrorCode::SesPlanVersionMismatch).with_source(e)
            }
        }
    }
}

/// Serialise a plan.
pub fn to_json(plan: &LaunchPlan) -> Result<Vec<u8>, PlanFileError> {
    let mut v =
        serde_json::to_vec_pretty(plan).map_err(|e| PlanFileError::Invalid(e.to_string()))?;
    v.push(b'\n');
    Ok(v)
}

/// Parse a plan, checking the format first so that an unknown version is reported as such
/// even when the rest of the document has changed shape.
pub fn from_json(bytes: &[u8]) -> Result<LaunchPlan, PlanFileError> {
    #[derive(Deserialize)]
    struct Head {
        format: Option<u64>,
    }
    let head: Head =
        serde_json::from_slice(bytes).map_err(|e| PlanFileError::Invalid(e.to_string()))?;
    match head.format {
        Some(f) if f == u64::from(LAUNCH_PLAN_FORMAT) => {}
        Some(found) => return Err(PlanFileError::UnknownFormat { found }),
        None => return Err(PlanFileError::Invalid("missing format".into())),
    }
    serde_json::from_slice(bytes).map_err(|e| PlanFileError::Invalid(e.to_string()))
}

/// Write `plan` to `path` atomically with mode 0600, creating the session runtime directory
/// (0700) if needed.
pub fn write(fs: &dyn Fs, path: &Path, plan: &LaunchPlan) -> Result<(), PlanFileError> {
    if let Some(dir) = path.parent() {
        fs.create_dir_all(dir, 0o700)?;
    }
    fs.atomic_write(path, &to_json(plan)?, PLAN_FILE_MODE)?;
    Ok(())
}

/// Read a plan back.
pub fn read(fs: &dyn Fs, path: &Path) -> Result<LaunchPlan, PlanFileError> {
    from_json(&fs.read(path, MAX_PLAN_BYTES)?)
}

// ---- argument grouping --------------------------------------------------------------------

/// Number of values each `bwrap` option takes (bubblewrap 0.8–0.13). Unknown options take
/// none.
pub fn option_arity(opt: &OsStr) -> usize {
    match opt.to_str().unwrap_or("") {
        "--overlay" => 3,
        "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try" | "--dev-bind"
        | "--dev-bind-try" | "--symlink" | "--setenv" | "--file" | "--bind-data"
        | "--ro-bind-data" | "--chmod" => 2,
        "--hostname" | "--perms" | "--size" | "--tmpfs" | "--dir" | "--proc" | "--dev"
        | "--mqueue" | "--chdir" | "--unsetenv" | "--json-status-fd" | "--sync-fd"
        | "--seccomp" | "--add-seccomp-fd" | "--info-fd" | "--block-fd" | "--userns-block-fd"
        | "--lock-file" | "--remount-ro" | "--overlay-src" | "--tmp-overlay" | "--ro-overlay"
        | "--uid" | "--gid" | "--cap-add" | "--cap-drop" | "--exec-label" | "--file-label"
        | "--args" | "--userns" | "--userns2" | "--pidns" | "--argv0" => 1,
        _ => 0,
    }
}

/// Split flat options into groups; `--perms` and `--size` join the option they modify.
pub fn group_args(args: &[OsString]) -> Vec<Vec<OsString>> {
    let mut out: Vec<Vec<OsString>> = Vec::new();
    let mut pending: Vec<OsString> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let n = option_arity(&args[i]);
        let end = (i + 1 + n).min(args.len());
        let group: Vec<OsString> = args[i..end].to_vec();
        let modifier = matches!(args[i].to_str(), Some("--perms" | "--size"));
        pending.extend(group);
        if !modifier {
            out.push(std::mem::take(&mut pending));
        }
        i = end;
    }
    if !pending.is_empty() {
        out.push(pending);
    }
    out
}

// ---- plan.txt -----------------------------------------------------------------------------

fn shown(a: &OsStr, r: &Redactor) -> String {
    let t = r.redact_str(&a.to_string_lossy());
    if t.is_empty() || t.contains(char::is_whitespace) || t.contains('"') {
        format!("{t:?}")
    } else {
        t
    }
}

/// `plan.txt`: session metadata, the `umu-run` command, the sandbox options one group per
/// line, the environment, and a mount table. Paths go through `redact`.
pub fn render_text(plan: &LaunchPlan, redact: &Redactor) -> String {
    let mut out = String::new();
    let s = &plan.session;
    let c = &plan.context;
    let _ = writeln!(out, "EasyPlay launch plan (format {})", plan.format);
    let _ = writeln!(out, "session      {}", s.id);
    let _ = writeln!(out, "kind         {}", s.kind.as_str());
    let _ = writeln!(out, "game         {}", redact.redact_str(&s.game_name));
    if let Some(g) = s.game_id {
        let _ = writeln!(out, "game id      {g}");
    }
    if let Some(e) = s.executable_id {
        let _ = writeln!(out, "executable   {e}");
    }
    let opt = |v: Option<String>| v.unwrap_or_else(|| "-".to_owned());
    let _ = writeln!(
        out,
        "preset       {}",
        opt(c.preset.map(|p| format!("{p:?}").to_lowercase()))
    );
    let _ = writeln!(
        out,
        "network      {}",
        opt(c.network.map(|n| format!("{n:?}").to_lowercase()))
    );
    let _ = writeln!(
        out,
        "runtime      {} ({}, {})",
        opt(c.runtime_name.as_ref().map(ToString::to_string)),
        opt(c.slr_variant.map(|v| v.as_str().to_owned())),
        opt(c.isolation.map(|i| i.as_str().to_owned()))
    );
    let _ = writeln!(out, "counted      {}", s.counted_eligible);
    let _ = writeln!(out, "stop grace   {} s", s.stop_grace_secs);
    let _ = writeln!(out, "bwrap        {}", redact.redact_path(&plan.bwrap));

    out.push_str("\n# command (inside the sandbox)\n");
    let cmd: Vec<String> = plan
        .sandbox
        .command
        .iter()
        .map(|a| shown(a, redact))
        .collect();
    let _ = writeln!(out, "{}", cmd.join(" "));
    if let Some(i) = plan.sandbox.command.iter().position(|a| a == "--") {
        let umu: Vec<String> = plan.sandbox.command[i + 1..]
            .iter()
            .map(|a| shown(a, redact))
            .collect();
        let _ = writeln!(out, "umu-run: {}", umu.join(" "));
    }

    let groups = group_args(&plan.sandbox.args);
    out.push_str("\n# sandbox options\n");
    for g in &groups {
        let line: Vec<String> = g.iter().map(|a| shown(a, redact)).collect();
        let _ = writeln!(out, "{}", line.join(" "));
    }

    out.push_str("\n# environment\n");
    for g in groups
        .iter()
        .filter(|g| g.first().is_some_and(|o| o == "--setenv"))
    {
        if let [_, name, value] = g.as_slice() {
            let _ = writeln!(
                out,
                "{}={}",
                name.to_string_lossy(),
                redact.redact_str(&value.to_string_lossy())
            );
        }
    }

    out.push_str("\n# mounts\n");
    let _ = writeln!(
        out,
        "{:<14} {:<4} {:<48} source",
        "kind", "mode", "destination"
    );
    for g in &groups {
        let core: Vec<&OsString> = g
            .iter()
            .skip_while(|a| *a == "--perms" || *a == "--size")
            .collect();
        let (kind, src, dst) = match core.as_slice() {
            [o, s, d] if o.to_str().is_some_and(|o| o.contains("bind")) => {
                (o.to_string_lossy(), Some(*s), *d)
            }
            [o, s, d] if *o == "--symlink" || *o == "--file" => (o.to_string_lossy(), Some(*s), *d),
            [o, d] if matches!(o.to_str(), Some("--tmpfs" | "--dir" | "--proc" | "--dev")) => {
                (o.to_string_lossy(), None, *d)
            }
            _ => continue,
        };
        let mode = if kind.starts_with("--ro") || kind == "--symlink" {
            "ro"
        } else if kind.starts_with("--bind") || kind.starts_with("--dev-bind") || kind == "--tmpfs"
        {
            "rw"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "{:<14} {:<4} {:<48} {}",
            kind.trim_start_matches("--"),
            mode,
            redact.redact_path(Path::new(dst)),
            src.map(|s| redact.redact_path(Path::new(s)))
                .unwrap_or_default()
        );
    }

    out.push_str("\n# descriptors\n");
    for fd in &plan.sandbox.fds {
        let _ = writeln!(
            out,
            "{} {:?} {}",
            fd.fd,
            fd.kind,
            fd.target
                .as_deref()
                .map(|t| redact.redact_path(t))
                .unwrap_or_default()
        );
    }
    out.push_str("\n# host actions\n");
    for a in &plan.host_actions {
        let line = serde_json::to_string(a).unwrap_or_default();
        let _ = writeln!(out, "{}", redact.redact_str(&line));
    }
    out
}
