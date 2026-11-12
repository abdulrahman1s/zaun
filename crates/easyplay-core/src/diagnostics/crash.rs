//! Crash records (LOGGING §6, ERROR_HANDLING §4.11).
//!
//! The panic hook of a GUI or CLI process writes `<state>/crash/<timestamp>-<role>.json`:
//! version, commit, role, thread, panic message, backtrace, the last application-log lines,
//! how long the process had been running and what it was doing ([`CrashPhase`]). Everything
//! is redacted before it is written. On the next start [`pending`] returns the newest record
//! not yet acknowledged (the banner "EasyPlay closed unexpectedly last time"), and
//! [`start_advice`] applies the two heuristics of §4.11: reset the window geometry when the
//! last two runs crashed within 5 s of starting, and start with the `glow` renderer when the
//! last crash happened while the graphics were initialised. Records older than 30 days are
//! pruned.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::clock::Timestamp;
use crate::fsx::{Fs, FsErrorKind};
use crate::logging::Redactor;

/// Version of the record format.
pub const CRASH_FORMAT: u32 = 1;
/// Records are kept this long.
pub const CRASH_RETENTION: Duration = Duration::from_secs(30 * 24 * 3600);
/// A crash this soon after start counts as a start-up crash.
pub const EARLY_CRASH: Duration = Duration::from_secs(5);
/// Largest record read back.
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
/// The acknowledgement file (holds the file name of the newest record the user has seen).
const ACK_FILE: &str = "acknowledged";

/// What the process was doing when it panicked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashPhase {
    /// Before the window or the command did its work.
    #[default]
    Startup,
    /// While the renderer (wgpu or glow) was being set up.
    Graphics,
    /// Normal operation.
    Running,
}

/// One crash record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecord {
    /// [`CRASH_FORMAT`].
    pub format: u32,
    /// When it happened.
    pub at: Timestamp,
    /// EasyPlay version.
    pub version: String,
    /// Commit the binary was built from.
    pub commit: String,
    /// `gui`, `cli` or `sup`.
    pub role: String,
    /// The panicking thread's name.
    pub thread: String,
    /// The panic message (redacted).
    pub message: String,
    /// Where it panicked (`file:line`).
    pub location: Option<String>,
    /// The backtrace (redacted).
    pub backtrace: String,
    /// Milliseconds since the process started.
    pub uptime_ms: u64,
    /// What the process was doing.
    pub phase: CrashPhase,
    /// The last application-log lines (redacted).
    pub log_tail: Vec<String>,
}

impl CrashRecord {
    /// The file name of this record.
    pub fn file_name(&self) -> String {
        let t = jiff::Timestamp::from_millisecond(self.at.as_millis())
            .map(|t| {
                t.to_zoned(jiff::tz::TimeZone::UTC)
                    .strftime("%Y%m%dT%H%M%S%3fZ")
                    .to_string()
            })
            .unwrap_or_else(|_| self.at.as_millis().to_string());
        let role: String = self
            .role
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        format!("{t}-{role}.json")
    }

    /// Whether it happened within [`EARLY_CRASH`] of start.
    pub fn early(&self) -> bool {
        u128::from(self.uptime_ms) < EARLY_CRASH.as_millis()
    }

    /// Redact every free-text field.
    #[must_use]
    pub fn redacted(mut self, r: &Redactor) -> Self {
        self.message = r.redact_str(&self.message);
        self.backtrace = r.redact_str(&self.backtrace);
        self.location = self.location.map(|l| r.redact_str(&l));
        self.log_tail = self.log_tail.iter().map(|l| r.redact_str(l)).collect();
        self
    }
}

/// Write `record` (already redacted) to `dir` with mode 0600; returns the path.
pub fn write(fs: &dyn Fs, dir: &Path, record: &CrashRecord) -> std::io::Result<PathBuf> {
    fs.create_dir_all(dir, 0o700)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let path = dir.join(record.file_name());
    let json = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    fs.atomic_write(&path, &json, 0o600)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(path)
}

/// Every readable record in `dir`, newest first (unreadable files are skipped).
pub fn list(fs: &dyn Fs, dir: &Path) -> Vec<(PathBuf, CrashRecord)> {
    let Ok(entries) = fs.read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, CrashRecord)> = entries
        .into_iter()
        .filter(|e| e.name.to_str().is_some_and(|n| n.ends_with(".json")))
        .filter_map(|e| {
            let bytes = fs.read(&e.path, MAX_RECORD_BYTES).ok()?;
            let rec: CrashRecord = serde_json::from_slice(&bytes).ok()?;
            Some((e.path, rec))
        })
        .collect();
    out.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| b.0.cmp(&a.0)));
    out
}

/// The records newer than the acknowledged one, newest first: the crashes since the user
/// last saw the banner (and since the start heuristics last applied).
pub fn unacknowledged(fs: &dyn Fs, dir: &Path) -> Vec<(PathBuf, CrashRecord)> {
    let acked = fs
        .read(&dir.join(ACK_FILE), 4096)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned());
    list(fs, dir)
        .into_iter()
        .take_while(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned()) != acked)
        .collect()
}

/// The newest record the user has not acknowledged yet (the next-start banner).
pub fn pending(fs: &dyn Fs, dir: &Path) -> Option<(PathBuf, CrashRecord)> {
    unacknowledged(fs, dir).into_iter().next()
}

/// Mark the newest record as seen (dismissing the banner).
pub fn acknowledge(fs: &dyn Fs, dir: &Path) -> std::io::Result<()> {
    let Some((path, _)) = list(fs, dir).into_iter().next() else {
        return Ok(());
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    fs.atomic_write(&dir.join(ACK_FILE), name.as_bytes(), 0o600)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Delete records older than [`CRASH_RETENTION`]; returns how many were removed.
pub fn prune(fs: &dyn Fs, dir: &Path, now: Timestamp) -> usize {
    let cutoff = now.saturating_sub(CRASH_RETENTION);
    list(fs, dir)
        .into_iter()
        .filter(|(_, r)| r.at < cutoff)
        .filter(|(p, _)| match fs.remove_file(p) {
            Ok(()) => true,
            Err(e) => e.kind == FsErrorKind::NotFound,
        })
        .count()
}

/// What the next GUI start does differently after crashes (ERROR_HANDLING §4.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartAdvice {
    /// Ignore the saved window geometry (two start-up crashes in a row).
    pub reset_geometry: bool,
    /// Start with the `glow` renderer (the last crash was in graphics initialisation).
    pub prefer_glow: bool,
}

/// The heuristics over the GUI's unacknowledged records, newest first: once the user has
/// seen the banner (or a later start went well enough to show it), the saved state is
/// trusted again.
pub fn start_advice(records: &[CrashRecord]) -> StartAdvice {
    let gui: Vec<&CrashRecord> = records.iter().filter(|r| r.role == "gui").collect();
    StartAdvice {
        reset_geometry: gui.len() >= 2 && gui[0].early() && gui[1].early(),
        prefer_glow: gui.first().is_some_and(|r| r.phase == CrashPhase::Graphics),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::RealFs;

    fn rec(at: i64, uptime_ms: u64, phase: CrashPhase) -> CrashRecord {
        CrashRecord {
            format: CRASH_FORMAT,
            at: Timestamp::from_millis(at),
            version: "0.1.0".into(),
            commit: "abc".into(),
            role: "gui".into(),
            thread: "main".into(),
            message: "boom at /home/sam/x".into(),
            location: Some("src/x.rs:1".into()),
            backtrace: "0: /home/sam/src".into(),
            uptime_ms,
            phase,
            log_tail: vec!["sam opened /home/sam/Games".into()],
        }
    }

    #[test]
    fn write_list_pending_acknowledge_prune() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFs::new();
        let d = dir.path().join("crash");
        assert!(pending(&fs, &d).is_none());
        let r = Redactor::new(Some(Path::new("/home/sam")), Some("sam"), None);
        let a = rec(1_700_000_000_000, 60_000, CrashPhase::Running).redacted(&r);
        assert!(!a.message.contains("/home/sam") && a.message.contains('~'));
        assert!(!a.log_tail[0].contains("/home/sam"), "{:?}", a.log_tail);
        let pa = write(&fs, &d, &a).unwrap();
        let md = fs.metadata(&pa).unwrap();
        assert_eq!(md.mode & 0o777, 0o600);
        let b = rec(1_700_000_100_000, 1_000, CrashPhase::Graphics);
        write(&fs, &d, &b).unwrap();
        let (_, newest) = pending(&fs, &d).unwrap();
        assert_eq!(newest, b);
        acknowledge(&fs, &d).unwrap();
        assert!(pending(&fs, &d).is_none());
        assert!(unacknowledged(&fs, &d).is_empty());
        let c = rec(1_700_000_200_000, 1_000, CrashPhase::Startup);
        write(&fs, &d, &c).unwrap();
        assert_eq!(pending(&fs, &d).unwrap().1, c);
        assert_eq!(unacknowledged(&fs, &d).len(), 1);
        // Garbage is ignored.
        fs.atomic_write(&d.join("x.json"), b"{", 0o600).unwrap();
        assert_eq!(list(&fs, &d).len(), 3);
        let now = Timestamp::from_millis(1_700_000_150_000).saturating_add(CRASH_RETENTION);
        assert_eq!(prune(&fs, &d, now), 2);
        assert_eq!(list(&fs, &d).len(), 1);
    }

    #[test]
    fn start_advice_heuristics() {
        let early = rec(2, 1_000, CrashPhase::Startup);
        let late = rec(1, 600_000, CrashPhase::Running);
        assert_eq!(start_advice(&[]), StartAdvice::default());
        assert!(!start_advice(&[early.clone(), late.clone()]).reset_geometry);
        assert!(start_advice(&[early.clone(), early.clone()]).reset_geometry);
        let gfx = rec(3, 500, CrashPhase::Graphics);
        let a = start_advice(&[gfx, late.clone()]);
        assert!(a.prefer_glow && !a.reset_geometry);
        let mut cli = early.clone();
        cli.role = "cli".into();
        assert!(!start_advice(&[cli, early]).reset_geometry);
    }
}
