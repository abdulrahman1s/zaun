//! Interpret sandbox output (UMU_INTEGRATION §6–7, LOGGING §3.1).
//!
//! [`sanitize_line`] frames and cleans raw bytes; [`classify`] maps one clean line to a
//! [`LineClass`] without state; [`LogClassifier`] adds what needs state (tracebacks, the
//! hard-link warning groups, whether the inner container has started, noise counters, the
//! last recognised error). Hand-written matchers, no regular expressions.
//!
//! Documented limits: a game can print lines that imitate umu's (`INFO: Using …`). Lines
//! are only taken as umu lines when they carry umu's level prefix; a game that prints that
//! exact prefix is misclassified as a phase line, which affects launch-progress display only:
//! phases never decide security-relevant behaviour, and the first `Launching` phase is the
//! only one that changes state.

use std::collections::VecDeque;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ErrorCode;
use crate::model::SessionKind;

/// Maximum line length kept; longer lines are cut and marked (LOGGING §3.1).
pub const MAX_LINE_BYTES: usize = 4096;
/// Marker appended to a cut line.
pub const TRUNCATED_MARK: &str = "…[truncated]";
/// Traceback lines kept as error details (§6 "last twenty lines").
pub const TRACEBACK_DETAIL_LINES: usize = 20;
/// Non-matching lines after which an unterminated traceback is abandoned.
pub const TRACEBACK_MAX_LINES: usize = 400;

// ---- sanitising ---------------------------------------------------------------------------

/// Clean one framed line: invalid UTF-8 → U+FFFD, ANSI escape sequences removed, other
/// control characters except tab → `·`, trailing `\r` dropped, cut at [`MAX_LINE_BYTES`]
/// with [`TRUNCATED_MARK`]. Returns the line and whether it was cut.
pub fn sanitize_line(raw: &[u8]) -> (String, bool) {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len().min(MAX_LINE_BYTES + 16));
    let mut chars = text.chars().peekable();
    let mut cut = false;
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            skip_escape(&mut chars);
            continue;
        }
        let c = if c.is_control() && c != '\t' { '·' } else { c };
        if out.len() + c.len_utf8() > MAX_LINE_BYTES {
            cut = true;
            break;
        }
        out.push(c);
    }
    if cut {
        out.push_str(TRUNCATED_MARK);
    }
    (out, cut)
}

/// Skip the rest of an escape sequence after ESC: CSI (`ESC [ … final`), OSC (`ESC ] … BEL`
/// or `ESC \`), or a two-character sequence.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        }
        Some(']') => {
            while let Some(c) = chars.next() {
                if c == '\u{7}' {
                    break;
                }
                if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        }
        _ => {}
    }
}

// ---- classes ------------------------------------------------------------------------------

/// A phase of umu's own setup and of Proton's start (§6).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UmuPhase {
    /// `umu-launcher version X`.
    Starting {
        /// umu version.
        version: String,
    },
    /// Downloading a Proton build.
    DownloadingProton {
        /// Archive or build name.
        name: String,
    },
    /// Downloading a runtime.
    DownloadingRuntime {
        /// Variant (`SteamLinuxRuntime_sniper.tar.xz`, …).
        variant: String,
        /// Version.
        version: String,
    },
    /// Resuming a parked download.
    ResumingDownload {
        /// Name.
        name: String,
    },
    /// A download's checksum matched.
    Verified {
        /// Name.
        name: String,
    },
    /// Extracting an archive.
    Extracting {
        /// Name.
        name: String,
    },
    /// Verifying the runtime tree.
    VerifyingRuntime,
    /// The runtime tree matched its manifest.
    RuntimeVerified,
    /// Something is current.
    UpToDate {
        /// Name.
        name: String,
    },
    /// The Proton build in use.
    UsingProton {
        /// Name.
        name: String,
    },
    /// umu's setup is done; the runtime starts the tool.
    Launching {
        /// Tool (`<proton>/proton` or the shim).
        tool: String,
        /// Runtime name.
        runtime: String,
    },
    /// Proton upgrades the prefix ("Updating private Windows storage…").
    UpgradingPrefix,
}

/// Level of a protonfixes line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixLevel {
    /// `INFO`.
    Info,
    /// `WARN`.
    Warn,
    /// `ERROR`.
    Error,
}

/// The Python exception (or logged error) behind a umu failure.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UmuErrorKind {
    /// `RuntimeError`.
    RuntimeError,
    /// `FileNotFoundError`.
    FileNotFound,
    /// `ValueError`.
    ValueError,
    /// `OSError` and its subclasses printed as `OSError`/`PermissionError`… with errno.
    OsError,
    /// Another exception class (name kept).
    Other(String),
    /// An `ERROR:` line logged by umu.
    Logged,
}

/// Noise dropped from the session log (counted).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseKind {
    /// pressure-vessel's hard-link fallback warning group (F-162).
    HardLinkFallback,
    /// `pv-locale-gen` chatter.
    LocaleGen,
}

impl NoiseKind {
    /// The summary label (`[8224 similar lines hidden: <label>]`).
    pub const fn label(self) -> &'static str {
        match self {
            NoiseKind::HardLinkFallback => "pressure-vessel hard-link fallback",
            NoiseKind::LocaleGen => "pressure-vessel locale generation",
        }
    }
}

/// The class of one line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineClass {
    /// A phase line.
    Phase(UmuPhase),
    /// A protonfixes line.
    Fix {
        /// Level.
        level: FixLevel,
        /// Message.
        text: String,
    },
    /// A Wine line (`wine:`, `err:`, `fixme:`).
    Wine(String),
    /// Another umu log line (`INFO:`/`WARNING:`/`DEBUG:` without a known pattern).
    UmuLog(String),
    /// `Traceback (most recent call last):`.
    TracebackStart,
    /// A line inside a traceback.
    TracebackLine,
    /// A umu failure.
    UmuError {
        /// Exception class or `Logged`.
        kind: UmuErrorKind,
        /// Message.
        message: String,
    },
    /// `pressure-vessel-wrap[N]: E: …`.
    ContainerError(String),
    /// `bwrap: …` after the sandbox reported its child: the inner container failed.
    InnerBwrapError(String),
    /// `bwrap: …` before that: the outer sandbox could not be set up.
    OuterBwrapError(String),
    /// Dropped noise.
    Noise(NoiseKind),
    /// Anything else (game output).
    Other,
}

/// Split umu's log prefix: optional `[module:line] `, then `LEVEL: `. Returns the level and
/// the message.
fn umu_level(line: &str) -> Option<(&str, &str)> {
    let mut rest = line;
    if let Some(r) = rest.strip_prefix('[')
        && let Some(end) = r.find("] ")
        && r[..end].contains(':')
        && !r[..end].contains(' ')
    {
        rest = &r[end + 2..];
    }
    for level in ["INFO", "WARNING", "ERROR", "DEBUG", "CRITICAL"] {
        if let Some(msg) = rest.strip_prefix(level).and_then(|m| m.strip_prefix(": ")) {
            return Some((level, msg));
        }
    }
    None
}

/// One whitespace-free token followed by `rest`.
fn token(s: &str) -> Option<(&str, &str)> {
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (end > 0).then(|| (&s[..end], &s[end..]))
}

fn whole_token(s: &str) -> Option<&str> {
    token(s).filter(|(_, rest)| rest.is_empty()).map(|(t, _)| t)
}

fn info_phase(msg: &str) -> Option<UmuPhase> {
    if let Some(r) = msg.strip_prefix("umu-launcher version ") {
        let (v, _) = token(r)?;
        return Some(UmuPhase::Starting {
            version: v.to_owned(),
        });
    }
    if let Some(r) = msg.strip_prefix("Downloading ") {
        if let Some(r2) = r.strip_suffix("), please wait...")
            && let Some((variant, rest)) = token(r2)
            && let Some(version) = rest.strip_prefix(" (").and_then(whole_token)
        {
            return Some(UmuPhase::DownloadingRuntime {
                variant: variant.to_owned(),
                version: version.to_owned(),
            });
        }
        if let Some(name) = r.strip_suffix("...").and_then(whole_token) {
            return Some(UmuPhase::DownloadingProton {
                name: name.to_owned(),
            });
        }
        return None;
    }
    if let Some(r) = msg.strip_prefix("Found '")
        && let Some(name) = r.strip_suffix("' in cache, resuming...")
    {
        return Some(UmuPhase::ResumingDownload {
            name: name.to_owned(),
        });
    }
    if let Some(r) = msg.strip_prefix("Extracting ")
        && let Some(name) = r.strip_suffix("...").and_then(whole_token)
    {
        return Some(UmuPhase::Extracting {
            name: name.to_owned(),
        });
    }
    if let Some(r) = msg.strip_prefix("Verifying integrity of ")
        && r.ends_with("...")
    {
        return Some(UmuPhase::VerifyingRuntime);
    }
    if let Some(r) = msg.strip_prefix("Using ")
        && let Some(name) = whole_token(r)
    {
        return Some(UmuPhase::UsingProton {
            name: name.to_owned(),
        });
    }
    if let Some(r) = msg.strip_prefix("Running '")
        && let Some((tool, runtime)) = r.split_once("' using runtime '")
        && let Some(runtime) = runtime.strip_suffix('\'')
    {
        return Some(UmuPhase::Launching {
            tool: tool.to_owned(),
            runtime: runtime.to_owned(),
        });
    }
    if let Some((name, rest)) = token(msg) {
        let name = name.strip_suffix(':');
        match (name, rest) {
            (Some(n), " SHA256 is OK" | " SHA512 is OK") => {
                return Some(UmuPhase::Verified { name: n.to_owned() });
            }
            (Some(_), " mtree is OK") => return Some(UmuPhase::RuntimeVerified),
            _ => {}
        }
        if rest == " is up to date" {
            return Some(UmuPhase::UpToDate {
                name: msg[..msg.len() - rest.len()].to_owned(),
            });
        }
    }
    None
}

/// `NAME[digits]: ` with the given program name; returns the rest.
fn program_prefix<'a>(line: &'a str, program: &str) -> Option<&'a str> {
    let r = line.strip_prefix(program)?.strip_prefix('[')?;
    let end = r.find("]: ")?;
    r[..end]
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| &r[end + 3..])
}

/// Python exception line: `Name: message` or `pkg.mod.Name: message`, where the class name
/// ends in `Error`, `Exception`, `Exit` or `Interrupt`, or `[Errno N]`-style `OSError`.
fn exception_line(line: &str) -> Option<(UmuErrorKind, String)> {
    let (head, message) = line.split_once(": ")?;
    let class = head.rsplit('.').next().unwrap_or(head);
    let ident_ok = !class.is_empty()
        && class
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && head
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        && class.bytes().next().is_some_and(|b| b.is_ascii_uppercase());
    let looks = ["Error", "Exception", "Exit", "Interrupt"]
        .iter()
        .any(|s| class.ends_with(s));
    if !(ident_ok && looks) {
        return None;
    }
    let kind = match class {
        "RuntimeError" => UmuErrorKind::RuntimeError,
        "FileNotFoundError" => UmuErrorKind::FileNotFound,
        "ValueError" => UmuErrorKind::ValueError,
        "OSError" | "PermissionError" | "IsADirectoryError" | "NotADirectoryError"
        | "FileExistsError" | "BlockingIOError" | "TimeoutError" | "ConnectionError" => {
            UmuErrorKind::OsError
        }
        other => UmuErrorKind::Other(other.to_owned()),
    };
    Some((kind, message.to_owned()))
}

/// Classify one sanitised line without context. Traceback continuation, the hard-link
/// groups and the inner/outer `bwrap` distinction need [`LogClassifier`]; here `bwrap:`
/// lines are reported as [`LineClass::OuterBwrapError`].
pub fn classify(line: &str) -> LineClass {
    if let Some((level, msg)) = umu_level(line) {
        return match level {
            "ERROR" | "CRITICAL" => LineClass::UmuError {
                kind: UmuErrorKind::Logged,
                message: msg.to_owned(),
            },
            "INFO" => info_phase(msg)
                .map(LineClass::Phase)
                .unwrap_or_else(|| LineClass::UmuLog(msg.to_owned())),
            _ => LineClass::UmuLog(msg.to_owned()),
        };
    }
    if line == "Traceback (most recent call last):" {
        return LineClass::TracebackStart;
    }
    if let Some(rest) = program_prefix(line, "pressure-vessel-wrap") {
        if let Some(e) = rest.strip_prefix("E: ") {
            return LineClass::ContainerError(e.to_owned());
        }
        if rest.starts_with("W: Unable to create hard link ") {
            return LineClass::Noise(NoiseKind::HardLinkFallback);
        }
        return LineClass::Other;
    }
    if line.starts_with("pv-locale-gen: ") || program_prefix(line, "pv-locale-gen").is_some() {
        return LineClass::Noise(NoiseKind::LocaleGen);
    }
    if let Some(r) = line.strip_prefix("ProtonFixes[")
        && let Some(end) = r.find("] ")
        && r[..end].bytes().all(|b| b.is_ascii_digit())
    {
        let r = &r[end + 2..];
        for (p, level) in [
            ("INFO: ", FixLevel::Info),
            ("WARN: ", FixLevel::Warn),
            ("ERROR: ", FixLevel::Error),
        ] {
            if let Some(text) = r.strip_prefix(p) {
                return LineClass::Fix {
                    level,
                    text: text.to_owned(),
                };
            }
        }
    }
    if let Some(r) = line.strip_prefix("Proton: Upgrading prefix from ")
        && r.contains(" to ")
    {
        return LineClass::Phase(UmuPhase::UpgradingPrefix);
    }
    if line.starts_with("wine: ") || wine_channel(line) {
        return LineClass::Wine(line.to_owned());
    }
    if let Some(r) = line.strip_prefix("bwrap: ") {
        return LineClass::OuterBwrapError(r.to_owned());
    }
    LineClass::Other
}

/// `err:`/`fixme:` prefixes, also in Wine's `<pid>:err:<channel>:` form.
fn wine_channel(line: &str) -> bool {
    let rest = match line.split_once(':') {
        Some((pid, rest))
            if !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_hexdigit()) && pid.len() <= 8 =>
        {
            rest
        }
        _ => line,
    };
    rest.starts_with("err:") || rest.starts_with("fixme:")
}

// ---- stateful classifier ------------------------------------------------------------------

/// A recognised failure, with its EasyPlay error code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UmuFailure {
    /// Code (§6 mapping).
    pub code: ErrorCode,
    /// Exception class or `Logged`.
    pub kind: UmuErrorKind,
    /// The message.
    pub message: String,
    /// Traceback lines (at most [`TRACEBACK_DETAIL_LINES`], newest last).
    pub details: Vec<String>,
}

/// Stateful classification of one session's output.
#[derive(Debug, Default)]
pub struct LogClassifier {
    in_traceback: bool,
    traceback: VecDeque<String>,
    traceback_other: usize,
    hard_link_followers: u8,
    sandbox_started: bool,
    launching_seen: bool,
    version: Option<String>,
    noise: [u64; 2],
    failure: Option<UmuFailure>,
    fix_needs_network: bool,
    runtime_store: Option<std::path::PathBuf>,
}

impl LogClassifier {
    /// A classifier for one session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Paths under this directory make `Read-only file system` an internal plan error
    /// (`EP-SBX-030`) rather than a generic umu error.
    #[must_use]
    pub fn with_runtime_store(mut self, store: &Path) -> Self {
        self.runtime_store = Some(store.to_path_buf());
        self
    }

    /// The bwrap status descriptor reported the sandbox's child: later `bwrap:` lines come
    /// from the inner container.
    pub fn set_sandbox_started(&mut self) {
        self.sandbox_started = true;
    }

    /// Classify the next line.
    pub fn feed(&mut self, line: &str) -> LineClass {
        if self.hard_link_followers > 0
            && let Some(rest) = program_prefix(line, "pressure-vessel-wrap")
            && rest.starts_with("W: ")
        {
            self.hard_link_followers -= 1;
            self.count(NoiseKind::HardLinkFallback);
            return LineClass::Noise(NoiseKind::HardLinkFallback);
        }
        if self.in_traceback {
            if line.starts_with(' ') || line.starts_with('\t') {
                self.push_traceback(line);
                return LineClass::TracebackLine;
            }
            if let Some((kind, message)) = exception_line(line) {
                self.push_traceback(line);
                self.in_traceback = false;
                self.record(kind.clone(), message.clone());
                return LineClass::UmuError { kind, message };
            }
            // Interleaved output of another process: classify it normally.
            self.traceback_other += 1;
            if self.traceback_other > TRACEBACK_MAX_LINES {
                self.in_traceback = false;
            }
        }
        let class = match classify(line) {
            LineClass::OuterBwrapError(t) if self.sandbox_started => LineClass::InnerBwrapError(t),
            c => c,
        };
        match &class {
            LineClass::TracebackStart => {
                self.in_traceback = true;
                self.traceback_other = 0;
                self.traceback.clear();
                self.push_traceback(line);
            }
            LineClass::Noise(k) => {
                if *k == NoiseKind::HardLinkFallback {
                    self.hard_link_followers = 2;
                }
                self.count(*k);
            }
            LineClass::Phase(UmuPhase::Starting { version }) => {
                self.version = Some(version.clone());
            }
            LineClass::Phase(UmuPhase::Launching { .. }) => self.launching_seen = true,
            LineClass::UmuError { kind, message } => self.record(kind.clone(), message.clone()),
            LineClass::Fix {
                level: FixLevel::Error | FixLevel::Warn,
                text,
            } if is_network_failure(text) => self.fix_needs_network = true,
            LineClass::ContainerError(text) | LineClass::InnerBwrapError(text) => {
                self.first_failure(ErrorCode::UmuContainerFailed, text);
            }
            LineClass::OuterBwrapError(text) => {
                self.first_failure(ErrorCode::SbxSetupFailed, text);
            }
            _ => {}
        }
        class
    }

    fn first_failure(&mut self, code: ErrorCode, text: &str) {
        if self.failure.is_none() {
            self.failure = Some(UmuFailure {
                code,
                kind: UmuErrorKind::Logged,
                message: text.to_owned(),
                details: Vec::new(),
            });
        }
    }

    fn push_traceback(&mut self, line: &str) {
        if self.traceback.len() == TRACEBACK_DETAIL_LINES {
            self.traceback.pop_front();
        }
        self.traceback.push_back(line.to_owned());
    }

    fn count(&mut self, k: NoiseKind) {
        self.noise[k as usize] += 1;
    }

    fn record(&mut self, kind: UmuErrorKind, message: String) {
        let code = map_message(&message, self.runtime_store.as_deref());
        let details = self.traceback.iter().cloned().collect();
        // The most specific failure wins; a later generic error does not replace a mapped
        // one.
        let replace = match &self.failure {
            None => true,
            Some(f) => {
                f.code == ErrorCode::UmuInternalError || f.code == ErrorCode::UmuContainerFailed
            }
        };
        if replace {
            self.failure = Some(UmuFailure {
                code,
                kind,
                message,
                details,
            });
        }
    }

    /// The umu version from the first line, if seen.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Whether umu reported the `Launching` phase (end of its own setup).
    pub fn launching_seen(&self) -> bool {
        self.launching_seen
    }

    /// The recognised failure, if any.
    pub fn failure(&self) -> Option<&UmuFailure> {
        self.failure.as_ref()
    }

    /// Whether game fixes failed for lack of network (prompt `FixNeedsNetwork`).
    pub fn fix_needs_network(&self) -> bool {
        self.fix_needs_network
    }

    /// Dropped-line counters, non-zero only.
    pub fn noise(&self) -> Vec<(NoiseKind, u64)> {
        [NoiseKind::HardLinkFallback, NoiseKind::LocaleGen]
            .into_iter()
            .map(|k| (k, self.noise[k as usize]))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// The summary lines written at session end (LOGGING §3.1).
    pub fn noise_summary(&self) -> Vec<String> {
        self.noise()
            .into_iter()
            .map(|(k, n)| format!("[{n} similar lines hidden: {}]", k.label()))
            .collect()
    }
}

/// Whether a protonfixes or winetricks message reports a download that failed for lack of
/// network.
pub fn is_network_failure(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "temporary failure in name resolution",
        "network is unreachable",
        "name or service not known",
        "urlopen error",
        "failed to download",
        "no internet",
        "connection refused",
        "max retries exceeded",
    ]
    .iter()
    .any(|p| t.contains(p))
}

// ---- error mapping (UMU-004) -------------------------------------------------------------

/// Map a recognised umu message to its EasyPlay error (§6 last table). `runtime_store` is
/// the host path of EasyPlay's runtime store; the sandbox shows it under the same name only
/// in Shared mode, so the check also accepts umu's in-sandbox location `.local/share/umu`.
pub fn map_message(message: &str, runtime_store: Option<&Path>) -> ErrorCode {
    let m = message;
    if m.contains("umu has not been setup for the user")
        || m.contains("An internet connection is required")
    {
        ErrorCode::RtNotInstalledOffline
    } else if m.contains("Environment variable not set or is empty: PROTONPATH") {
        ErrorCode::RtUnresolvable
    } else if m.contains("is not valid, toolmanifest.vdf not found") {
        ErrorCode::RtInvalidFolder
    } else if m.contains("Failed to match") && m.contains("with a container runtime") {
        ErrorCode::RtUnsupportedRuntime
    } else if m.contains("Digest mismatched") {
        ErrorCode::RtVerificationFailed
    } else if m.contains("Read-only file system")
        && (runtime_store.is_some_and(|s| m.contains(&*s.to_string_lossy()))
            || m.contains("/.local/share/umu/"))
    {
        ErrorCode::SbxRuntimeLayout
    } else if m.contains("This script should never be run as the root user") {
        ErrorCode::HostRunningAsRoot
    } else if m.contains("not designed to run on musl") {
        ErrorCode::HostMuslUnsupported
    } else {
        ErrorCode::UmuInternalError
    }
}

// ---- exit status (§7) ---------------------------------------------------------------------

/// What an exit status means (§7). The init shim reports `128 + n` for signal `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitMeaning {
    /// Status 0 of a Prepare or Provision session.
    Success,
    /// Status 0 of a game before any Wine process was seen: it exited immediately.
    ExitedImmediately,
    /// Normal exit of a running program with this code.
    Exited {
        /// Exit code.
        code: i32,
    },
    /// Status 1 with a recognised umu failure.
    LaunchFailed {
        /// Code from the failure.
        code: ErrorCode,
    },
    /// Status 1 without one before any Wine process: program not found by Proton, or it
    /// returned 1.
    ProgramDidNotStart,
    /// Runtime entry-point usage error or command not found (125–127).
    EntryPointFailed {
        /// Status.
        status: i32,
    },
    /// Killed by a crash signal (`SIGSEGV`, `SIGABRT`, `SIGILL`, `SIGBUS`, `SIGFPE`).
    Crashed {
        /// Signal number.
        signal: i32,
    },
    /// Stopped by a termination signal.
    Stopped {
        /// Signal number.
        signal: i32,
    },
}

/// Interpret the sandbox's exit status for a session of `kind`. `wine_seen`: a Wine process
/// of the session was observed; `failure`: what the classifier recognised.
pub fn interpret_exit(
    status: i32,
    kind: SessionKind,
    wine_seen: bool,
    failure: Option<&UmuFailure>,
) -> ExitMeaning {
    const CRASH: [i32; 5] = [4, 6, 7, 8, 11];
    if status > 128 && status < 128 + 65 {
        let signal = status - 128;
        return if CRASH.contains(&signal) {
            ExitMeaning::Crashed { signal }
        } else {
            ExitMeaning::Stopped { signal }
        };
    }
    match status {
        0 if matches!(kind, SessionKind::Prepare | SessionKind::Provision) => ExitMeaning::Success,
        0 if !wine_seen => ExitMeaning::ExitedImmediately,
        125..=127 if !wine_seen => ExitMeaning::EntryPointFailed { status },
        1 if !wine_seen => match failure {
            Some(f) => ExitMeaning::LaunchFailed { code: f.code },
            None => ExitMeaning::ProgramDidNotStart,
        },
        code => ExitMeaning::Exited { code },
    }
}

impl ExitMeaning {
    /// The error code a failed start is reported with, if this is one.
    pub fn failure_code(self) -> Option<ErrorCode> {
        match self {
            ExitMeaning::LaunchFailed { code } => Some(code),
            ExitMeaning::ProgramDidNotStart => Some(ErrorCode::UmuProgramDidNotStart),
            ExitMeaning::EntryPointFailed { .. } => Some(ErrorCode::UmuContainerFailed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitising() {
        assert_eq!(
            sanitize_line(b"\x1b[1;32mINFO:\x1b[0m hi\r\n"),
            ("INFO: hi".into(), false)
        );
        assert_eq!(sanitize_line(b"a\x07b\tc").0, "a·b\tc");
        assert_eq!(sanitize_line(b"\x1b]0;title\x07x").0, "x");
        assert_eq!(sanitize_line(b"\xff\xfe").0, "\u{fffd}\u{fffd}");
        let long = vec![b'x'; 5000];
        let (s, cut) = sanitize_line(&long);
        assert!(cut);
        assert_eq!(s.len(), MAX_LINE_BYTES + TRUNCATED_MARK.len());
        // Multi-byte characters are never split.
        let long: Vec<u8> = "é".repeat(3000).into_bytes();
        let (s, cut) = sanitize_line(&long);
        assert!(cut && s.ends_with(TRUNCATED_MARK));
    }

    fn phase(line: &str) -> Option<UmuPhase> {
        match classify(line) {
            LineClass::Phase(p) => Some(p),
            _ => None,
        }
    }

    #[test]
    fn phase_patterns() {
        let s = |x: &str| x.to_owned();
        let rows = [
            (
                "[umu.umu_run:781] INFO: umu-launcher version 1.4.4 (3.14.0 (main))",
                UmuPhase::Starting {
                    version: s("1.4.4"),
                },
            ),
            (
                "INFO: Downloading GE-Proton10-25.tar.gz...",
                UmuPhase::DownloadingProton {
                    name: s("GE-Proton10-25.tar.gz"),
                },
            ),
            (
                "INFO: Downloading SteamLinuxRuntime_sniper.tar.xz (3.0.20261110.268881), please wait...",
                UmuPhase::DownloadingRuntime {
                    variant: s("SteamLinuxRuntime_sniper.tar.xz"),
                    version: s("3.0.20261110.268881"),
                },
            ),
            (
                "INFO: Found 'GE-Proton10-25.tar.gz.parts' in cache, resuming...",
                UmuPhase::ResumingDownload {
                    name: s("GE-Proton10-25.tar.gz.parts"),
                },
            ),
            (
                "INFO: GE-Proton10-25.tar.gz: SHA512 is OK",
                UmuPhase::Verified {
                    name: s("GE-Proton10-25.tar.gz"),
                },
            ),
            (
                "INFO: SteamLinuxRuntime_sniper.tar.xz: SHA256 is OK",
                UmuPhase::Verified {
                    name: s("SteamLinuxRuntime_sniper.tar.xz"),
                },
            ),
            (
                "INFO: Extracting GE-Proton10-25.tar.gz...",
                UmuPhase::Extracting {
                    name: s("GE-Proton10-25.tar.gz"),
                },
            ),
            (
                "INFO: Verifying integrity of sniper_platform_3.0.20261110.268881...",
                UmuPhase::VerifyingRuntime,
            ),
            (
                "INFO: sniper_platform_3.0.20261110.268881: mtree is OK",
                UmuPhase::RuntimeVerified,
            ),
            (
                "INFO: steamrt3 is up to date",
                UmuPhase::UpToDate {
                    name: s("steamrt3"),
                },
            ),
            (
                "INFO: Using UMU-Proton-10.0-4",
                UmuPhase::UsingProton {
                    name: s("UMU-Proton-10.0-4"),
                },
            ),
            (
                "INFO: Running '/p/proton' using runtime 'steamrt3'",
                UmuPhase::Launching {
                    tool: s("/p/proton"),
                    runtime: s("steamrt3"),
                },
            ),
            (
                "Proton: Upgrading prefix from None to 10.0-104 (/pfx/)",
                UmuPhase::UpgradingPrefix,
            ),
        ];
        for (line, expected) in rows {
            assert_eq!(phase(line), Some(expected), "{line}");
        }
        assert_eq!(phase("INFO: Downloading two words..."), None);
        assert_eq!(
            phase("Using GE-Proton"),
            None,
            "no level prefix: game output"
        );
    }

    #[test]
    fn other_patterns() {
        assert_eq!(
            classify("ProtonFixes[1234] WARN: Skipping fix execution"),
            LineClass::Fix {
                level: FixLevel::Warn,
                text: "Skipping fix execution".into()
            }
        );
        assert!(matches!(
            classify("wine: created the configuration directory"),
            LineClass::Wine(_)
        ));
        assert!(matches!(
            classify("0024:err:module:foo"),
            LineClass::Wine(_)
        ));
        assert!(matches!(classify("fixme:ntdll:bar"), LineClass::Wine(_)));
        assert_eq!(
            classify("ERROR: Environment variable not set or is empty: PROTONPATH"),
            LineClass::UmuError {
                kind: UmuErrorKind::Logged,
                message: "Environment variable not set or is empty: PROTONPATH".into()
            }
        );
        assert_eq!(
            classify("pressure-vessel-wrap[42]: E: Failed to open"),
            LineClass::ContainerError("Failed to open".into())
        );
        assert_eq!(
            classify(
                "pressure-vessel-wrap[42]: W: Unable to create hard link \"a\" to \"b\": Invalid cross-device link"
            ),
            LineClass::Noise(NoiseKind::HardLinkFallback)
        );
        assert_eq!(
            classify("pv-locale-gen: generating en_US.UTF-8"),
            LineClass::Noise(NoiseKind::LocaleGen)
        );
        assert_eq!(classify("hello from the game"), LineClass::Other);
        assert_eq!(classify("[not a prefix] INFO: Using x"), LineClass::Other);
    }

    #[test]
    fn traceback_with_interleaving() {
        let mut c = LogClassifier::new();
        c.set_sandbox_started();
        let lines = [
            "Traceback (most recent call last):",
            "  File \"/usr/lib/umu/umu_run.py\", line 1, in <module>",
            "game says: Error: not a python line",
            "pressure-vessel-wrap[9]: W: Unable to create hard link x",
            "    main()",
            "OSError: [Errno 30] Read-only file system: '/home/sam/.local/share/umu/steamrt3/.installed.ok.tmp'",
        ];
        let classes: Vec<LineClass> = lines.iter().map(|l| c.feed(l)).collect();
        assert_eq!(classes[0], LineClass::TracebackStart);
        assert_eq!(classes[1], LineClass::TracebackLine);
        assert_eq!(classes[2], LineClass::Other);
        assert_eq!(classes[3], LineClass::Noise(NoiseKind::HardLinkFallback));
        assert_eq!(classes[4], LineClass::TracebackLine);
        assert!(matches!(
            &classes[5],
            LineClass::UmuError {
                kind: UmuErrorKind::OsError,
                ..
            }
        ));
        let f = c.failure().unwrap();
        assert_eq!(f.code, ErrorCode::SbxRuntimeLayout);
        assert_eq!(f.details.len(), 4);
        assert_eq!(
            c.feed("bwrap: Can't mount proc"),
            LineClass::InnerBwrapError("Can't mount proc".into())
        );
    }

    #[test]
    fn hard_link_groups_and_summary() {
        let mut c = LogClassifier::new();
        for _ in 0..3 {
            c.feed("pressure-vessel-wrap[9]: W: Unable to create hard link \"a\" to \"b\": Invalid cross-device link");
            c.feed("pressure-vessel-wrap[9]: W: Falling back to copying, but this will take more time and disk space.");
            c.feed("pressure-vessel-wrap[9]: W: For best results, the runtime should be on the same filesystem.");
        }
        assert_eq!(
            c.feed("pressure-vessel-wrap[9]: W: something else"),
            LineClass::Other
        );
        assert_eq!(c.noise(), vec![(NoiseKind::HardLinkFallback, 9)]);
        assert_eq!(
            c.noise_summary(),
            vec!["[9 similar lines hidden: pressure-vessel hard-link fallback]"]
        );
    }

    #[test]
    fn error_mapping_rows() {
        let rows = [
            (
                "umu has not been setup for the user",
                ErrorCode::RtNotInstalledOffline,
            ),
            (
                "An internet connection is required during initial setup",
                ErrorCode::RtNotInstalledOffline,
            ),
            (
                "Environment variable not set or is empty: PROTONPATH",
                ErrorCode::RtUnresolvable,
            ),
            (
                "Path '/x' is not valid, toolmanifest.vdf not found",
                ErrorCode::RtInvalidFolder,
            ),
            (
                "Failed to match '1391110' with a container runtime",
                ErrorCode::RtUnsupportedRuntime,
            ),
            (
                "Digest mismatched: GE-Proton10-25.tar.gz",
                ErrorCode::RtVerificationFailed,
            ),
            (
                "[Errno 30] Read-only file system: '/home/sam/.local/share/umu/steamrt3/x'",
                ErrorCode::SbxRuntimeLayout,
            ),
            (
                "[Errno 30] Read-only file system: '/mnt/x'",
                ErrorCode::UmuInternalError,
            ),
            (
                "This script should never be run as the root user",
                ErrorCode::HostRunningAsRoot,
            ),
            (
                "This script is not designed to run on musl-based systems",
                ErrorCode::HostMuslUnsupported,
            ),
            ("something else", ErrorCode::UmuInternalError),
        ];
        for (m, code) in rows {
            assert_eq!(map_message(m, None), code, "{m}");
        }
        assert_eq!(
            map_message(
                "Read-only file system: '/data/runtimes/umu/x'",
                Some(Path::new("/data/runtimes/umu"))
            ),
            ErrorCode::SbxRuntimeLayout
        );
    }

    #[test]
    fn exit_status_table() {
        use SessionKind::*;
        let fail = UmuFailure {
            code: ErrorCode::RtUnresolvable,
            kind: UmuErrorKind::Logged,
            message: String::new(),
            details: Vec::new(),
        };
        assert_eq!(
            interpret_exit(0, Prepare, false, None),
            ExitMeaning::Success
        );
        assert_eq!(
            interpret_exit(0, Game, false, None),
            ExitMeaning::ExitedImmediately
        );
        assert_eq!(
            interpret_exit(0, Game, true, None),
            ExitMeaning::Exited { code: 0 }
        );
        assert_eq!(
            interpret_exit(1, Game, false, Some(&fail)),
            ExitMeaning::LaunchFailed {
                code: ErrorCode::RtUnresolvable
            }
        );
        assert_eq!(
            interpret_exit(1, Game, false, None),
            ExitMeaning::ProgramDidNotStart
        );
        assert_eq!(
            interpret_exit(1, Game, true, None),
            ExitMeaning::Exited { code: 1 }
        );
        assert_eq!(
            interpret_exit(127, Game, false, None),
            ExitMeaning::EntryPointFailed { status: 127 }
        );
        assert_eq!(
            interpret_exit(139, Game, true, None),
            ExitMeaning::Crashed { signal: 11 }
        );
        assert_eq!(
            interpret_exit(134, Game, true, None),
            ExitMeaning::Crashed { signal: 6 }
        );
        assert_eq!(
            interpret_exit(143, Game, true, None),
            ExitMeaning::Stopped { signal: 15 }
        );
        assert_eq!(
            interpret_exit(137, Game, false, None),
            ExitMeaning::Stopped { signal: 9 }
        );
        assert_eq!(
            ExitMeaning::ProgramDidNotStart.failure_code(),
            Some(ErrorCode::UmuProgramDidNotStart)
        );
    }

    #[test]
    fn network_failures() {
        assert!(is_network_failure(
            "<urlopen error [Errno -3] Temporary failure in name resolution>"
        ));
        assert!(!is_network_failure("Applying fix for Game"));
        let mut c = LogClassifier::new();
        c.feed("ProtonFixes[7] WARN: Failed to download vcrun2019");
        assert!(c.fix_needs_network());
    }
}
