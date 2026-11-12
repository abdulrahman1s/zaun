//! `FakeProc`: scripted process tables for [`ProcSource`].

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;

use easyplay_core::procfs::{ProcEntry, ProcError, ProcSource, ProcState, ProcUsage};
use parking_lot::Mutex;

#[derive(Debug, Default)]
struct State {
    entries: BTreeMap<u32, ProcEntry>,
    cmdlines: HashMap<u32, Vec<OsString>>,
    usage: HashMap<u32, ProcUsage>,
    boot_id: String,
}

/// A process table the test edits between calls.
#[derive(Debug)]
pub struct FakeProc {
    state: Mutex<State>,
}

impl Default for FakeProc {
    fn default() -> Self {
        Self::new()
    }
}

/// A sleeping process in PID namespace `ns`, with start time `pid * 100` ticks.
pub fn proc_entry(pid: u32, ppid: u32, comm: &str, ns: u64) -> ProcEntry {
    ProcEntry {
        pid,
        ppid,
        comm: comm.to_owned(),
        start_time_ticks: u64::from(pid) * 100,
        pid_ns: Some(ns),
        state: ProcState::Sleeping,
        exe_name: Some(comm.to_owned()),
    }
}

impl FakeProc {
    /// An empty table with a fixed boot ID.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                boot_id: "00000000-0000-4000-8000-000000000b00".to_owned(),
                ..State::default()
            }),
        }
    }

    /// Add or replace a process.
    pub fn insert(&self, entry: ProcEntry) -> &Self {
        self.state.lock().entries.insert(entry.pid, entry);
        self
    }

    /// Add several processes.
    pub fn extend(&self, entries: impl IntoIterator<Item = ProcEntry>) -> &Self {
        let mut s = self.state.lock();
        for e in entries {
            s.entries.insert(e.pid, e);
        }
        drop(s);
        self
    }

    /// Remove a process (it exited and was reaped).
    pub fn remove(&self, pid: u32) -> &Self {
        let mut s = self.state.lock();
        s.entries.remove(&pid);
        s.cmdlines.remove(&pid);
        drop(s);
        self
    }

    /// Change a process's state (e.g. to `Zombie`).
    pub fn set_state(&self, pid: u32, state: ProcState) -> &Self {
        if let Some(e) = self.state.lock().entries.get_mut(&pid) {
            e.state = state;
        }
        self
    }

    /// Set a command line.
    pub fn set_cmdline(&self, pid: u32, args: &[&str]) -> &Self {
        self.state
            .lock()
            .cmdlines
            .insert(pid, args.iter().map(OsString::from).collect());
        self
    }

    /// Set CPU time and resident memory.
    pub fn set_usage(&self, pid: u32, cpu_ms: u64, rss_bytes: u64) -> &Self {
        self.state
            .lock()
            .usage
            .insert(pid, ProcUsage { cpu_ms, rss_bytes });
        self
    }

    /// Set the boot ID.
    pub fn set_boot_id(&self, id: &str) -> &Self {
        self.state.lock().boot_id = id.to_owned();
        self
    }

    /// Remove every process.
    pub fn clear(&self) {
        let mut s = self.state.lock();
        s.entries.clear();
        s.cmdlines.clear();
    }
}

impl ProcSource for FakeProc {
    fn pids(&self) -> Result<Vec<u32>, ProcError> {
        Ok(self.state.lock().entries.keys().copied().collect())
    }

    fn entry(&self, pid: u32) -> Result<Option<ProcEntry>, ProcError> {
        Ok(self.state.lock().entries.get(&pid).cloned())
    }

    fn cmdline(&self, pid: u32) -> Result<Option<Vec<OsString>>, ProcError> {
        let s = self.state.lock();
        if !s.entries.contains_key(&pid) {
            return Ok(None);
        }
        Ok(Some(s.cmdlines.get(&pid).cloned().unwrap_or_default()))
    }

    fn boot_id(&self) -> Result<String, ProcError> {
        Ok(self.state.lock().boot_id.clone())
    }

    fn usage(&self, pid: u32) -> Result<Option<ProcUsage>, ProcError> {
        let s = self.state.lock();
        if !s.entries.contains_key(&pid) {
            return Ok(None);
        }
        Ok(Some(s.usage.get(&pid).copied().unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::procfs::{ProcIdentity, ProcTree};

    #[test]
    fn identity_rejects_reused_pid() {
        let p = FakeProc::new();
        p.insert(proc_entry(42, 1, "game.exe", 9));
        let id = p.entry(42).unwrap().unwrap().identity();
        assert!(p.is_alive(id).unwrap());
        // The process exits and the PID is reused by an unrelated process.
        p.remove(42);
        assert!(!p.is_alive(id).unwrap());
        let mut reused = proc_entry(42, 1, "bash", 1);
        reused.start_time_ticks = 999_999;
        p.insert(reused);
        assert!(!p.is_alive(id).unwrap());
        assert!(
            p.is_alive(ProcIdentity {
                pid: 42,
                start_time_ticks: 999_999
            })
            .unwrap()
        );
        p.set_state(42, ProcState::Zombie);
        assert!(
            !p.is_alive(ProcIdentity {
                pid: 42,
                start_time_ticks: 999_999
            })
            .unwrap()
        );
    }

    #[test]
    fn snapshot_builds_a_tree() {
        let p = FakeProc::new();
        p.extend([
            proc_entry(10, 1, "bwrap", 1),
            proc_entry(11, 10, "easyplay-init", 5),
            proc_entry(12, 11, "umu-run", 5),
        ])
        .set_cmdline(12, &["python3", "/usr/bin/umu-run"]);
        let tree = ProcTree::build(p.snapshot().unwrap());
        assert_eq!(tree.sandbox_members(5).len(), 2);
        assert_eq!(p.cmdline(12).unwrap().unwrap().len(), 2);
        assert_eq!(p.cmdline(99).unwrap(), None);
        assert!(!p.boot_id().unwrap().is_empty());
    }
}
