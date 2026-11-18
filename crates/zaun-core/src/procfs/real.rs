//! `/proc` reader and parsers.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{ProcEntry, ProcError, ProcSource, ProcState, ProcUsage};
use crate::fsx::{FileType, Fs, FsErrorKind};

/// Fields of `/proc/<pid>/stat` Zaun uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatFields {
    /// Field 1.
    pub pid: u32,
    /// Field 2 without parentheses (may contain spaces and parentheses).
    pub comm: String,
    /// Field 3.
    pub state: ProcState,
    /// Field 4.
    pub ppid: u32,
    /// Field 22.
    pub start_time_ticks: u64,
}

/// Parse `/proc/<pid>/stat`. `comm` is everything between the first `(` and the **last** `)`,
/// so names containing spaces or parentheses parse correctly.
pub fn parse_stat(text: &str) -> Option<StatFields> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let comm = text[open + 1..close].to_owned();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // rest[0] is field 3 (state); field n is rest[n - 3].
    let state = ProcState::from_char(rest.first()?.chars().next()?);
    let ppid = rest.get(1)?.parse().ok()?;
    let start_time_ticks = rest.get(19)?.parse().ok()?;
    Some(StatFields {
        pid,
        comm,
        state,
        ppid,
        start_time_ticks,
    })
}

/// Fields 14, 15 (`utime`, `stime`, clock ticks) and 24 (`rss`, pages) of
/// `/proc/<pid>/stat`.
pub fn parse_stat_usage(text: &str) -> Option<(u64, u64, u64)> {
    let close = text.rfind(')')?;
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let utime = rest.get(11)?.parse().ok()?;
    let stime = rest.get(12)?.parse().ok()?;
    let rss: i64 = rest.get(21)?.parse().ok()?;
    Some((utime, stime, u64::try_from(rss).unwrap_or(0)))
}

/// Ticks and pages to milliseconds and bytes.
pub fn usage_from(
    utime: u64,
    stime: u64,
    rss_pages: u64,
    ticks_per_s: u64,
    page: u64,
) -> ProcUsage {
    ProcUsage {
        cpu_ms: (utime + stime).saturating_mul(1000) / ticks_per_s.max(1),
        rss_bytes: rss_pages.saturating_mul(page),
    }
}

/// Fields of `/proc/<pid>/status` Zaun uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusFields {
    /// `Name:`.
    pub name: Option<String>,
    /// `Tgid:`.
    pub tgid: Option<u32>,
    /// `PPid:`.
    pub ppid: Option<u32>,
    /// Real UID from `Uid:`.
    pub uid: Option<u32>,
    /// `NSpid:` — the PID in each nested namespace, outermost first.
    pub nspid: Vec<u32>,
}

/// Parse `/proc/<pid>/status`.
pub fn parse_status(text: &str) -> StatusFields {
    let mut f = StatusFields::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Name" => f.name = Some(value.to_owned()),
            "Tgid" => f.tgid = value.parse().ok(),
            "PPid" => f.ppid = value.parse().ok(),
            "Uid" => f.uid = value.split_whitespace().next().and_then(|v| v.parse().ok()),
            "NSpid" => {
                f.nspid = value
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
            }
            _ => {}
        }
    }
    f
}

/// Parse a namespace link target such as `pid:[4026531836]`.
pub fn parse_ns_link(target: &str) -> Option<u64> {
    let inner = target.strip_prefix("pid:[")?.strip_suffix(']')?;
    inner.parse().ok()
}

const SMALL_FILE: u64 = 64 * 1024;
const CMDLINE_LIMIT: u64 = 256 * 1024;

/// Reads `/proc` (or another procfs root, for tests) through an [`Fs`].
#[derive(Clone)]
pub struct RealProc {
    fs: Arc<dyn Fs>,
    root: PathBuf,
}

impl RealProc {
    /// Read `/proc` through `fs`.
    pub fn new(fs: Arc<dyn Fs>) -> Self {
        Self::with_root(fs, "/proc")
    }

    /// Read a procfs-shaped tree at `root`.
    pub fn with_root(fs: Arc<dyn Fs>, root: impl Into<PathBuf>) -> Self {
        Self {
            fs,
            root: root.into(),
        }
    }

    fn pid_path(&self, pid: u32, file: &str) -> PathBuf {
        self.root.join(pid.to_string()).join(file)
    }

    /// Read a file; `Ok(None)` when the process is gone.
    fn read_opt(&self, path: &Path, limit: u64) -> Result<Option<Vec<u8>>, ProcError> {
        match self.fs.read(path, limit) {
            Ok(b) => Ok(Some(b)),
            Err(e) if matches!(e.kind, FsErrorKind::NotFound) => Ok(None),
            // ESRCH surfaces as Other on a vanished process; treat as gone.
            Err(e) if e.errno() == Some(3) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl ProcSource for RealProc {
    fn pids(&self) -> Result<Vec<u32>, ProcError> {
        let mut pids: Vec<u32> = self
            .fs
            .read_dir(&self.root)?
            .into_iter()
            .filter(|e| e.file_type == FileType::Dir)
            .filter_map(|e| e.name.to_str().and_then(|n| n.parse().ok()))
            .collect();
        pids.sort_unstable();
        Ok(pids)
    }

    fn entry(&self, pid: u32) -> Result<Option<ProcEntry>, ProcError> {
        let Some(stat) = self.read_opt(&self.pid_path(pid, "stat"), SMALL_FILE)? else {
            return Ok(None);
        };
        let stat_text = String::from_utf8_lossy(&stat);
        let fields = parse_stat(&stat_text).ok_or(ProcError::Parse { file: "stat" })?;
        let pid_ns = match self.fs.read_link(&self.pid_path(pid, "ns/pid")) {
            Ok(target) => target.to_str().and_then(parse_ns_link),
            Err(_) => None,
        };
        Ok(Some(ProcEntry {
            pid: fields.pid,
            ppid: fields.ppid,
            exe_name: Some(fields.comm.clone()),
            comm: fields.comm,
            start_time_ticks: fields.start_time_ticks,
            pid_ns,
            state: fields.state,
        }))
    }

    fn cmdline(&self, pid: u32) -> Result<Option<Vec<OsString>>, ProcError> {
        let Some(raw) = self.read_opt(&self.pid_path(pid, "cmdline"), CMDLINE_LIMIT)? else {
            return Ok(None);
        };
        let mut args: Vec<OsString> = raw
            .split(|b| *b == 0)
            .map(|a| OsString::from_vec(a.to_vec()))
            .collect();
        if raw.last() == Some(&0) {
            args.pop();
        }
        if raw.is_empty() {
            args.clear();
        }
        Ok(Some(args))
    }

    fn usage(&self, pid: u32) -> Result<Option<ProcUsage>, ProcError> {
        let Some(stat) = self.read_opt(&self.pid_path(pid, "stat"), SMALL_FILE)? else {
            return Ok(None);
        };
        let (u, s, rss) = parse_stat_usage(&String::from_utf8_lossy(&stat))
            .ok_or(ProcError::Parse { file: "stat" })?;
        let ticks = rustix::param::clock_ticks_per_second();
        let page = rustix::param::page_size() as u64;
        Ok(Some(usage_from(u, s, rss, ticks, page)))
    }

    fn boot_id(&self) -> Result<String, ProcError> {
        let raw = self
            .fs
            .read(&self.root.join("sys/kernel/random/boot_id"), 256)?;
        let id = String::from_utf8_lossy(&raw).trim().to_owned();
        if id.is_empty() {
            return Err(ProcError::Parse { file: "boot_id" });
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::RealFs;

    const STAT_PLAIN: &str = "4182 (zaun) S 1 4182 4182 0 -1 4194560 3301 0 0 0 52 11 0 0 20 0 9 0 123456 1234567890 12345 18446744073709551615 1 1 0 0 0 0 0 4096 0 0 0 0 17 3 0 0 0 0 0";
    const STAT_TRICKY: &str =
        "777 (Game (x) ) Z 42 777 777 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 999 0 0 0";

    #[test]
    fn stat_parsing() {
        let s = parse_stat(STAT_PLAIN).unwrap();
        assert_eq!(s.pid, 4182);
        assert_eq!(s.comm, "zaun");
        assert_eq!(s.state, ProcState::Sleeping);
        assert_eq!(s.ppid, 1);
        assert_eq!(s.start_time_ticks, 123_456);
        let t = parse_stat(STAT_TRICKY).unwrap();
        assert_eq!(t.comm, "Game (x) ");
        assert_eq!(t.state, ProcState::Zombie);
        assert_eq!(t.ppid, 42);
        assert_eq!(t.start_time_ticks, 999);
        assert!(parse_stat("garbage").is_none());
        assert!(parse_stat("1 (x) S 1").is_none());
    }

    #[test]
    fn usage_parsing() {
        // utime 52, stime 11, rss 12345 pages.
        assert_eq!(parse_stat_usage(STAT_PLAIN), Some((52, 11, 12_345)));
        let u = usage_from(52, 11, 12_345, 100, 4096);
        assert_eq!(u.cpu_ms, 630);
        assert_eq!(u.rss_bytes, 12_345 * 4096);
        assert_eq!(parse_stat_usage("1 (x) S 1"), None);
    }

    #[test]
    fn status_parsing() {
        let s = parse_status(
            "Name:\twine64-preloader\nUmask:\t0022\nState:\tS (sleeping)\nTgid:\t4195\nNgid:\t0\nPid:\t4195\nPPid:\t4190\nUid:\t1000\t1000\t1000\t1000\nNSpid:\t4195\t2\t17\n",
        );
        assert_eq!(s.name.as_deref(), Some("wine64-preloader"));
        assert_eq!(s.tgid, Some(4195));
        assert_eq!(s.ppid, Some(4190));
        assert_eq!(s.uid, Some(1000));
        assert_eq!(s.nspid, vec![4195, 2, 17]);
    }

    #[test]
    fn ns_link() {
        assert_eq!(parse_ns_link("pid:[4026531836]"), Some(4_026_531_836));
        assert_eq!(parse_ns_link("net:[4026531836]"), None);
        assert_eq!(parse_ns_link("pid:[x]"), None);
    }

    #[test]
    fn reads_own_process() {
        let p = RealProc::new(Arc::new(RealFs::new()));
        let me = std::process::id();
        let entry = p.entry(me).unwrap().unwrap();
        assert_eq!(entry.pid, me);
        assert!(entry.start_time_ticks > 0);
        assert!(entry.pid_ns.is_some());
        assert!(p.is_alive(entry.identity()).unwrap());
        // A different start time is a different process (PID reuse).
        let mut reused = entry.identity();
        reused.start_time_ticks += 1;
        assert!(!p.is_alive(reused).unwrap());
        assert!(p.pids().unwrap().contains(&me));
        let cmd = p.cmdline(me).unwrap().unwrap();
        assert!(!cmd.is_empty());
        assert_eq!(p.boot_id().unwrap().len(), 36);
        assert_eq!(p.entry(u32::MAX - 1).unwrap(), None);
    }
}
