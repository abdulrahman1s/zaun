//! Console and rotating-file `tracing` layers (LOGGING §2).
//!
//! Line format: `timestamp level role[pid] target: message key=value…`, or JSON lines with
//! the same fields (`EASYPLAY_LOG_FORMAT=json`). The file layer writes through a non-blocking
//! bounded queue (8 192 events, lossy, with a once-per-second drop report) into a size-rotated
//! `O_APPEND` file shared by every EasyPlay process. `path` fields are redacted.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

use super::Redactor;
use crate::clock::{Clock, SystemClock};
use crate::consts::{
    APP_LOG_FILES, APP_LOG_MAX_BYTES, APP_LOG_QUEUE, CRASH_RING_LINES, LOG_LINE_MAX_BYTES,
};
use crate::fsx::{AppendFile, rotate_numbered};

/// Which EasyPlay process is logging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessRole {
    /// The GUI.
    Gui,
    /// A CLI client or headless command.
    Cli,
    /// A session supervisor.
    Supervisor,
}

impl ProcessRole {
    /// The short name used in log lines.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProcessRole::Gui => "gui",
            ProcessRole::Cli => "cli",
            ProcessRole::Supervisor => "sup",
        }
    }
}

/// Output format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// Text lines.
    #[default]
    Text,
    /// JSON lines.
    Json,
}

impl LogFormat {
    /// From the value of `EASYPLAY_LOG_FORMAT`.
    pub fn from_env_value(v: Option<&str>) -> Self {
        match v {
            Some(v) if v.eq_ignore_ascii_case("json") => LogFormat::Json,
            _ => LogFormat::Text,
        }
    }
}

/// Everything needed to install logging.
#[derive(Clone, Debug)]
pub struct LogOptions {
    /// Process role.
    pub role: ProcessRole,
    /// Preference `logging.level` (`error` … `trace`).
    pub preference_level: String,
    /// Value of `EASYPLAY_LOG`, if set.
    pub env_level: Option<String>,
    /// Number of `-v` flags.
    pub verbose: u8,
    /// Output format.
    pub format: LogFormat,
    /// Whether to log to stderr (a terminal, or `--verbose`).
    pub console: bool,
    /// The application log file; `None` disables the file layer.
    pub file: Option<PathBuf>,
    /// Rotation size.
    pub max_bytes: u64,
    /// Number of files kept.
    pub max_files: usize,
    /// Redaction for `path` fields.
    pub redactor: Redactor,
}

impl LogOptions {
    /// Defaults for a role: level `info`, text, no console, no file.
    pub fn new(role: ProcessRole) -> Self {
        Self {
            role,
            preference_level: "info".to_owned(),
            env_level: None,
            verbose: 0,
            format: LogFormat::Text,
            console: false,
            file: None,
            max_bytes: APP_LOG_MAX_BYTES,
            max_files: APP_LOG_FILES,
            redactor: Redactor::none(),
        }
    }
}

/// Why logging could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The log file could not be opened.
    #[error("cannot open log file {path}: {source}")]
    Open {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// The level directive is invalid.
    #[error("invalid log level directive {0:?}")]
    Directive(String),
    /// A global subscriber is already installed.
    #[error("logging is already initialised")]
    AlreadyInitialised,
    /// The reload handle is gone.
    #[error("cannot change the log level: {0}")]
    Reload(String),
}

/// The filter directive in effect: `-v` (debug) / `-vv` (trace) wins over `EASYPLAY_LOG`,
/// which wins over the preference (CONFIGURATION §1). Third-party crates are kept at `warn`
/// unless the directive names them.
pub fn effective_directive(preference: &str, env: Option<&str>, verbose: u8) -> String {
    let base = match verbose {
        0 => match env {
            Some(e) if !e.trim().is_empty() => e.trim().to_owned(),
            _ => {
                let p = preference.trim();
                if p.is_empty() {
                    "info".to_owned()
                } else {
                    p.to_owned()
                }
            }
        },
        1 => "debug".to_owned(),
        _ => "trace".to_owned(),
    };
    // A bare level applies to EasyPlay's crates; dependencies stay quiet.
    if is_bare_level(&base) {
        format!("warn,easyplay={base},easyplay_core={base},easyplay_ui={base}")
    } else {
        base
    }
}

fn is_bare_level(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "error" | "warn" | "info" | "debug" | "trace" | "off"
    )
}

fn parse_filter(directive: &str) -> Result<EnvFilter, LoggingError> {
    EnvFilter::try_new(directive).map_err(|_| LoggingError::Directive(directive.to_owned()))
}

/// The last application-log lines, kept for crash records (LOGGING §6).
#[derive(Clone, Debug, Default)]
pub struct RecentLines(Arc<Mutex<VecDeque<String>>>);

impl RecentLines {
    fn push(&self, line: String) {
        let mut q = self.0.lock();
        if q.len() >= CRASH_RING_LINES {
            q.pop_front();
        }
        q.push_back(line);
    }

    /// The buffered lines, oldest first.
    pub fn snapshot(&self) -> Vec<String> {
        self.0.lock().iter().cloned().collect()
    }
}

/// `MakeWriter` that appends each event to [`RecentLines`].
#[derive(Clone)]
struct RingMakeWriter(RecentLines);

struct RingWriter {
    ring: RecentLines,
    buf: Vec<u8>,
}

impl Write for RingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            let text = String::from_utf8_lossy(&self.buf);
            self.ring.push(text.trim_end_matches('\n').to_owned());
        }
    }
}

impl<'a> MakeWriter<'a> for RingMakeWriter {
    type Writer = RingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        RingWriter {
            ring: self.0.clone(),
            buf: Vec::new(),
        }
    }
}

/// A size-rotated application log file shared by several processes.
pub struct RotatingFile {
    file: AppendFile,
    max_bytes: u64,
    max_files: usize,
}

impl RotatingFile {
    /// Open (mode 0600) or create the log file.
    pub fn open(path: &Path, max_bytes: u64, max_files: usize) -> io::Result<Self> {
        Ok(Self {
            file: AppendFile::open(path, 0o600)?,
            max_bytes,
            max_files,
        })
    }

    fn rotate_if_needed(&mut self, incoming: usize) -> io::Result<()> {
        if !self.file.is_current() {
            self.file.reopen()?;
        }
        if self.file.len()? + incoming as u64 <= self.max_bytes {
            return Ok(());
        }
        let path = self.file.path().to_owned();
        let keep = self.max_files;
        let max = self.max_bytes;
        let file = &self.file;
        file.with_lock(|| {
            // Another process may have rotated while we waited for the lock.
            if file.is_current() && file.len()? + incoming as u64 > max {
                rotate_numbered(&path, keep)?;
            }
            Ok(())
        })?;
        self.file.reopen()
    }
}

/// Cap one record at [`LOG_LINE_MAX_BYTES`] (keeping it newline-terminated and valid UTF-8).
fn cap_record(buf: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if buf.len() <= LOG_LINE_MAX_BYTES {
        return std::borrow::Cow::Borrowed(buf);
    }
    let mut end = LOG_LINE_MAX_BYTES - 1;
    while end > 0 && (buf[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    let mut v = buf[..end].to_vec();
    v.push(b'\n');
    std::borrow::Cow::Owned(v)
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let record = cap_record(buf);
        self.rotate_if_needed(record.len())?;
        self.file.write_record(&record)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Our event format (text or JSON).
struct EasyPlayFormat {
    role: ProcessRole,
    pid: u32,
    format: LogFormat,
    redactor: Arc<Redactor>,
    clock: SystemClock,
}

/// Shorten `easyplay_core::launch::orchestrator` to `launch`.
fn short_target(target: &str) -> &str {
    for prefix in ["easyplay_core::", "easyplay_ui::", "easyplay::"] {
        if let Some(rest) = target.strip_prefix(prefix) {
            return rest.split("::").next().unwrap_or(rest);
        }
    }
    target
}

struct Fields<'a> {
    message: String,
    pairs: Vec<(&'static str, String)>,
    redactor: &'a Redactor,
}

impl Fields<'_> {
    fn add(&mut self, field: &Field, value: String) {
        let value = if field.name() == "path" {
            self.redactor.redact_str(&value)
        } else {
            value
        };
        if field.name() == "message" {
            self.message = value;
        } else {
            self.pairs.push((field.name(), value));
        }
    }
}

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, value.to_owned());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.add(field, format!("{value:?}"));
    }
}

fn quote_if_needed(v: &str) -> String {
    if v.is_empty() || v.chars().any(|c| c.is_whitespace() || c == '"' || c == '=') {
        format!("{v:?}")
    } else {
        v.to_owned()
    }
}

impl<S, N> FormatEvent<S, N> for EasyPlayFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut fields = Fields {
            message: String::new(),
            pairs: Vec::new(),
            redactor: &self.redactor,
        };
        event.record(&mut fields);
        let ts = self.clock.now().to_rfc3339();
        let target = short_target(meta.target());
        match self.format {
            LogFormat::Text => {
                let mut line = String::new();
                let _ = write!(
                    line,
                    "{ts} {:<5} {}[{}] {target}: {}",
                    meta.level().as_str(),
                    self.role.as_str(),
                    self.pid,
                    fields.message.replace('\n', " ")
                );
                for (k, v) in &fields.pairs {
                    let _ = write!(line, " {k}={}", quote_if_needed(&v.replace('\n', " ")));
                }
                writeln!(writer, "{line}")
            }
            LogFormat::Json => {
                let mut obj = serde_json::Map::new();
                obj.insert("ts".into(), ts.into());
                obj.insert("level".into(), meta.level().as_str().into());
                obj.insert("role".into(), self.role.as_str().into());
                obj.insert("pid".into(), self.pid.into());
                obj.insert("target".into(), target.into());
                obj.insert("message".into(), fields.message.into());
                let mut f = serde_json::Map::new();
                for (k, v) in fields.pairs {
                    f.insert(k.to_owned(), v.into());
                }
                obj.insert("fields".into(), f.into());
                writeln!(writer, "{}", serde_json::Value::Object(obj))
            }
        }
    }
}

fn formatter(opts: &LogOptions) -> EasyPlayFormat {
    EasyPlayFormat {
        role: opts.role,
        pid: std::process::id(),
        format: opts.format,
        redactor: Arc::new(opts.redactor.clone()),
        clock: SystemClock::new(),
    }
}

type ReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

/// Keeps logging alive; dropping it flushes the file writer.
pub struct LogGuard {
    _worker: Option<tracing_appender::non_blocking::WorkerGuard>,
    reload: ReloadHandle,
    recent: RecentLines,
    directive: Mutex<String>,
}

impl LogGuard {
    /// Change the level at run time (preference `logging.level` changed). The `-v` and
    /// `EASYPLAY_LOG` precedence is the caller's: pass the new effective directive.
    pub fn set_directive(&self, directive: &str) -> Result<(), LoggingError> {
        let filter = parse_filter(directive)?;
        self.reload
            .reload(filter)
            .map_err(|e| LoggingError::Reload(e.to_string()))?;
        *self.directive.lock() = directive.to_owned();
        Ok(())
    }

    /// The directive in effect.
    pub fn directive(&self) -> String {
        self.directive.lock().clone()
    }

    /// The last [`CRASH_RING_LINES`] lines.
    pub fn recent_lines(&self) -> Vec<String> {
        self.recent.snapshot()
    }
}

/// Install the global subscriber for this process.
pub fn init(opts: &LogOptions) -> Result<LogGuard, LoggingError> {
    let directive = effective_directive(
        &opts.preference_level,
        opts.env_level.as_deref(),
        opts.verbose,
    );
    let (filter_layer, reload) = reload::Layer::new(parse_filter(&directive)?);
    let recent = RecentLines::default();

    let ring_layer = tracing_subscriber::fmt::layer()
        .event_format(formatter(&LogOptions {
            format: LogFormat::Text,
            ..opts.clone()
        }))
        .with_writer(RingMakeWriter(recent.clone()));

    let console_layer = opts.console.then(|| {
        tracing_subscriber::fmt::layer()
            .event_format(formatter(opts))
            .with_writer(io::stderr)
    });

    let mut worker = None;
    let file_layer = match &opts.file {
        Some(path) => {
            let file =
                RotatingFile::open(path, opts.max_bytes, opts.max_files).map_err(|source| {
                    LoggingError::Open {
                        path: path.clone(),
                        source,
                    }
                })?;
            let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
                .buffered_lines_limit(APP_LOG_QUEUE)
                .lossy(true)
                .thread_name("log-writer")
                .finish(file);
            spawn_drop_reporter(writer.error_counter());
            worker = Some(guard);
            Some(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter(opts))
                    .with_writer(writer),
            )
        }
        None => None,
    };

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(ring_layer)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInitialised)?;

    Ok(LogGuard {
        _worker: worker,
        reload,
        recent,
        directive: Mutex::new(directive),
    })
}

fn spawn_drop_reporter(counter: tracing_appender::non_blocking::ErrorCounter) {
    let _ = std::thread::Builder::new()
        .name("log-drops".into())
        .spawn(move || {
            let mut reported = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let dropped = counter.dropped_lines();
                if dropped > reported {
                    tracing::warn!(
                        count = dropped - reported,
                        "log events dropped (queue full)"
                    );
                    reported = dropped;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]
    use super::*;

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl<'a> MakeWriter<'a> for Buf {
        type Writer = BufWriter;
        fn make_writer(&'a self) -> Self::Writer {
            BufWriter(self.0.clone())
        }
    }
    struct BufWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for BufWriter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn capture(format: LogFormat, f: impl FnOnce()) -> String {
        let buf = Buf::default();
        let mut opts = LogOptions::new(ProcessRole::Gui);
        opts.format = format;
        opts.redactor = Redactor::new(Some(Path::new("/home/sam")), Some("sam"), None);
        let sub = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(formatter(&opts))
                .with_writer(buf.clone()),
        );
        tracing::subscriber::with_default(sub, f);
        String::from_utf8(buf.0.lock().clone()).unwrap()
    }

    #[test]
    fn directive_precedence() {
        assert_eq!(
            effective_directive("info", None, 0),
            "warn,easyplay=info,easyplay_core=info,easyplay_ui=info"
        );
        assert!(effective_directive("info", Some("error"), 0).contains("easyplay_core=error"));
        assert!(effective_directive("info", Some("error"), 1).contains("easyplay_core=debug"));
        assert!(effective_directive("info", None, 2).contains("easyplay_core=trace"));
        assert_eq!(
            effective_directive("info", Some("easyplay_core::launch=trace"), 0),
            "easyplay_core::launch=trace"
        );
        assert!(effective_directive("", Some(" "), 0).contains("=info"));
        assert!(parse_filter("warn,easyplay_core=debug").is_ok());
        assert!(parse_filter("not a [directive").is_err());
    }

    #[test]
    fn text_line_format() {
        let out = capture(LogFormat::Text, || {
            tracing::info!(
                target: "easyplay_core::launch::orchestrator",
                game = "0192f0c2",
                state = "Spawning",
                path = "/home/sam/Games/x.exe",
                error = "decode: unexpected end",
                "state changed"
            );
        });
        let pid = std::process::id();
        let rest = out.split_once(' ').unwrap().1;
        assert_eq!(
            rest,
            format!(
                "INFO  gui[{pid}] launch: state changed game=0192f0c2 state=Spawning path=~/Games/x.exe error=\"decode: unexpected end\"\n"
            )
        );
        let ts = out.split_once(' ').unwrap().0;
        assert_eq!(ts.len(), "2026-11-12T21:04:11.203Z".len());
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn json_line_format() {
        let out = capture(LogFormat::Json, || {
            tracing::warn!(target: "easyplay_core::artwork", path = "/home/sam/a.png", "thumbnail failed");
        });
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["level"], "WARN");
        assert_eq!(v["role"], "gui");
        assert_eq!(v["target"], "artwork");
        assert_eq!(v["message"], "thumbnail failed");
        assert_eq!(v["fields"]["path"], "~/a.png");
    }

    #[test]
    fn short_targets() {
        assert_eq!(
            short_target("easyplay_core::session::supervisor"),
            "session"
        );
        assert_eq!(short_target("easyplay::main"), "main");
        assert_eq!(short_target("wgpu_core::device"), "wgpu_core::device");
    }

    #[test]
    fn rotation_at_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("easyplay.log");
        let mut f = RotatingFile::open(&p, 100, 3).unwrap();
        for i in 0..30 {
            f.write_all(format!("line {i:03} padding\n").as_bytes())
                .unwrap();
        }
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["easyplay.log", "easyplay.log.1", "easyplay.log.2"]);
        for n in &names {
            let len = std::fs::metadata(dir.path().join(n)).unwrap().len();
            assert!(len <= 100, "{n} {len}");
        }
        let current = std::fs::read_to_string(&p).unwrap();
        assert!(current.ends_with("line 029 padding\n"));
    }

    #[test]
    fn long_records_are_capped() {
        let rec = vec![b'x'; 10_000];
        assert_eq!(cap_record(&rec).len(), LOG_LINE_MAX_BYTES);
        assert!(cap_record(&rec).ends_with(b"\n"));
        let mut utf = "é".repeat(3000).into_bytes();
        utf.push(b'\n');
        let capped = cap_record(&utf);
        assert!(std::str::from_utf8(&capped).is_ok());
    }

    #[test]
    fn concurrent_writers_never_interleave_within_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("easyplay.log");
        let threads: Vec<_> = (0..4)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    // Separate descriptors, as separate processes would have.
                    let mut f = RotatingFile::open(&p, 1 << 30, 5).unwrap();
                    for i in 0..500 {
                        let line = format!("writer={t} seq={i:04} {}\n", "y".repeat(200));
                        f.write_all(line.as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2000);
        for l in lines {
            assert!(
                l.starts_with("writer=") && l.ends_with(&"y".repeat(200)),
                "{l}"
            );
        }
    }

    #[test]
    fn recent_lines_ring() {
        let ring = RecentLines::default();
        for i in 0..60 {
            ring.push(format!("{i}"));
        }
        let s = ring.snapshot();
        assert_eq!(s.len(), CRASH_RING_LINES);
        assert_eq!(s[0], "10");
        assert_eq!(s[49], "59");
    }
}
