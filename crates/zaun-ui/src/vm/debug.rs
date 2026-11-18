//! View-models of the Developer Mode views and the diagnostics report (DEV-002…DEV-004,
//! ERR-003): the launch inspector, the log viewer, the process tree, the report preview.
//! No `egui` here; the screens in `screens::debug` draw them.

use std::time::Duration;

use zaun_core::clock::MonoInstant;
use zaun_core::diagnostics::log_line::{self, LineKind, LineLevel, LineSource};
use zaun_core::error::UserError;
use zaun_core::ids::{GameId, QueryId, SessionId};
use zaun_core::session::ProcessRole;

use super::AsyncValue;
use crate::bridge::{
    InspectTarget, InspectorData, LogChunk, LogFile, ProcessInfo, ReportFile, Session,
};

/// Inspector tabs (DEVELOPER_MODE §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InspectorTab {
    /// Summary.
    #[default]
    Summary,
    /// Access.
    Access,
    /// Command.
    Command,
    /// Sandbox.
    Sandbox,
    /// Environment.
    Environment,
    /// Processes.
    Processes,
    /// Events.
    Events,
    /// Raw.
    Raw,
}

impl InspectorTab {
    /// In display order.
    pub const ALL: [InspectorTab; 8] = [
        InspectorTab::Summary,
        InspectorTab::Access,
        InspectorTab::Command,
        InspectorTab::Sandbox,
        InspectorTab::Environment,
        InspectorTab::Processes,
        InspectorTab::Events,
        InspectorTab::Raw,
    ];
}

/// The process tree: refreshed once a second while it is visible, never otherwise.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessTreeVm {
    /// The session shown.
    pub session: Option<SessionId>,
    /// Outstanding query.
    pub query: Option<QueryId>,
    /// Last answer.
    pub rows: Vec<ProcessInfo>,
    /// When the last request was sent.
    pub last: Option<MonoInstant>,
}

/// Refresh period of the process tree.
pub const PROCESS_REFRESH: Duration = Duration::from_secs(1);

/// One row of the tree, depth first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// Nesting depth.
    pub depth: usize,
    /// The process.
    pub info: ProcessInfo,
}

impl ProcessTreeVm {
    /// Whether a refresh is due at `now` (called only while the view is drawn).
    pub fn due(&self, now: MonoInstant) -> bool {
        self.session.is_some()
            && self.query.is_none()
            && self
                .last
                .is_none_or(|t| now.saturating_duration_since(t) >= PROCESS_REFRESH)
    }

    /// Record a request.
    pub fn requested(&mut self, q: QueryId, now: MonoInstant) {
        self.query = Some(q);
        self.last = Some(now);
    }

    /// The rows in tree order (children under their parent).
    pub fn tree(&self) -> Vec<TreeRow> {
        let pids: Vec<u32> = self.rows.iter().map(|p| p.pid).collect();
        let mut out = Vec::new();
        let mut roots: Vec<&ProcessInfo> = self
            .rows
            .iter()
            .filter(|p| !pids.contains(&p.ppid))
            .collect();
        roots.sort_by_key(|p| p.pid);
        fn walk(rows: &[ProcessInfo], p: &ProcessInfo, depth: usize, out: &mut Vec<TreeRow>) {
            if out.len() > rows.len() {
                return;
            }
            out.push(TreeRow {
                depth,
                info: p.clone(),
            });
            let mut kids: Vec<&ProcessInfo> = rows.iter().filter(|c| c.ppid == p.pid).collect();
            kids.sort_by_key(|c| c.pid);
            for k in kids {
                walk(rows, k, depth + 1, out);
            }
        }
        for r in roots {
            walk(&self.rows, r, 0, &mut out);
        }
        out
    }

    /// Process count, CPU milliseconds and resident bytes.
    pub fn totals(&self) -> (usize, u64, u64) {
        let cpu = self
            .rows
            .iter()
            .filter_map(|p| p.usage.map(|u| u.cpu_ms))
            .sum();
        let rss = self
            .rows
            .iter()
            .filter_map(|p| p.usage.map(|u| u.rss_bytes))
            .sum();
        (self.rows.len(), cpu, rss)
    }
}

/// The role's display name.
pub fn role_name(r: ProcessRole) -> &'static str {
    match r {
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

/// The launch inspector sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorVm {
    /// The game it was opened for.
    pub game: Option<GameId>,
    /// What it shows.
    pub target: InspectTarget,
    /// Outstanding query.
    pub query: Option<QueryId>,
    /// The data.
    pub data: AsyncValue<Box<InspectorData>, UserError>,
    /// Current tab.
    pub tab: InspectorTab,
    /// Mounts filter text.
    pub filter: String,
    /// Mounts: only writable.
    pub only_writable: bool,
    /// Mounts: only from the user's choices.
    pub only_choices: bool,
    /// Access statements expanded.
    pub expanded: Vec<usize>,
    /// Processes.
    pub procs: ProcessTreeVm,
}

impl InspectorVm {
    /// A sheet for `target`, waiting for `query`.
    pub fn new(
        target: InspectTarget,
        game: Option<GameId>,
        query: QueryId,
        now: MonoInstant,
    ) -> Self {
        let session = match target {
            InspectTarget::Session(s) => Some(s),
            InspectTarget::NextLaunch { .. } => None,
        };
        Self {
            game,
            target,
            query: Some(query),
            data: AsyncValue::Loading { since: now },
            tab: InspectorTab::Summary,
            filter: String::new(),
            only_writable: false,
            only_choices: false,
            expanded: Vec::new(),
            procs: ProcessTreeVm {
                session,
                ..ProcessTreeVm::default()
            },
        }
    }

    /// The mounts table filtered by the box and the toggles.
    pub fn mounts(&self, data: &InspectorData) -> Vec<zaun_core::launch::inspect::MountRow> {
        let needle = self.filter.to_lowercase();
        zaun_core::launch::inspect::mount_rows(data)
            .into_iter()
            .filter(|m| !self.only_writable || m.writable())
            .filter(|m| !self.only_choices || m.from_choice)
            .filter(|m| {
                needle.is_empty()
                    || [
                        m.kind.as_str(),
                        m.source.as_str(),
                        m.destination.as_str(),
                        m.reason.as_deref().unwrap_or(""),
                        m.section.as_deref().unwrap_or(""),
                    ]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&needle))
            })
            .collect()
    }
}

/// Lines kept by the viewer (SCREEN_SPECS_2 §15 safety).
pub const MAX_LINES: usize = 200_000;
/// Longest line shown.
pub const MAX_LINE_BYTES: usize = 4096;
/// Poll period of a followed log.
pub const FOLLOW_POLL: Duration = Duration::from_secs(1);

/// Make one line of untrusted log text safe to draw: control characters as `·` (tabs kept),
/// cut at [`MAX_LINE_BYTES`].
pub fn sanitize(bytes: &[u8]) -> String {
    let cut = &bytes[..bytes.len().min(MAX_LINE_BYTES)];
    let text = String::from_utf8_lossy(cut);
    let mut s: String = text
        .chars()
        .map(|c| {
            let bidi = matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
            if c == '\t' || !(c.is_control() || bidi) {
                c
            } else {
                '·'
            }
        })
        .collect();
    if bytes.len() > MAX_LINE_BYTES {
        s.push_str("…[truncated]");
    }
    s
}

/// The log viewer (Logs tab, and the simplified viewer of *View log*).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogViewerVm {
    /// The game.
    pub game: Option<GameId>,
    /// Sessions for the selector.
    pub sessions: Vec<Session>,
    /// Outstanding session list query.
    pub sessions_q: Option<QueryId>,
    /// The sessions were listed (an empty list means none recorded).
    pub listed: bool,
    /// The session shown.
    pub session: Option<SessionId>,
    /// The file shown.
    pub file: Option<LogFile>,
    /// Lines loaded (sanitised).
    pub lines: Vec<String>,
    /// Who wrote each line and how serious it is (`kinds[i]` is `lines[i]`'s).
    pub kinds: Vec<LineKind>,
    /// The least serious level shown (and everything more serious).
    pub level: LineLevel,
    /// Only this program's lines; `None` = all.
    pub source: Option<LineSource>,
    /// Bytes after the last newline.
    pub partial: Vec<u8>,
    /// Next offset to read.
    pub offset: u64,
    /// File size at the last read.
    pub size: u64,
    /// Outstanding chunk query.
    pub query: Option<QueryId>,
    /// The file is gone.
    pub missing: bool,
    /// Lines dropped at the front (cap).
    pub dropped: usize,
    /// Filter text.
    pub filter: String,
    /// Follow the tail.
    pub follow: bool,
    /// Wrap long lines.
    pub wrap: bool,
    /// When the followed file was last polled.
    pub polled: Option<MonoInstant>,
    /// An error answer.
    pub error: Option<UserError>,
}

impl LogViewerVm {
    /// Show `session`'s `file` from the start.
    pub fn open(&mut self, session: SessionId, file: LogFile) {
        let keep = (
            self.game,
            self.sessions.clone(),
            self.listed,
            self.filter.clone(),
            self.wrap,
            (self.level, self.source),
        );
        *self = Self {
            game: keep.0,
            sessions: keep.1,
            listed: keep.2,
            filter: keep.3,
            wrap: keep.4,
            level: keep.5.0,
            source: keep.5.1,
            session: Some(session),
            file: Some(file),
            follow: true,
            ..Self::default()
        };
    }

    /// Whether the shown session is still running.
    pub fn running(&self) -> bool {
        self.session
            .is_some_and(|s| self.sessions.iter().any(|x| x.id == s && x.is_open()))
    }

    /// Take a chunk. Returns whether more of the file is waiting to be read now.
    pub fn on_chunk(&mut self, c: LogChunk) -> bool {
        self.query = None;
        if Some(c.session) != self.session || Some(c.file) != self.file || c.offset != self.offset {
            return false;
        }
        self.missing = c.missing;
        self.size = c.size;
        self.offset = c.offset + c.bytes.len() as u64;
        let mut buf = std::mem::take(&mut self.partial);
        buf.extend_from_slice(&c.bytes);
        let mut start = 0;
        for (i, b) in buf.iter().enumerate() {
            if *b == b'\n' {
                self.push(sanitize(&buf[start..i]));
                start = i + 1;
            }
        }
        self.partial = buf[start..].to_vec();
        let at_end = self.offset >= self.size;
        if at_end && !self.partial.is_empty() && !self.running() {
            let rest = std::mem::take(&mut self.partial);
            self.push(sanitize(&rest));
        }
        if self.lines.len() > MAX_LINES {
            let n = self.lines.len() - MAX_LINES;
            self.lines.drain(..n);
            self.kinds.drain(..n);
            self.dropped += n;
        }
        !at_end && !c.bytes.is_empty()
    }

    fn push(&mut self, line: String) {
        self.kinds.push(log_line::classify(&line));
        self.lines.push(line);
    }

    /// Indices of the lines shown: at the chosen level or more serious, of the chosen
    /// program, containing the filter text.
    pub fn visible(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        (0..self.lines.len())
            .filter(|&i| {
                let k = self.kinds.get(i).copied().unwrap_or_default();
                k.level <= self.level
                    && self.source.is_none_or(|s| s == k.source)
                    && (needle.is_empty() || self.lines[i].to_lowercase().contains(&needle))
            })
            .collect()
    }

    /// Errors and warnings loaded, whatever is shown.
    pub fn counts(&self) -> (usize, usize) {
        self.kinds.iter().fold((0, 0), |(e, w), k| match k.level {
            LineLevel::Error => (e + 1, w),
            LineLevel::Warning => (e, w + 1),
            _ => (e, w),
        })
    }

    /// Whether lines are hidden by the level or the program (not by the filter text).
    pub fn narrowed(&self) -> bool {
        self.level != LineLevel::Debug || self.source.is_some()
    }

    /// Whether a poll of the followed file is due at `now`.
    pub fn poll_due(&self, now: MonoInstant) -> bool {
        self.follow
            && self.running()
            && self.query.is_none()
            && self
                .polled
                .is_none_or(|t| now.saturating_duration_since(t) >= FOLLOW_POLL)
    }
}

/// The report preview.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportVm {
    /// The game included.
    pub game: Option<GameId>,
    /// The session included.
    pub session: Option<SessionId>,
    /// Outstanding query.
    pub query: Option<QueryId>,
    /// The files.
    pub files: Option<Vec<ReportFile>>,
    /// The file being viewed.
    pub viewing: Option<usize>,
    /// An error answer.
    pub error: Option<UserError>,
}

/// Everything the debug views keep.
#[derive(Clone, Debug, Default)]
pub struct DebugVm {
    /// The inspector sheet.
    pub inspector: Option<InspectorVm>,
    /// The log viewer.
    pub logs: Option<LogViewerVm>,
    /// The report preview.
    pub report: Option<ReportVm>,
    /// The crash banner was dismissed (sent once).
    pub crash_dismissed: bool,
    /// The selected recovery choice.
    pub recovery_choice: usize,
    /// A recovery choice was sent.
    pub recovery_sent: bool,
    /// The report was requested from a banner (opened by the next frame).
    pub open_report: bool,
    /// The "uses advanced options" list is open.
    pub review_open: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaun_core::procfs::{ProcState, ProcUsage};

    fn p(pid: u32, ppid: u32, comm: &str, cpu: u64, rss: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            comm: comm.into(),
            state: ProcState::Sleeping,
            role: ProcessRole::Other,
            cmdline: None,
            usage: Some(ProcUsage {
                cpu_ms: cpu,
                rss_bytes: rss,
            }),
        }
    }

    fn sid() -> SessionId {
        SessionId::from_uuid(uuid::Uuid::from_u128(5))
    }

    #[test]
    fn process_tree_order_totals_and_refresh() {
        let mut vm = ProcessTreeVm {
            session: Some(sid()),
            ..ProcessTreeVm::default()
        };
        let t0 = MonoInstant::ZERO;
        assert!(vm.due(t0));
        vm.requested(QueryId::from_uuid(uuid::Uuid::from_u128(1)), t0);
        assert!(
            !vm.due(t0.saturating_add(Duration::from_secs(5))),
            "one at a time"
        );
        vm.query = None;
        assert!(!vm.due(t0.saturating_add(Duration::from_millis(500))));
        assert!(vm.due(t0.saturating_add(Duration::from_secs(1))));
        vm.rows = vec![
            p(30, 20, "wineserver", 100, 1 << 20),
            p(10, 1, "init", 1, 1 << 10),
            p(21, 10, "game.exe", 5000, 1 << 30),
            p(20, 10, "umu-run", 200, 1 << 22),
        ];
        let order: Vec<(usize, u32)> = vm.tree().iter().map(|r| (r.depth, r.info.pid)).collect();
        assert_eq!(order, vec![(0, 10), (1, 20), (2, 30), (1, 21)]);
        assert_eq!(
            vm.totals(),
            (4, 5301, (1 << 30) + (1 << 22) + (1 << 20) + (1 << 10))
        );
        // No session: never due (hidden views do not refresh either: `due` is only asked
        // while drawing).
        assert!(!ProcessTreeVm::default().due(t0));
    }

    #[test]
    fn log_chunks_split_sanitise_and_cap() {
        let mut vm = LogViewerVm::default();
        vm.open(sid(), LogFile::Game);
        let more = vm.on_chunk(LogChunk {
            session: sid(),
            file: LogFile::Game,
            offset: 0,
            bytes: b"one\ntw\x1bo\x00\npart".to_vec(),
            size: 30,
            missing: false,
        });
        assert!(more);
        assert_eq!(vm.lines, vec!["one".to_owned(), "tw·o·".to_owned()]);
        assert_eq!(vm.partial, b"part");
        // A stale chunk (other offset) is ignored.
        assert!(!vm.on_chunk(LogChunk {
            session: sid(),
            file: LogFile::Game,
            offset: 0,
            bytes: b"x\n".to_vec(),
            size: 30,
            missing: false,
        }));
        assert_eq!(vm.lines.len(), 2);
        vm.query = None;
        let more = vm.on_chunk(LogChunk {
            session: sid(),
            file: LogFile::Game,
            offset: 14,
            bytes: b"ial\nlast".to_vec(),
            size: 22,
            missing: false,
        });
        assert!(!more);
        assert_eq!(vm.lines.last().unwrap(), "last");
        assert_eq!(vm.lines[2], "partial");
        vm.filter = "PART".into();
        assert_eq!(vm.visible(), vec![2]);
        // Long lines are cut.
        let long = vec![b'a'; MAX_LINE_BYTES + 10];
        assert!(sanitize(&long).ends_with("…[truncated]"));
    }

    #[test]
    fn hostile_log_content_is_plain_text() {
        let s = sanitize("\u{202e}evil\r\u{7}bell [link](http://x) <b>".as_bytes());
        assert!(!s.chars().any(|c| c.is_control() || c == '\u{202e}'));
        assert!(s.contains("[link](http://x) <b>"));
    }
}
