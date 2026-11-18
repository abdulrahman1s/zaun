//! The log pump (PROC-004, PROCESS_LIFECYCLE §3.1, LOGGING §3.1): a thread that drains the
//! sandbox's merged stdout/stderr pipe, frames and sanitises lines, classifies them with
//! `umu::log_parse`, writes `game.log` (noise dropped, repetitions collapsed, size cap) and
//! `events.jsonl` (launch phases), and reports phases to the supervisor loop.
//!
//! The pipe is always drained, also after the cap and when the disk is full (writes then stop
//! after one warning in `supervisor.log`): the sandbox never blocks on its output. Memory is
//! bounded by the line cap of the splitter and the read buffer.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

use crate::clock::{Clock, MonoInstant};
use crate::consts::SESSION_LOG_CAP_DEFAULT;
use crate::logging::session_log::{
    FramedLine, LineSplitter, SessionLogConfig, SessionLogWriter, sanitize_line,
};
use crate::umu::{LineClass, LogClassifier, UmuFailure, UmuPhase};

/// Read buffer of the pump.
const READ_BUF: usize = 64 * 1024;

/// Where the pump writes.
#[derive(Clone, Debug)]
pub struct PumpConfig {
    /// `game.log`.
    pub game_log: PathBuf,
    /// `events.jsonl`.
    pub events_file: PathBuf,
    /// Byte cap of `game.log`.
    pub cap_bytes: u64,
    /// The runtime store (for the classifier's `ZN-SBX-030` mapping).
    pub runtime_store: Option<PathBuf>,
}

impl PumpConfig {
    /// Files in the session log directory `dir`, default cap.
    pub fn in_dir(dir: &std::path::Path) -> Self {
        Self {
            game_log: dir.join("game.log"),
            events_file: dir.join("events.jsonl"),
            cap_bytes: SESSION_LOG_CAP_DEFAULT,
            runtime_store: None,
        }
    }
}

/// What the pump tells the supervisor loop while running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PumpEvent {
    /// A launch phase, `elapsed` after spawn.
    Phase {
        /// Time since spawn.
        elapsed: Duration,
        /// The phase.
        phase: UmuPhase,
    },
}

/// What the pump found, at the end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PumpReport {
    /// The recognised umu/container failure.
    pub failure: Option<UmuFailure>,
    /// umu reported `Launching`.
    pub launching_seen: bool,
    /// Lines read.
    pub lines: u64,
    /// Bytes read.
    pub bytes: u64,
    /// `game.log` reached its cap.
    pub capped: bool,
}

/// A running pump.
#[derive(Debug)]
pub struct LogPump {
    handle: Option<JoinHandle<PumpReport>>,
    done: mpsc::Receiver<()>,
    sandbox_started: Arc<AtomicBool>,
}

#[derive(Serialize)]
struct EventLine<'a> {
    t_ms: u64,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<&'a UmuPhase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

struct Sinks {
    game: Option<SessionLogWriter<BufWriter<File>>>,
    events: Option<File>,
    warned: bool,
}

impl Sinks {
    fn warn_once(&mut self, what: &str, e: &io::Error) {
        if !self.warned {
            self.warned = true;
            tracing::warn!("session log {what} failed, further output is not written: {e}");
        }
    }

    fn event(&mut self, line: &EventLine<'_>) {
        let Some(f) = self.events.as_mut() else {
            return;
        };
        let mut text = serde_json::to_vec(line).unwrap_or_default();
        text.push(b'\n');
        if let Err(e) = f.write_all(&text) {
            self.events = None;
            self.warn_once("events.jsonl write", &e);
        }
    }
}

fn open_append(path: &std::path::Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(
        path,
        OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    Ok(File::from(fd))
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn pump(
    mut pipe: File,
    cfg: PumpConfig,
    clock: Arc<dyn Clock>,
    t0: MonoInstant,
    events: mpsc::Sender<PumpEvent>,
    sandbox_started: Arc<AtomicBool>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
) -> PumpReport {
    let mut sinks = Sinks {
        game: None,
        events: None,
        warned: false,
    };
    match open_append(&cfg.game_log) {
        Ok(f) => {
            sinks.game = Some(SessionLogWriter::new(
                BufWriter::new(f),
                SessionLogConfig {
                    cap_bytes: cfg.cap_bytes,
                    ..SessionLogConfig::default()
                },
            ));
        }
        Err(e) => sinks.warn_once("game.log open", &e),
    }
    match open_append(&cfg.events_file) {
        Ok(f) => sinks.events = Some(f),
        Err(e) => sinks.warn_once("events.jsonl open", &e),
    }
    let mut classifier = LogClassifier::new();
    if let Some(store) = &cfg.runtime_store {
        classifier = classifier.with_runtime_store(store);
    }
    let mut splitter = LineSplitter::new();
    let mut report = PumpReport::default();
    let mut started = false;
    let mut buf = vec![0u8; READ_BUF];

    let mut handle_line = |line: FramedLine, sinks: &mut Sinks, classifier: &mut LogClassifier| {
        report.lines += 1;
        if !started && sandbox_started.load(Ordering::SeqCst) {
            started = true;
            classifier.set_sandbox_started();
        }
        let elapsed = clock.mono().saturating_duration_since(t0);
        let text = sanitize_line(&line.bytes);
        let class = classifier.feed(&text);
        match &class {
            LineClass::Noise(_) => return,
            LineClass::Phase(phase) => {
                sinks.event(&EventLine {
                    t_ms: ms(elapsed),
                    kind: "phase",
                    phase: Some(phase),
                    code: None,
                });
                let _ = events.send(PumpEvent::Phase {
                    elapsed,
                    phase: phase.clone(),
                });
                if let Some(w) = &wake {
                    w();
                }
            }
            LineClass::UmuError { .. }
            | LineClass::ContainerError(_)
            | LineClass::InnerBwrapError(_)
            | LineClass::OuterBwrapError(_) => {
                let code = classifier.failure().map(|f| f.code.as_str());
                sinks.event(&EventLine {
                    t_ms: ms(elapsed),
                    kind: "error",
                    phase: None,
                    code,
                });
            }
            _ => {}
        }
        if let Some(w) = sinks.game.as_mut()
            && let Err(e) = w.write_line(elapsed, &line)
        {
            sinks.game = None;
            sinks.warn_once("game.log write", &e);
        }
    };

    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                report.bytes += n as u64;
                for line in splitter.push(&buf[..n]) {
                    handle_line(line, &mut sinks, &mut classifier);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                tracing::warn!("reading sandbox output failed: {e}");
                break;
            }
        }
    }
    if let Some(line) = splitter.finish() {
        handle_line(line, &mut sinks, &mut classifier);
    }
    report.failure = classifier.failure().cloned();
    report.launching_seen = classifier.launching_seen();
    if let Some(mut w) = sinks.game.take() {
        let elapsed = clock.mono().saturating_duration_since(t0);
        for s in classifier.noise_summary() {
            let _ = w.write_line(
                elapsed,
                &FramedLine {
                    bytes: s.into_bytes(),
                    truncated: false,
                },
            );
        }
        report.capped = w.is_capped();
        if let Err(e) = w.finish() {
            sinks.warn_once("game.log flush", &e);
        }
    }
    report
}

impl LogPump {
    /// Start draining `pipe` (the read end of the sandbox's stdout/stderr). `t0` is the
    /// spawn instant; `wake` is called after each phase event.
    pub fn start(
        pipe: OwnedFd,
        cfg: PumpConfig,
        clock: Arc<dyn Clock>,
        t0: MonoInstant,
        events: mpsc::Sender<PumpEvent>,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> io::Result<LogPump> {
        let file = File::from(pipe);
        let sandbox_started = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&sandbox_started);
        let (done_tx, done) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("log-pump".into())
            .spawn(move || {
                let r = pump(file, cfg, clock, t0, events, flag, wake);
                let _ = done_tx.send(());
                r
            })?;
        Ok(LogPump {
            handle: Some(handle),
            done,
            sandbox_started,
        })
    }

    /// bwrap reported its child: later `bwrap:` lines come from the inner container.
    pub fn set_sandbox_started(&self) {
        self.sandbox_started.store(true, Ordering::SeqCst);
    }

    /// Wait up to `timeout` for the pipe to reach end of file and return the report;
    /// `None` when something still holds the pipe open (the thread is then left behind).
    pub fn finish(mut self, timeout: Duration) -> Option<PumpReport> {
        match self.done.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.handle.take().and_then(|h| h.join().ok())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Test fixtures on a temporary directory.
mod tests {
    use super::*;
    use crate::clock::SystemClock;

    fn run(
        input: impl FnOnce(io::PipeWriter) + Send + 'static,
        cap: u64,
    ) -> (PumpReport, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let (r, w) = io::pipe().unwrap();
        let cfg = PumpConfig {
            cap_bytes: cap,
            ..PumpConfig::in_dir(dir.path())
        };
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let t0 = clock.mono();
        let (tx, rx) = mpsc::channel();
        let pump = LogPump::start(OwnedFd::from(r), cfg.clone(), clock, t0, tx, None).unwrap();
        let writer = std::thread::spawn(move || input(w));
        writer.join().unwrap();
        let report = pump.finish(Duration::from_secs(60)).unwrap();
        assert!(rx.try_iter().count() <= report.lines as usize);
        (
            report,
            std::fs::read_to_string(&cfg.game_log).unwrap(),
            std::fs::read_to_string(&cfg.events_file).unwrap(),
        )
    }

    #[test]
    fn phases_errors_and_noise() {
        let (rep, log, events) = run(
            |mut w| {
                w.write_all(
                    b"INFO: umu-launcher version 1.4.4 (3.14)\nhello \x1b[31mred\x1b[0m\n\
                      Traceback (most recent call last):\n  File \"x\", line 1\nRuntimeError: boom\npartial",
                )
                .unwrap();
            },
            1 << 20,
        );
        assert_eq!(rep.lines, 6);
        assert!(rep.failure.is_some());
        assert!(log.contains("hello red"), "{log}");
        assert!(log.contains("partial"));
        assert!(events.contains("\"kind\":\"phase\""), "{events}");
        assert!(events.contains("\"kind\":\"error\""), "{events}");
    }

    #[test]
    fn large_output_is_drained_with_a_capped_file() {
        // 64 MiB through the pipe with a 1 MiB cap: the writer never blocks for long and the
        // file stays at the cap (LOGGING §8; the 1 GiB variant runs in the lifecycle suite).
        let (rep, log, _) = run(
            |mut w| {
                let mut chunk = Vec::new();
                for i in 0..16_384u32 {
                    chunk.extend(format!("line {i:08} {}\n", "x".repeat(40)).as_bytes());
                }
                let mut sent = 0usize;
                while sent < 64 << 20 {
                    w.write_all(&chunk).unwrap();
                    sent += chunk.len();
                }
            },
            1 << 20,
        );
        assert!(rep.bytes >= 64 << 20);
        assert!(rep.capped);
        assert!(log.len() <= (1 << 20) + 200, "{}", log.len());
        assert!(log.contains("[log truncated at 1 MiB]"));
    }
}
