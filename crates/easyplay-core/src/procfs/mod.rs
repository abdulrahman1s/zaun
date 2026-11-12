//! Read process information for tracking and the debug process tree.
//!
//! Layer 2. Reads `/proc/<pid>/{stat,status,comm,cmdline,ns/pid}` through [`Fs`](crate::fsx::Fs)
//! (so tests can use an in-memory tree) and never caches across calls. Sends no signals (that
//! is `session::sys`) and never reads `/proc/<pid>/environ`.

mod real;
mod tree;

pub use real::{RealProc, StatFields, StatusFields, parse_ns_link, parse_stat, parse_status};
pub use tree::ProcTree;

use std::ffi::OsString;

use crate::error::{Error, ErrorCode};
use crate::fsx::FsError;

/// Process state from `/proc/<pid>/stat` field 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcState {
    /// `R`.
    Running,
    /// `S`.
    Sleeping,
    /// `D`.
    DiskSleep,
    /// `Z`.
    Zombie,
    /// `T`.
    Stopped,
    /// `t`.
    TracingStop,
    /// `X` or `x`.
    Dead,
    /// `I`.
    Idle,
    /// Anything else.
    Other(char),
}

impl ProcState {
    /// Parse the one-letter state.
    pub fn from_char(c: char) -> Self {
        match c {
            'R' => ProcState::Running,
            'S' => ProcState::Sleeping,
            'D' => ProcState::DiskSleep,
            'Z' => ProcState::Zombie,
            'T' => ProcState::Stopped,
            't' => ProcState::TracingStop,
            'X' | 'x' => ProcState::Dead,
            'I' => ProcState::Idle,
            other => ProcState::Other(other),
        }
    }

    /// Whether the process has exited (zombie or dead).
    pub fn is_gone(self) -> bool {
        matches!(self, ProcState::Zombie | ProcState::Dead)
    }
}

/// Identity of a process that is immune to PID reuse: the PID plus its start time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcIdentity {
    /// PID (host namespace).
    pub pid: u32,
    /// Start time in clock ticks since boot (`stat` field 22).
    pub start_time_ticks: u64,
}

/// One process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcEntry {
    /// PID.
    pub pid: u32,
    /// Parent PID.
    pub ppid: u32,
    /// `comm` (at most 15 bytes, untrusted).
    pub comm: String,
    /// Start time in clock ticks since boot.
    pub start_time_ticks: u64,
    /// PID namespace inode from `ns/pid`, when readable.
    pub pid_ns: Option<u64>,
    /// State.
    pub state: ProcState,
    /// File name of the executable (`Name:` of `status`, which is `comm`) or the first
    /// `cmdline` element's file name for Wine processes, when known. Untrusted.
    pub exe_name: Option<String>,
}

impl ProcEntry {
    /// The identity.
    pub fn identity(&self) -> ProcIdentity {
        ProcIdentity {
            pid: self.pid,
            start_time_ticks: self.start_time_ticks,
        }
    }
}

/// Why process information could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ProcError {
    /// A file under `/proc` could not be read.
    #[error("cannot read process information: {0}")]
    Fs(#[from] FsError),
    /// A file had an unexpected format.
    #[error("unexpected format in {file}")]
    Parse {
        /// Which file (e.g. `stat`).
        file: &'static str,
    },
}

impl From<ProcError> for Error {
    fn from(e: ProcError) -> Self {
        Error::new(ErrorCode::IntTaskFailed)
            .with("area", "procfs")
            .with_source(e)
    }
}

/// Source of process information. Implemented by [`RealProc`] and
/// `easyplay_testkit::FakeProc`.
pub trait ProcSource: Send + Sync {
    /// PIDs of all visible processes.
    fn pids(&self) -> Result<Vec<u32>, ProcError>;
    /// One process; `Ok(None)` when it does not exist (any more).
    fn entry(&self, pid: u32) -> Result<Option<ProcEntry>, ProcError>;
    /// Command line of a process; `Ok(None)` when it does not exist. Untrusted content.
    fn cmdline(&self, pid: u32) -> Result<Option<Vec<OsString>>, ProcError>;
    /// The boot ID (`/proc/sys/kernel/random/boot_id`).
    fn boot_id(&self) -> Result<String, ProcError>;

    /// All processes that could be read (processes that exit while listing are skipped).
    fn snapshot(&self) -> Result<Vec<ProcEntry>, ProcError> {
        let mut out = Vec::new();
        for pid in self.pids()? {
            if let Ok(Some(e)) = self.entry(pid) {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Whether the process with this identity is still alive: same PID **and** same start
    /// time, and not a zombie. A reused PID never matches.
    fn is_alive(&self, id: ProcIdentity) -> Result<bool, ProcError> {
        Ok(self
            .entry(id.pid)?
            .is_some_and(|e| e.start_time_ticks == id.start_time_ticks && !e.state.is_gone()))
    }
}
