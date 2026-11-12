//! Developer Mode and diagnostics flows (DEV-001, DEV-003, DEV-004, ERR-002, ERR-003,
//! ERR-005): launch inspector data, session lists and log chunks for the viewer, process
//! lists, the full Doctor report, the diagnostics report, the crash banner.
//!
//! Everything that touches the disk or `/proc` runs on the job pool and answers with a
//! `QueryResult` (or, for the Doctor, the diagnostics snapshot). The dry run goes through
//! `launch::dry_run`, which executes no host action and writes nothing.

use std::path::PathBuf;
use std::sync::Arc;

use super::super::app_service::AppService;
use super::super::command::{InspectTarget, LogFile};
use super::super::event::{Event, LogChunk, Notice, QueryResult};
use super::super::snapshot::Diagnostics;
use crate::diagnostics::crash;
use crate::diagnostics::report::{self, GamePart, ReportFile, ReportInput};
use crate::error::{Error, ErrorCode};
use crate::host::DoctorContext;
use crate::ids::{GameId, QueryId, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::launch::{self, InspectorData, InspectorSource, LaunchRequest, OrchestratorDeps};
use crate::model::SessionKind;
use crate::storage::SessionRepo;

/// Largest chunk returned by `ReadLog` (the viewer asks again from the new offset).
pub const LOG_CHUNK: usize = 1024 * 1024;
/// The first chunk is small so the viewer shows text at once (SCREEN_SPECS_2 §15).
pub const FIRST_CHUNK: usize = 64 * 1024;
/// Sessions listed for the selector.
pub const SESSION_LIST: u64 = 50;

impl AppService {
    /// Update the diagnostics snapshot and announce it.
    pub(crate) fn update_diagnostics(&self, f: impl FnOnce(&mut Diagnostics)) {
        let mut d = (*self.hub.diagnostics.load()).clone();
        f(&mut d);
        self.hub.diagnostics.store(Arc::new(d));
        self.emit(Event::DiagnosticsChanged);
    }

    /// `Inspect`.
    pub(crate) fn inspect(&mut self, target: InspectTarget, reply_to: QueryId) {
        match target {
            InspectTarget::Session(sid) => self.inspect_session(sid, reply_to),
            InspectTarget::NextLaunch { game, exe } => self.inspect_next(game, exe, reply_to),
        }
    }

    fn inspect_session(&mut self, sid: SessionId, reply_to: QueryId) {
        let row = match self.sessions.get(&sid).cloned() {
            Some(s) => Some(s),
            None => self.lib.db_mut().session(sid).ok().flatten(),
        };
        let game = row.as_ref().and_then(|s| s.game_id);
        let describe = self.describer(game);
        let Some(row) = row else {
            let e = Error::new(ErrorCode::FsReadDenied).with("detail", "no such session");
            return self.emit(Event::QueryResult {
                id: reply_to,
                result: QueryResult::Failed(Box::new(describe(&e))),
            });
        };
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        self.answer_on_pool("inspect-session", reply_to, move |_| {
            match launch::inspect::load_session(&*fs, &paths, &row) {
                Ok(d) => QueryResult::Inspector(Box::new(d)),
                Err(e) => QueryResult::Failed(Box::new(describe(
                    &Error::new(ErrorCode::FsReadDenied)
                        .with("detail", e.to_string())
                        .with("session", sid),
                ))),
            }
        });
    }

    fn inspect_next(
        &mut self,
        id: GameId,
        exe: Option<crate::ids::ExecutableId>,
        reply_to: QueryId,
    ) {
        let describe = self.describer(Some(id));
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => {
                return self.emit(Event::QueryResult {
                    id: reply_to,
                    result: QueryResult::Failed(Box::new(describe(&e))),
                });
            }
        };
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let paths = self.deps.paths.clone();
        let mut env = self.deps.launch_env.clone();
        let developer = crate::sandbox::validate::effective_developer_mode(
            self.prefs.advanced.developer_mode,
            &game.policy,
            &game.launch,
        );
        let install_id = self.install_id.clone();
        let other_roots: Vec<_> = self.lib.exe_index().other_roots(Some(id));
        let runtimes = Arc::clone(&self.deps.runtimes);
        let launcher = Arc::clone(&self.deps.launcher);
        let locks = Arc::clone(&self.deps.locks);
        let host = Arc::clone(&self.deps.host);
        env.global_env = self.prefs.launch.global_env.clone();
        self.answer_on_pool("inspect-dry-run", reply_to, move |_| {
            let facts = host.facts();
            let mut henv = env.host_env(&paths, &install_id, &other_roots, &*fs);
            henv.developer_mode = developer;
            let deps = OrchestratorDeps {
                host: &facts,
                env: &henv,
                runtimes: &*runtimes,
                launcher: &*launcher,
                locks: &*locks,
                fs: Arc::clone(&fs),
                clock: &*clock,
                steps: None,
                cancel: None,
            };
            let req = LaunchRequest {
                game: &game,
                exe,
                extra_args: &[],
                session_id: SessionId::generate(&*clock),
                network_consent: false,
            };
            match launch::dry_run(&req, &deps) {
                Ok(d) => {
                    let mut notes: Vec<String> = d.note.into_iter().collect();
                    notes.extend(d.planned.sandbox.notes.iter().map(|n| n.text.clone()));
                    if d.audit_first {
                        notes.push(
                            "The shared runtime is audited before the game starts.".to_owned(),
                        );
                    }
                    let explain = crate::sandbox::explain(&d.planned.sandbox, &game.policy);
                    QueryResult::Inspector(Box::new(InspectorData {
                        source: InspectorSource::DryRun,
                        plan: d.planned.plan,
                        sandbox: Some(d.planned.sandbox),
                        explain,
                        timeline: Vec::new(),
                        session: None,
                        notes,
                    }))
                }
                Err(e) => {
                    let err = match e {
                        launch::LaunchError::Failed(e) => e,
                        other => Error::new(ErrorCode::IntTaskFailed)
                            .with("detail", format!("{other:?}")),
                    };
                    QueryResult::Failed(Box::new(describe(&err)))
                }
            }
        });
    }

    /// `ListSessions`: the game's recent sessions of every kind, newest first.
    pub(crate) fn list_sessions(&mut self, game: GameId, reply_to: QueryId) {
        let result = match self
            .lib
            .db_mut()
            .session_history(Some(game), &[], 0, SESSION_LIST)
        {
            Ok(mut v) => {
                // Open sessions first: they are the newest.
                for s in self.sessions.values() {
                    if s.game_id == Some(game) && !v.iter().any(|x| x.id == s.id) {
                        v.insert(0, s.clone());
                    }
                }
                QueryResult::Sessions(v)
            }
            Err(e) => QueryResult::Failed(Box::new(self.describe(&e.into(), Some(game)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
    }

    /// `ReadLog`: one chunk of a session log file.
    pub(crate) fn read_log(
        &mut self,
        session: SessionId,
        file: LogFile,
        offset: u64,
        reply_to: QueryId,
    ) {
        let fs = Arc::clone(&self.deps.fs);
        let path = self.deps.paths.session_dir(session).join(file.file_name());
        self.answer_on_pool("read-log", reply_to, move |_| {
            let Ok(md) = fs.metadata(&path) else {
                return QueryResult::LogChunk(LogChunk {
                    session,
                    file,
                    offset,
                    bytes: Vec::new(),
                    size: 0,
                    missing: true,
                });
            };
            let len = if offset == 0 { FIRST_CHUNK } else { LOG_CHUNK };
            let bytes = fs.read_range(&path, offset, len).unwrap_or_default();
            QueryResult::LogChunk(LogChunk {
                session,
                file,
                offset,
                bytes,
                size: md.size,
                missing: !md.is_file(),
            })
        });
    }

    /// `ListProcesses`: the open session's processes now, with CPU time and memory.
    pub(crate) fn list_processes(&mut self, session: SessionId, reply_to: QueryId) {
        let Some(s) = self.sessions.get(&session).cloned() else {
            return self.emit(Event::QueryResult {
                id: reply_to,
                result: QueryResult::Processes(Vec::new()),
            });
        };
        let target = s
            .game_id
            .and_then(|g| self.lib.game(g).ok())
            .and_then(|g| {
                s.executable_id
                    .and_then(|id| g.executables.iter().find(|e| e.id == id).cloned())
                    .or_else(|| g.primary().cloned())
            })
            .and_then(|e| e.path.file_name().map(|n| n.to_string_lossy().into_owned()));
        let proc = Arc::clone(&self.deps.proc);
        let own_ns = self
            .deps
            .proc
            .entry(std::process::id())
            .ok()
            .flatten()
            .and_then(|e| e.pid_ns);
        self.answer_on_pool("list-processes", reply_to, move |_| {
            QueryResult::Processes(crate::session::process_list(
                &*proc,
                &s,
                own_ns,
                target.as_deref(),
                true,
            ))
        });
    }

    /// `RunDoctor`: every check, including the database and the init self-test.
    pub(crate) fn run_doctor(&mut self) {
        let Some(facts) = self.host.clone() else {
            // The probe is still running; it triggers the Doctor when it lands.
            return;
        };
        let fs = Arc::clone(&self.deps.fs);
        let runner = self.deps.host.runner();
        let paths = self.deps.paths.clone();
        let uid = self.deps.launch_env.uid;
        let init = self.deps.launch_env.init_binary.clone();
        let catalog = self.hub.runtimes.load();
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Background, JobLabel("doctor"), move |_| {
                let ctx = DoctorContext {
                    fs: &*fs,
                    paths: &paths,
                    uid,
                    init_candidates: vec![init],
                };
                let mut r = crate::diagnostics::doctor_report(&facts, &ctx, &catalog);
                crate::diagnostics::complete(
                    &mut r,
                    &ctx,
                    &crate::diagnostics::DoctorProbes {
                        runner: runner.as_deref(),
                        db_file: &paths.db_file,
                    },
                );
                post(super::super::app_service::Internal::Doctor(Box::new(r)));
            });
    }

    /// The Doctor job finished.
    pub(crate) fn on_doctor(&mut self, report: crate::host::DoctorReport) {
        self.update_diagnostics(|d| d.doctor = Some(report));
    }

    /// `BuildReport`: collect the files for the preview.
    pub(crate) fn build_report(
        &mut self,
        game: Option<GameId>,
        session: Option<SessionId>,
        reply_to: QueryId,
    ) {
        let g = game.and_then(|id| self.lib.game(id).ok());
        let session = session.or_else(|| {
            let id = g.as_ref()?.id;
            self.lib
                .db_mut()
                .session_history(Some(id), &[SessionKind::Game], 0, 1)
                .ok()
                .and_then(|v| v.first().map(|s| s.id))
        });
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let redactor = self.redactor();
        let doctor = self.hub.diagnostics.load().doctor.clone();
        let facts = self.host.clone();
        self.answer_on_pool("build-report", reply_to, move |_| {
            let version = format!("EasyPlay {}\nreport format 1\n", crate::consts::APP_VERSION);
            let files = report::build(&ReportInput {
                fs: &*fs,
                paths: &paths,
                redactor: &redactor,
                version,
                doctor: doctor.as_ref(),
                facts: facts.as_deref(),
                game: g.as_ref().map(|g| GamePart {
                    policy: &g.policy,
                    launch: &g.launch,
                    session,
                }),
            });
            QueryResult::Report(files)
        });
    }

    /// `SaveReport`: write the archive where the user chose.
    pub(crate) fn save_report(&mut self, files: Vec<ReportFile>, dest: PathBuf) {
        let fs = Arc::clone(&self.deps.fs);
        let mtime = u64::try_from(self.deps.clock.now().as_millis() / 1000).unwrap_or(0);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("save-report"),
            move |_| {
                let r = report::archive_bytes(&files, mtime)
                    .map_err(|e| Error::new(ErrorCode::FsWriteDenied).with_source(e))
                    .and_then(|bytes| fs.atomic_write(&dest, &bytes, 0o600).map_err(Error::from))
                    .map(|()| dest);
                post(super::super::app_service::Internal::ReportSaved(r));
            },
        );
    }

    /// The archive was written (or not).
    pub(crate) fn on_report_saved(&mut self, r: Result<PathBuf, Error>) {
        match r {
            Ok(path) => self.notice(Notice::ReportSaved { path }),
            Err(e) => self.fail(e),
        }
    }

    /// Load the crash banner (at start).
    pub(crate) fn load_crash(&mut self) {
        let dir = self.deps.paths.crash_dir.clone();
        let rec = crash::pending(&*self.deps.fs, &dir).map(|(_, r)| r);
        if rec.is_some() {
            self.update_diagnostics(|d| d.crash = rec);
        }
    }

    /// `DismissCrash`.
    pub(crate) fn dismiss_crash(&mut self) {
        if let Err(e) = crash::acknowledge(&*self.deps.fs, &self.deps.paths.crash_dir) {
            tracing::warn!("crash record not acknowledged: {e}");
        }
        self.update_diagnostics(|d| d.crash = None);
    }
}
