//! Active sessions and their processes (PROC-011 partial, PROC-014; PROCESS_LIFECYCLE §11).
//!
//! [`Registry`] is the in-memory map of open rows whose supervisor is alive, rebuilt from the
//! database (the GUI's session watcher, `service::session_watch`, polls it; that thread is
//! part of the service layer and not implemented yet). [`process_list`] reads `/proc` on
//! demand only — nothing here runs in the background.

use std::collections::BTreeMap;

use crate::ids::{GameId, SessionId};
use crate::model::Session;
use crate::procfs::{ProcEntry, ProcIdentity, ProcSource, ProcState, ProcTree};
use crate::storage::{SessionRepo, StorageResult};

use super::stop::supervisor_identity;

/// An open session with a live supervisor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSession {
    /// The row.
    pub session: Session,
    /// Its supervisor.
    pub supervisor: ProcIdentity,
}

/// Active sessions by ID.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    sessions: BTreeMap<SessionId, ActiveSession>,
}

impl Registry {
    /// Rebuild from the open rows; rows whose supervisor is dead are left for recovery.
    pub fn rebuild<S: SessionRepo + ?Sized>(
        store: &mut S,
        proc: &dyn ProcSource,
    ) -> StorageResult<Self> {
        let mut sessions = BTreeMap::new();
        for s in store.open_sessions()? {
            if let Some(id) = supervisor_identity(&s)
                && proc.is_alive(id).unwrap_or(false)
            {
                sessions.insert(
                    s.id,
                    ActiveSession {
                        session: s,
                        supervisor: id,
                    },
                );
            }
        }
        Ok(Self { sessions })
    }

    /// All active sessions, oldest first by ID.
    pub fn active(&self) -> impl Iterator<Item = &ActiveSession> {
        self.sessions.values()
    }

    /// The active session of a game (one per prefix).
    pub fn for_game(&self, game: GameId) -> Option<&ActiveSession> {
        self.sessions
            .values()
            .find(|a| a.session.game_id == Some(game))
    }

    /// One session.
    pub fn get(&self, id: SessionId) -> Option<&ActiveSession> {
        self.sessions.get(&id)
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether nothing runs.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// A sandbox seen through `/proc`: its PID 1 (`child-pid`) and PID namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxView {
    /// Host PID of the sandbox's PID 1.
    pub child_pid: u32,
    /// Its PID namespace.
    pub pid_ns: Option<u64>,
    /// Whether the sandbox has a PID namespace of its own (false under a fake bwrap in
    /// tests). Namespace membership is used only then; otherwise the members are the
    /// descendants of `child_pid` — never "everything in my namespace".
    pub own_namespace: bool,
}

impl SandboxView {
    /// The view of a row (`sandbox_pid`, `pid_ns`), comparing with the caller's namespace.
    pub fn of(session: &Session, own_ns: Option<u64>) -> Option<Self> {
        let child_pid = session.sandbox_pid?;
        Some(Self {
            child_pid,
            pid_ns: session.pid_ns,
            own_namespace: session.pid_ns.is_some() && session.pid_ns != own_ns,
        })
    }

    /// Live member processes (never PID 1 of the host or the caller).
    pub fn members(&self, proc: &dyn ProcSource) -> Vec<ProcEntry> {
        let Ok(snapshot) = proc.snapshot() else {
            return Vec::new();
        };
        members_in(&ProcTree::build(snapshot), self)
    }

    /// The init shim's host PID: PID 2 of the namespace (the first child of bwrap's PID 1),
    /// or `child_pid` itself under a fake bwrap.
    pub fn init_pid(&self, proc: &dyn ProcSource) -> u32 {
        if !self.own_namespace {
            return self.child_pid;
        }
        proc.snapshot()
            .ok()
            .map(ProcTree::build)
            .and_then(|t| t.children(self.child_pid).first().copied())
            .unwrap_or(self.child_pid)
    }
}

fn members_in(tree: &ProcTree, v: &SandboxView) -> Vec<ProcEntry> {
    let mut pids = match (v.own_namespace, v.pid_ns) {
        (true, Some(ns)) => tree.sandbox_members(ns),
        _ => tree.descendants(v.child_pid),
    };
    pids.insert(v.child_pid);
    let me = std::process::id();
    pids.into_iter()
        .filter(|p| *p > 1 && *p != me)
        .filter_map(|p| tree.get(p).cloned())
        .filter(|e| !e.state.is_gone())
        .collect()
}

/// What a process of a sandbox probably is (PROCESS_LIFECYCLE §11, DEVELOPER_MODE §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessRole {
    /// `easyplay-init` (bound as `/run/easyplay/init`).
    Init,
    /// `umu-run`.
    Umu,
    /// bwrap, pressure-vessel, the inner container.
    Container,
    /// The Proton script.
    Proton,
    /// `wineserver`.
    Wineserver,
    /// A Wine service process.
    WineService,
    /// The game (the target executable or another Windows program).
    Game,
    /// Anything else.
    Other,
}

impl ProcessRole {
    /// Stable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProcessRole::Init => "init",
            ProcessRole::Umu => "umu",
            ProcessRole::Container => "container",
            ProcessRole::Proton => "proton",
            ProcessRole::Wineserver => "wineserver",
            ProcessRole::WineService => "wine-service",
            ProcessRole::Game => "game",
            ProcessRole::Other => "other",
        }
    }
}

const WINE_SERVICES: [&str; 10] = [
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "explorer.exe",
    "rpcss.exe",
    "svchost.exe",
    "conhost.exe",
    "tabtip.exe",
    "start.exe",
    // Proton 10's accessibility helper, started with every prefix (observed in M6).
    "xalia.exe",
];

/// The role guess for a `comm` (`target`: the game's `comm`, lower case).
pub fn guess_role(comm: &str, target: Option<&str>) -> ProcessRole {
    let c = comm.to_lowercase();
    if target.is_some_and(|t| t == c) {
        return ProcessRole::Game;
    }
    match c.as_str() {
        "init" | "easyplay-init" => ProcessRole::Init,
        "umu-run" | "umu" => ProcessRole::Umu,
        "bwrap" | "srt-bwrap" | "pv-adverb" | "steam-runtime-l" | "steam-runtime-s" => {
            ProcessRole::Container
        }
        "proton" => ProcessRole::Proton,
        "wineserver" => ProcessRole::Wineserver,
        _ if c.contains("umu-run") => ProcessRole::Umu,
        _ if c.starts_with("pressure-vessel") || c.starts_with("pv-") => ProcessRole::Container,
        _ if WINE_SERVICES.contains(&c.as_str()) => ProcessRole::WineService,
        _ if c.ends_with(".exe") => ProcessRole::Game,
        _ => ProcessRole::Other,
    }
}

/// One process of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    /// Host PID.
    pub pid: u32,
    /// Parent.
    pub ppid: u32,
    /// `comm` (untrusted).
    pub comm: String,
    /// State.
    pub state: ProcState,
    /// Role guess.
    pub role: ProcessRole,
    /// Command line (Developer Mode only), at most 512 bytes. Untrusted.
    pub cmdline: Option<String>,
    /// CPU time and resident memory, when readable.
    pub usage: Option<crate::procfs::ProcUsage>,
}

/// The processes of an active session, read now (call at most once per second, only while
/// someone is looking). `target`: the game's `comm` for the role guess.
pub fn process_list(
    proc: &dyn ProcSource,
    session: &Session,
    own_ns: Option<u64>,
    target: Option<&str>,
    with_cmdline: bool,
) -> Vec<ProcessInfo> {
    let Some(view) = SandboxView::of(session, own_ns) else {
        return Vec::new();
    };
    view.members(proc)
        .into_iter()
        .map(|e| {
            let cmdline = with_cmdline
                .then(|| proc.cmdline(e.pid).ok().flatten())
                .flatten()
                .map(|args| {
                    let mut s = args
                        .iter()
                        .map(|a| a.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if s.len() > 512 {
                        let mut end = 512;
                        while !s.is_char_boundary(end) {
                            end -= 1;
                        }
                        s.truncate(end);
                    }
                    s
                });
            let role = if e.pid == view.child_pid {
                ProcessRole::Container
            } else {
                guess_role(&e.comm, target)
            };
            ProcessInfo {
                usage: proc.usage(e.pid).ok().flatten(),
                pid: e.pid,
                ppid: e.ppid,
                comm: e.comm,
                state: e.state,
                role,
                cmdline,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles() {
        let t = Some("game.exe");
        for (comm, role) in [
            ("init", ProcessRole::Init),
            ("umu-run", ProcessRole::Umu),
            (".umu-run-wrappe", ProcessRole::Umu),
            ("xalia.exe", ProcessRole::WineService),
            ("pressure-vessel", ProcessRole::Container),
            ("pv-adverb", ProcessRole::Container),
            ("wineserver", ProcessRole::Wineserver),
            ("services.exe", ProcessRole::WineService),
            ("Game.exe", ProcessRole::Game),
            ("launcher.exe", ProcessRole::Game),
            ("python3", ProcessRole::Other),
        ] {
            assert_eq!(guess_role(comm, t), role, "{comm}");
        }
    }
}
