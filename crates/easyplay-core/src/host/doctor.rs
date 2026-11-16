//! Doctor checks that depend only on host facts (ERROR_HANDLING §6: D-01, D-02, D-04 … D-14).
//!
//! Placed in `host` for milestone M3 (the `diagnostics` module arrives with its own task);
//! `runtime::catalog_check` adds D-15. Each check has a stable ID, a status, a text key for
//! localisation, an English summary and an optional fix.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::facts::{HostFacts, HostHint, SessionType, UMU_VERIFIED_VERSION};
use crate::error::ErrorCode;
use crate::fsx::Fs;
use crate::paths::AppPaths;

/// Version of the `doctor --json` document.
pub const DOCTOR_FORMAT: u32 = 1;

/// Outcome of one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Fine.
    Pass,
    /// Works, with a caveat.
    Warn,
    /// Blocks launching.
    Fail,
    /// Not checked in this build.
    Skipped,
}

/// One doctor check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// `D-01` … `D-18`.
    pub id: String,
    /// Status.
    pub status: CheckStatus,
    /// Text key (`doctor.d04.fail`).
    pub key: String,
    /// English summary.
    pub summary: String,
    /// What to do, when something is wrong (a sentence).
    pub fix: Option<String>,
    /// A shell command that fixes it, shown separately for copying (`sudo sysctl -w …`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_command: Option<String>,
    /// Related error code.
    pub code: Option<ErrorCode>,
}

impl Check {
    /// A check result.
    pub fn new(id: &str, status: CheckStatus, summary: impl Into<String>) -> Self {
        let st = match status {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
            CheckStatus::Skipped => "skipped",
        };
        Self {
            id: id.to_owned(),
            status,
            key: format!("doctor.{}.{st}", id.to_ascii_lowercase().replace('-', "")),
            summary: summary.into(),
            fix: None,
            fix_command: None,
            code: None,
        }
    }

    /// Builder: fix text.
    #[must_use]
    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    /// Builder: error code.
    #[must_use]
    pub fn code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self
    }
}

/// The `doctor --json` document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DoctorReport {
    /// [`DOCTOR_FORMAT`].
    pub format: u32,
    /// EasyPlay version.
    pub version: String,
    /// Checks in ID order.
    pub checks: Vec<Check>,
    /// The facts the checks were computed from.
    pub facts: HostFacts,
}

impl DoctorReport {
    /// Whether any check failed.
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }
}

/// Inputs besides the facts.
pub struct DoctorContext<'a> {
    /// Filesystem.
    pub fs: &'a dyn Fs,
    /// EasyPlay's paths.
    pub paths: &'a AppPaths,
    /// The user ID.
    pub uid: u32,
    /// Candidate locations of `easyplay-init`.
    pub init_candidates: Vec<PathBuf>,
}

/// The shell command that applies a hint, if there is one.
pub fn fix_command(h: &HostHint) -> Option<String> {
    match h {
        HostHint::Sysctl { setting } => Some(format!("sudo sysctl -w {setting}")),
        HostHint::AppArmor { sysctl, .. } => Some(format!("sudo sysctl -w {sysctl}")),
        HostHint::SetuidBwrap | HostHint::UmuWrapper => None,
    }
}

fn hint_text(h: &HostHint) -> String {
    match h {
        HostHint::Sysctl { setting } => format!("Set the kernel option {setting} (sysctl)."),
        HostHint::AppArmor { sysctl, profile } => format!(
            "This system restricts sandboxes through AppArmor. Install the profile {profile}, or set {sysctl} (this relaxes the restriction for every program)."
        ),
        HostHint::SetuidBwrap => {
            "bubblewrap is installed setuid; install a build that uses user namespaces.".into()
        }
        HostHint::UmuWrapper => {
            "Point the setting paths.umu_run at the real umu-run program.".into()
        }
    }
}

fn d01(facts: &HostFacts, ctx: &DoctorContext<'_>) -> Check {
    let Some(dir) = &facts
        .display
        .xdg_runtime_dir
        .clone()
        .or_else(|| ctx.paths.xdg_runtime_dir.clone())
    else {
        return Check::new("D-01", CheckStatus::Fail, "XDG_RUNTIME_DIR is not set")
            .fix("Log in through a desktop session or systemd-logind.");
    };
    match ctx.fs.metadata(dir) {
        Ok(md) if md.is_dir() && md.uid == ctx.uid && md.mode & 0o077 == 0 => Check::new(
            "D-01",
            CheckStatus::Pass,
            format!("{} is private", dir.display()),
        ),
        Ok(_) => Check::new(
            "D-01",
            CheckStatus::Fail,
            format!("{} is not a private directory owned by you", dir.display()),
        )
        .fix("It must be a directory with mode 0700 owned by your user."),
        Err(_) => Check::new(
            "D-01",
            CheckStatus::Fail,
            format!("{} does not exist", dir.display()),
        ),
    }
}

fn d02(ctx: &DoctorContext<'_>) -> Check {
    let p = ctx.paths;
    let mut bad = Vec::new();
    for dir in [&p.config_dir, &p.data_dir, &p.cache_dir, &p.state_dir] {
        // The nearest existing ancestor decides whether EasyPlay can create the directory.
        let existing = dir
            .ancestors()
            .find(|a| ctx.fs.exists(a))
            .unwrap_or(Path::new("/"));
        let ok = ctx.fs.metadata_follow(existing).is_ok_and(|m| {
            m.is_dir() && ((m.uid == ctx.uid && m.mode & 0o200 != 0) || ctx.uid == 0)
        });
        if !ok {
            bad.push(dir.display().to_string());
        }
    }
    let free = ctx.fs.free_space(
        p.data_dir
            .ancestors()
            .find(|a| ctx.fs.exists(a))
            .unwrap_or(Path::new("/")),
    );
    if !bad.is_empty() {
        return Check::new(
            "D-02",
            CheckStatus::Fail,
            format!("Not writable: {}", bad.join(", ")),
        )
        .code(ErrorCode::FsWriteDenied);
    }
    match free {
        Ok(b) if b < 2_684_354_560 => Check::new(
            "D-02",
            CheckStatus::Warn,
            format!("Only {} MiB free for EasyPlay's data", b / (1024 * 1024)),
        )
        .fix("Compatibility layers need about 2.5 GiB; free some space."),
        Ok(b) => Check::new(
            "D-02",
            CheckStatus::Pass,
            format!("Folders writable; {} GiB free", b / (1024 * 1024 * 1024)),
        ),
        Err(_) => Check::new("D-02", CheckStatus::Pass, "Folders writable"),
    }
}

fn problem_check(
    facts: &HostFacts,
    id: &str,
    code: ErrorCode,
    ok: &str,
    fail_summary: &str,
) -> Check {
    match facts.problem(code) {
        None => Check::new(id, CheckStatus::Pass, ok),
        Some(p) => {
            let mut c = Check::new(id, CheckStatus::Fail, fail_summary).code(code);
            c.fix = p.hint.as_ref().map(hint_text);
            c.fix_command = p.hint.as_ref().and_then(fix_command);
            c.summary = format!("{fail_summary}: {}", p.detail.lines().last().unwrap_or(""));
            c
        }
    }
}

/// Every host-only check, in ID order.
pub fn host_checks(facts: &HostFacts, ctx: &DoctorContext<'_>) -> Vec<Check> {
    let mut out = vec![d01(facts, ctx), d02(ctx)];
    // D-03 needs the database itself: `diagnostics::doctor::database_check` replaces this.
    out.push(Check::new(
        "D-03",
        CheckStatus::Skipped,
        "The library database was not checked",
    ));

    // D-04 bubblewrap present with required options.
    out.push(match &facts.bwrap {
        None => Check::new(
            "D-04",
            CheckStatus::Fail,
            "bubblewrap (bwrap) is not installed",
        )
        .code(ErrorCode::HostBwrapNotFound)
        .fix("Install the bubblewrap package."),
        Some(b) => {
            let missing = b.missing_required();
            let v = b
                .version
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown version".into());
            if missing.is_empty() {
                Check::new(
                    "D-04",
                    CheckStatus::Pass,
                    format!("bubblewrap {v} at {}", b.path.display()),
                )
            } else {
                let names: Vec<&str> = missing.iter().map(|f| f.as_str()).collect();
                Check::new(
                    "D-04",
                    CheckStatus::Fail,
                    format!("bubblewrap {v} lacks {}", names.join(", ")),
                )
                .code(ErrorCode::HostBwrapMissingOption)
                .fix("Install bubblewrap 0.8 or newer.")
            }
        }
    });

    // D-05 user namespaces (setuid-only bwrap reported here too).
    out.push(if facts.bwrap.is_none() {
        Check::new("D-05", CheckStatus::Skipped, "bubblewrap missing")
    } else if facts.problem(ErrorCode::HostBwrapSetuidOnly).is_some() {
        problem_check(
            facts,
            "D-05",
            ErrorCode::HostBwrapSetuidOnly,
            "",
            "bubblewrap is setuid and user namespaces do not work",
        )
    } else {
        problem_check(
            facts,
            "D-05",
            ErrorCode::HostUsernsUnavailable,
            "User namespaces work",
            "User namespaces are unavailable",
        )
    });

    // D-06 nested user namespaces.
    out.push(match &facts.bwrap {
        Some(b) if b.userns_ok => problem_check(
            facts,
            "D-06",
            ErrorCode::HostNestedUsernsUnavailable,
            "Nested sandboxes work (needed by the Steam Linux Runtime)",
            "Nested sandboxes do not work",
        ),
        _ => Check::new("D-06", CheckStatus::Skipped, "User namespaces unavailable"),
    });

    // D-07 overlay (informational).
    out.push(if facts.overlay_ok() {
        Check::new(
            "D-07",
            CheckStatus::Pass,
            "Protected game folders are available",
        )
    } else {
        Check::new(
            "D-07",
            CheckStatus::Warn,
            "Protected game folders are not available on this system",
        )
    });

    // D-08 reflink (informational).
    out.push(match facts.fs.reflink_ok {
        Some(true) => Check::new(
            "D-08",
            CheckStatus::Pass,
            "The runtime store supports reflinks: each game gets a private runtime copy",
        ),
        Some(false) => Check::new(
            "D-08",
            CheckStatus::Warn,
            format!(
                "The runtime store ({}) does not support reflinks: games share the audited runtime",
                facts
                    .fs
                    .store_fs_type
                    .as_deref()
                    .unwrap_or("unknown filesystem")
            ),
        ),
        None => Check::new(
            "D-08",
            CheckStatus::Skipped,
            "The runtime store does not exist yet",
        ),
    });

    // D-09 umu-run.
    out.push(match &facts.umu {
        None => {
            let code = if facts.problem(ErrorCode::HostUmuClosureRejected).is_some() {
                ErrorCode::HostUmuClosureRejected
            } else {
                ErrorCode::HostUmuNotFound
            };
            let detail = facts.problem(code).map(|p| p.detail.clone()).unwrap_or_default();
            Check::new("D-09", CheckStatus::Fail, format!("umu-run is not usable: {detail}"))
                .code(code)
                .fix("Install umu-launcher 1.4 or newer.")
        }
        Some(u) => {
            let v = u.version.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into());
            if let Some(w) = u.wrapper.as_ref().filter(|w| w.breaks_nesting()) {
                let version_note = if u.version_accepted() {
                    String::new()
                } else {
                    "; its version could not be read through the wrapper".to_owned()
                };
                Check::new("D-09", CheckStatus::Warn, format!(
                    "umu-run at {} is a sandbox wrapper ({}){version_note}; it usually cannot run inside EasyPlay's sandbox",
                    u.path.display(),
                    w.chain.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> ")
                ))
                .fix(hint_text(&HostHint::UmuWrapper))
            } else if !u.version_accepted() {
                Check::new("D-09", CheckStatus::Fail, format!("umu-run {v} at {} is too old or did not report its version", u.path.display()))
                    .code(ErrorCode::HostUmuTooOld)
                    .fix("Install umu-launcher 1.4 or newer.")
            } else if u.version.is_some_and(|x| x > UMU_VERIFIED_VERSION) {
                Check::new("D-09", CheckStatus::Warn, format!("umu-run {v} is newer than the verified {UMU_VERIFIED_VERSION}"))
            } else {
                let mut extra = if u.closure.is_empty() {
                    String::new()
                } else {
                    format!("; also shows {} folder(s) to games", u.closure.len())
                };
                if u.wrapper.as_ref().is_some_and(|w| w.fhs_env) {
                    extra.push_str("; nixpkgs FHS environment (works inside the sandbox)");
                }
                Check::new("D-09", CheckStatus::Pass, format!("umu-run {v} at {}{extra}", u.path.display()))
            }
        }
    });

    // D-10 init shim: presence here; `diagnostics::doctor::init_check` runs it in a sandbox.
    out.push(
        match ctx
            .init_candidates
            .iter()
            .find(|p| super::exec::is_executable(ctx.fs, p))
        {
            Some(p) => Check::new(
                "D-10",
                CheckStatus::Pass,
                format!("easyplay-init at {}", p.display()),
            ),
            None => Check::new("D-10", CheckStatus::Fail, "easyplay-init was not found")
                .code(ErrorCode::HostInitMissing)
                .fix("Reinstall EasyPlay."),
        },
    );

    // D-11 display.
    let d = &facts.display;
    out.push(if !d.any() {
        Check::new(
            "D-11",
            CheckStatus::Fail,
            "No Wayland or X11 display socket was found",
        )
        .code(ErrorCode::HostNoDisplay)
    } else {
        let mut parts = Vec::new();
        if let Some(w) = &d.wayland {
            parts.push(format!("Wayland {}", w.name));
        }
        if let Some(x) = &d.x11 {
            parts.push(if x.filesystem_socket {
                format!("X11 :{}", x.number)
            } else {
                format!(
                    "X11 :{} (abstract socket only: not available to offline games)",
                    x.number
                )
            });
        }
        let status = if d.session_type == SessionType::X11
            || d.x11.as_ref().is_some_and(|x| !x.filesystem_socket)
        {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };
        let mut c = Check::new("D-11", status, parts.join(", "));
        if d.session_type == SessionType::X11 {
            c.fix = Some(
                "On X11 every program with display access can read keystrokes of other windows."
                    .into(),
            );
        }
        c
    });

    // D-12 GPU.
    let g = &facts.gpu;
    let render = g.render_nodes.iter().filter(|n| n.accessible).count();
    out.push(if render == 0 {
        Check::new("D-12", CheckStatus::Warn, "No accessible GPU render node")
            .code(ErrorCode::HostNoGpu)
    } else {
        let vendors: Vec<String> = g.vendors.iter().map(|v| format!("{v:?}")).collect();
        let b32 = match g.has_32bit_driver {
            Some(true) => "32-bit driver present",
            Some(false) => "no 32-bit driver (older games may not start)",
            None => "32-bit driver unknown",
        };
        let status = if g.has_32bit_driver == Some(false) {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };
        Check::new(
            "D-12",
            status,
            format!("{render} render node(s), {}; {b32}", vendors.join("/")),
        )
    });

    // D-13 audio.
    let a = &facts.audio;
    out.push(match (&a.pulse_socket, &a.pipewire_socket) {
        (None, None) => Check::new(
            "D-13",
            CheckStatus::Warn,
            "No PulseAudio or PipeWire socket: games will be silent",
        ),
        (p, w) => Check::new(
            "D-13",
            CheckStatus::Pass,
            format!(
                "{}; {}",
                format!(
                    "{}{}",
                    if p.is_some() { "PulseAudio " } else { "" },
                    if w.is_some() { "PipeWire" } else { "" }
                )
                .trim(),
                if a.playback_only() {
                    "the microphone can be withheld"
                } else {
                    "sound always includes the microphone"
                }
            ),
        ),
    });

    // D-14 controllers.
    let i = &facts.input;
    let pads = i
        .joysticks()
        .filter(|n| n.kind == super::facts::InputNodeKind::Event)
        .count();
    // Readable keyboards only rule out the opt-in "all input devices" mode; controllers work.
    out.push(if i.keyboards_readable {
        Check::new("D-14", CheckStatus::Pass, format!(
            "{pads} controller(s); \"all input devices\" mode is off because your account can read keyboards directly"
        ))
    } else {
        Check::new("D-14", CheckStatus::Pass, format!("{pads} controller(s); keyboards are not readable"))
    });

    out
}
