//! [`Backend`] over the real application service: maps [`UiCommand`] onto
//! `service::Command`, `service::Event` onto [`UiEvent`], and the [`SnapshotHub`]'s values
//! onto the seam's views. Setup requests go to the setup sheet and prompts to their dialogs;
//! notices become toasts worded in `strings::service`.
//!
//! Commands the service does not implement yet (`SearchFixes`, `RescanMissing`, the desktop
//! actions) answer at once: queries with
//! `QueryPayload::Failed` (so screens leave their loading state), actions with a warning
//! toast. `assess` is core's `sandbox::explain::explain_policy` through the service's
//! explainer (the statements of the real launch plan); `preset_policy` is the real
//! `sandbox::apply_preset` over the probed host facts.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::artwork::ArtSource;
use easyplay_core::config::{PreferencePatch, Preferences};
use easyplay_core::desktop::{HandlerOverall, IntegrationState};
use easyplay_core::error::catalog::{self, DescribeCtx};
use easyplay_core::error::{Error, ErrorCode, Severity, UserError};
use easyplay_core::host::HostFacts;
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::library::LibrarySnapshot;
use easyplay_core::model::{Activity, BasePreset, RuntimeFamily, SandboxPolicy, UserDir};
use easyplay_core::service::{
    AfterCreate as SvcAfter, Command, Diagnostics, Event, FolderRequest, GameEdit as SvcEdit,
    HostSnapshot, LaunchOrigin, Notice, OpenOrigin as SvcOrigin, QueryResult, RuntimesSnapshot,
    ServiceHandle, StopMode,
};

use crate::bridge::{
    AfterCreate, ArtworkSource, Backend, CheckState, FolderScan, FolderTarget, GameDetail,
    GameEdit, HandlerState, HostCheck, HostStatus, HostView, LayerInfo, OpenOrigin,
    PolicyAssessment, ProgramFound, QueryPayload, RuntimesView, SetupContext, SetupOrigin,
    ShortcutInfo, ToastLevel, ToastMsg, UiCommand, UiEvent,
};
use crate::strings::service as text;

struct State {
    events: Receiver<Event>,
    pending: VecDeque<UiEvent>,
    host: Option<HostCache>,
    runtimes: Option<RuntimesCache>,
}

/// The host view and the snapshots it was built from. The snapshots themselves are kept,
/// not their addresses: a freed snapshot's address can be given to a newer one, and the
/// view would then be taken for current.
struct HostCache {
    facts: Arc<HostSnapshot>,
    integration: Arc<IntegrationState>,
    diagnostics: Arc<Diagnostics>,
    view: Arc<HostView>,
}

/// The runtimes view, the catalogue and the library revision it was built from.
struct RuntimesCache {
    catalog: Arc<RuntimesSnapshot>,
    revision: u64,
    view: Arc<RuntimesView>,
}

/// The production backend.
pub struct ServiceBackend {
    handle: ServiceHandle,
    state: Mutex<State>,
    home: PathBuf,
    user_dirs: Vec<(UserDir, PathBuf)>,
}

impl ServiceBackend {
    /// Wrap a running service and its event receiver.
    pub fn new(handle: ServiceHandle, events: Receiver<Event>) -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let user_dirs = [
            UserDir::Desktop,
            UserDir::Documents,
            UserDir::Downloads,
            UserDir::Music,
            UserDir::Pictures,
            UserDir::Videos,
        ]
        .into_iter()
        .map(|d| (d, home.join(crate::demo::user_dir_name(d))))
        .collect();
        Self {
            handle,
            state: Mutex::new(State {
                events,
                pending: VecDeque::new(),
                host: None,
                runtimes: None,
            }),
            home,
            user_dirs,
        }
    }

    /// Use the real home and XDG user directories (from the service's launch inputs).
    pub fn with_user_dirs(mut self, home: PathBuf, user_dirs: Vec<(UserDir, PathBuf)>) -> Self {
        self.home = home;
        if !user_dirs.is_empty() {
            self.user_dirs = user_dirs;
        }
        self
    }

    /// The service handle (commands the bridge does not cover yet).
    pub fn handle(&self) -> &ServiceHandle {
        &self.handle
    }

    /// While no window is open (the resident service): take the service's events, keep what
    /// the next window must still show and drop the rest, so the queue stays small however
    /// long the service runs unattended. See [`compact_unattended`].
    pub fn settle_unattended(&self) -> Unattended {
        let mut s = self.lock();
        let mut queue = std::mem::take(&mut s.pending);
        while let Ok(ev) = s.events.try_recv() {
            drop(s);
            let ui = self.map(ev);
            s = self.lock();
            queue.extend(ui);
        }
        let (kept, verdict) = compact_unattended(queue);
        s.pending = kept;
        verdict
    }

    /// No window can be opened: discard waiting setup sheets and cancel waiting prompts, so
    /// their flows end instead of staying parked.
    pub fn abandon_unattended(&self) {
        let queue = std::mem::take(&mut self.lock().pending);
        for ev in queue {
            match ev {
                UiEvent::SetupRequested { .. } => self.handle.send(Command::DiscardDraft),
                UiEvent::Prompt(p) => self.handle.send(Command::AnswerPrompt {
                    prompt: p.id,
                    answer: easyplay_core::model::PromptAnswer::Cancel,
                }),
                _ => {}
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, ev: UiEvent) {
        self.lock().pending.push_back(ev);
    }

    fn unsupported(&self) {
        self.push(UiEvent::Toast(ToastMsg {
            text: text::NOT_AVAILABLE.to_owned(),
            level: ToastLevel::Warning,
            undo: None,
        }));
    }

    fn query_unsupported(&self, id: QueryId) {
        let e = describe(&Error::new(ErrorCode::IntNotAvailable));
        self.push(UiEvent::QueryResult {
            id,
            payload: QueryPayload::Failed(e),
        });
    }

    fn set_pref(&self, patch: PreferencePatch) {
        self.handle.send(Command::SetPreference(patch));
    }

    /// Map one service event; `None` when the UI has nothing to do.
    fn map(&self, ev: Event) -> Option<UiEvent> {
        Some(match ev {
            Event::LibraryChanged => UiEvent::LibraryChanged,
            Event::HostFactsChanged => UiEvent::HostChanged,
            Event::RuntimesChanged => UiEvent::RuntimesChanged,
            Event::PreferencesChanged => UiEvent::PreferencesChanged,
            Event::DiagnosticsChanged => UiEvent::DiagnosticsChanged,
            Event::SessionsChanged => return None,
            Event::ShowGame(g) => UiEvent::ShowGame(g),
            Event::QuitRequested => UiEvent::QuitRequested,
            Event::SetupRequested { draft, context } => UiEvent::SetupRequested {
                draft,
                context: SetupContext {
                    origin: if context.origin.launches_known() {
                        SetupOrigin::FileManager
                    } else {
                        SetupOrigin::InsideApp
                    },
                    queue: (1, 1 + context.queued as u32),
                },
            },
            Event::Prompt(p) => UiEvent::Prompt(p),
            Event::Notice(n) => UiEvent::Toast(ToastMsg {
                text: notice_text(&n, &self.handle.hub.library.load()),
                level: match n {
                    Notice::ArtworkKeptPlaceholder { .. } | Notice::OnlyFirstPaths { .. } => {
                        ToastLevel::Warning
                    }
                    Notice::StopRequested { .. } | Notice::AlreadyInLibrary { .. } => {
                        ToastLevel::Info
                    }
                    Notice::LibraryRecovered {
                        how: easyplay_core::service::RecoveredHow::StartedEmpty,
                        ..
                    } => ToastLevel::Warning,
                    _ => ToastLevel::Success,
                },
                undo: None,
            }),
            Event::Failure(u) => UiEvent::Failure(*u),
            Event::LaunchProgress { game, state, .. } => UiEvent::LaunchProgress { game, state },
            Event::QueryResult { id, result } => UiEvent::QueryResult {
                id,
                payload: match result {
                    QueryResult::Game(g) => {
                        let mut d = detail(*g);
                        d.has_shortcut = self.handle.hub.integration.load().has_shortcut(d.game.id);
                        QueryPayload::Game(Box::new(d))
                    }
                    QueryResult::Stats(s) => QueryPayload::Stats(s),
                    QueryResult::Folder(f) => QueryPayload::Folder(folder_scan(f)),
                    QueryResult::PrefixSize { bytes, .. } => QueryPayload::PrefixSize(bytes),
                    QueryResult::Inspector(d) => QueryPayload::Inspector(d),
                    QueryResult::Sessions(v) => QueryPayload::Sessions(v),
                    QueryResult::LogChunk(c) => QueryPayload::LogChunk(c),
                    QueryResult::Processes(p) => QueryPayload::Processes(p),
                    QueryResult::Report(f) => QueryPayload::Report(f),
                    QueryResult::Failed(e) => QueryPayload::Failed(*e),
                },
            },
        })
    }

    fn update(&self, id: GameId, edits: Vec<GameEdit>) {
        let mut out = Vec::new();
        for e in edits {
            out.push(match e {
                GameEdit::Name(n) => SvcEdit::Name(n),
                GameEdit::Notes(n) => SvcEdit::Notes(n),
                GameEdit::Policy(p) => SvcEdit::Policy(p),
                GameEdit::Launch(l) => SvcEdit::Launch(l),
                GameEdit::Runtime { reference, follow } => SvcEdit::Runtime { reference, follow },
                GameEdit::UmuId(u) => SvcEdit::UmuId(u),
                GameEdit::SteamAppId(a) => SvcEdit::SteamAppId(a),
                GameEdit::ResetName => SvcEdit::ResetName,
                GameEdit::Kind(k) => SvcEdit::Kind(k),
                GameEdit::ExeRole(x, r) => SvcEdit::ExeRole(x, r),
                GameEdit::ExeLabel(x, l) => SvcEdit::ExeLabel(x, l),
                GameEdit::ExeArgs(x, a) => SvcEdit::ExeArgs(x, a),
                GameEdit::RemoveExe(x) => SvcEdit::RemoveExe(x),
            });
        }
        if !out.is_empty() {
            self.handle.send(Command::UpdateGame { id, edits: out });
        }
    }
}

fn describe(err: &Error) -> UserError {
    let redact = |s: &str| s.to_owned();
    catalog::describe(
        err,
        &DescribeCtx {
            game: None,
            session: None,
            redact: &redact,
        },
    )
}

/// The record the game page shows; facts the service does not report yet stay empty.
fn detail(game: easyplay_core::model::Game) -> GameDetail {
    GameDetail {
        runtime_resolved: game
            .runtime_last_resolved
            .as_ref()
            .map(|n| n.as_str().to_owned()),
        game,
        save_location: None,
        prefix_created_at: None,
        runtime_missing: false,
        runtime_older: false,
        runtime_without_fixes: false,
        fix_title: None,
        has_shortcut: false,
    }
}

fn notice_text(n: &Notice, lib: &LibrarySnapshot) -> String {
    let name = |g: &GameId| lib.get(*g).map(|s| s.name.to_string()).unwrap_or_default();
    match n {
        Notice::GameAdded { name, .. } => crate::strings::feedback::added(name),
        Notice::AlreadyInLibrary { game } => text::already(&name(game)),
        Notice::FoundMoved { game } => text::found_moved(&name(game)),
        Notice::OnlyFirstPaths { processed } => text::only_first(*processed),
        Notice::GameRemoved { name, .. } => crate::strings::feedback::removed(name),
        Notice::ArtworkKeptPlaceholder { .. } => text::KEPT_PLACEHOLDER.to_owned(),
        Notice::ArtworkFetched { game, .. } => text::artwork(&name(game)),
        Notice::RuntimeAdopted { variant, .. } => text::adopted(variant.as_str()),
        Notice::PrefixReset { game } => crate::strings::feedback::prefix_reset(&name(game)),
        Notice::StopRequested { .. } => crate::strings::card::STOPPING.to_owned(),
        Notice::StatisticsCleared { game, sessions } => {
            let n = game.as_ref().map(name);
            text::stats_cleared(n.as_deref(), *sessions)
        }
        Notice::SessionRemoved { .. } => text::SESSION_REMOVED.to_owned(),
        Notice::ProgramAdded { game } => text::program_added(&name(game)),
        Notice::ProgramLocated { game } => text::program_located(&name(game)),
        Notice::ToolStarted { game } => text::tool_started(&name(game)),
        Notice::LaunchCancelled { game } => text::launch_cancelled(&name(game)),
        Notice::ReportSaved { path } => text::report_saved(
            &path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        Notice::LibraryRecovered {
            how,
            history_rows_lost,
        } => text::library_recovered(*how, *history_rows_lost),
    }
}

/// The add-game sheet's view of a core folder scan.
fn folder_scan(f: easyplay_core::library::FolderScan) -> FolderScan {
    FolderScan {
        folder: f.folder,
        programs: f
            .programs
            .into_iter()
            .map(|p| ProgramFound {
                path: p.path,
                role: p.role,
                filtered: p.filtered,
                size: p.size,
            })
            .collect(),
        truncated: f.truncated,
        scanning: false,
        error: f.error,
    }
}

/// The `.exe` handler row from the integration snapshot.
fn handler_state(i: &IntegrationState) -> HandlerState {
    match &i.handler {
        None => HandlerState::Unknown,
        Some(HandlerOverall::EasyPlay) => HandlerState::EasyPlay,
        Some(HandlerOverall::Other(name)) => HandlerState::Other(name.clone()),
        Some(HandlerOverall::Nothing) => HandlerState::None,
    }
}

/// The service's folder request for a UI folder target.
fn folder_request(t: FolderTarget) -> FolderRequest {
    match t {
        FolderTarget::GameFolder(g) => FolderRequest::GameFolder(g),
        FolderTarget::SaveData(g) => FolderRequest::SaveData(g),
        FolderTarget::Prefix(g) => FolderRequest::Prefix(g),
        FolderTarget::Session(s) => FolderRequest::Session(s),
        FolderTarget::DataFolder => FolderRequest::DataFolder,
        FolderTarget::Backups => FolderRequest::Backups,
        FolderTarget::Path(p) => FolderRequest::Granted(p),
    }
}

/// The host-status row from the facts (no I/O).
pub fn host_status(facts: Option<&HostFacts>) -> HostStatus {
    match facts {
        None => HostStatus::Unknown,
        Some(f) if f.bwrap.is_none() || f.umu.is_none() || !f.problems.is_empty() => {
            HostStatus::NeedsAttention
        }
        Some(_) => HostStatus::Ready,
    }
}

/// The Doctor screen's rows: every check of the full report (ERROR_HANDLING §6), in order.
pub fn doctor_checks(r: &easyplay_core::host::DoctorReport) -> Vec<HostCheck> {
    use easyplay_core::host::CheckStatus;
    r.checks
        .iter()
        .map(|c| HostCheck {
            id: c.id.clone(),
            state: match c.status {
                CheckStatus::Pass | CheckStatus::Skipped => CheckState::Pass,
                CheckStatus::Warn => CheckState::Warn,
                CheckStatus::Fail => CheckState::Fail,
            },
            name: text::doctor_check_name(&c.id).to_owned(),
            result: c.summary.clone(),
            detail: c.fix.clone(),
            fix_command: c.fix_command.clone(),
        })
        .collect()
}

/// The system-check rows from the facts.
pub fn host_checks(facts: Option<&HostFacts>) -> Vec<HostCheck> {
    let Some(f) = facts else {
        return [text::CHECK_SANDBOX, text::CHECK_LAUNCHER]
            .iter()
            .map(|n| HostCheck {
                id: String::new(),
                state: CheckState::Pending,
                name: (*n).to_owned(),
                result: String::new(),
                detail: None,
                fix_command: None,
            })
            .collect();
    };
    let tool = |id: &str, name: &str, found: Option<String>| HostCheck {
        id: id.to_owned(),
        state: if found.is_some() {
            CheckState::Pass
        } else {
            CheckState::Fail
        },
        name: name.to_owned(),
        result: found.unwrap_or_else(|| text::NOT_FOUND.to_owned()),
        detail: None,
        fix_command: None,
    };
    let mut out = vec![
        tool(
            "D-01",
            text::CHECK_SANDBOX,
            f.bwrap
                .as_ref()
                .map(|b| text::version("bubblewrap", b.version.map(|v| v.to_string()))),
        ),
        tool(
            "D-06",
            text::CHECK_LAUNCHER,
            f.umu
                .as_ref()
                .map(|u| text::version("umu", u.version.map(|v| v.to_string()))),
        ),
    ];
    for p in &f.problems {
        let u = describe(&Error::new(p.code));
        out.push(HostCheck {
            id: p.code.as_str().to_owned(),
            state: if u.severity >= Severity::Error {
                CheckState::Fail
            } else {
                CheckState::Warn
            },
            name: u.title,
            result: u.message,
            detail: Some(p.detail.clone()),
            fix_command: p
                .hint
                .as_ref()
                .and_then(easyplay_core::host::doctor::fix_command),
        });
    }
    out
}

impl Backend for ServiceBackend {
    fn send(&self, cmd: UiCommand) {
        let h = &self.handle;
        match cmd {
            UiCommand::OpenPaths { paths, origin } => h.send(Command::OpenPaths {
                paths,
                origin: match origin {
                    OpenOrigin::Picker => SvcOrigin::Picker,
                    OpenOrigin::DragDrop => SvcOrigin::DragDrop,
                    OpenOrigin::FileManager => SvcOrigin::FileManager,
                },
            }),
            UiCommand::CreateGame { draft, then } => h.send(Command::CreateGame {
                draft,
                then: match then {
                    AfterCreate::Launch => SvcAfter::Launch,
                    AfterCreate::Nothing | AfterCreate::ApplyToRest => SvcAfter::Nothing,
                },
            }),
            UiCommand::DiscardDraft => h.send(Command::DiscardDraft),
            UiCommand::UpdateGame { id, edits } => self.update(id, edits),
            UiCommand::RemoveGame { id, options } => h.send(Command::RemoveGame { id, options }),
            UiCommand::SetFavorite { id, favorite } => {
                h.send(Command::SetFavorite { id, favorite });
            }
            UiCommand::SetHidden { id, hidden } => h.send(Command::SetHidden { id, hidden }),
            UiCommand::SetTags { id, tags } => h.send(Command::SetTags { id, tags }),
            UiCommand::ApplyPreset { ids, preset } => {
                let lib = h.hub.library.load();
                let mut changed = 0;
                let mut skipped = 0;
                for id in ids {
                    if lib.get(id).is_some_and(|g| g.activity != Activity::Idle) {
                        skipped += 1;
                        continue;
                    }
                    changed += 1;
                    h.send(Command::UpdateGame {
                        id,
                        edits: vec![SvcEdit::Preset(preset)],
                    });
                }
                if changed + skipped > 1 {
                    self.push(UiEvent::Toast(ToastMsg {
                        text: crate::strings::library::bulk_summary(changed, skipped),
                        level: ToastLevel::Success,
                        undo: None,
                    }));
                }
            }
            UiCommand::ImportArtwork { id, slot, source } => match source {
                ArtworkSource::File(p) => h.send(Command::ImportArtwork {
                    id,
                    slot,
                    source: ArtSource::File(p),
                }),
                ArtworkSource::ProgramIcon => self.unsupported(),
            },
            UiCommand::ClearArtwork { id, slot } => h.send(Command::ImportArtwork {
                id,
                slot,
                source: ArtSource::Generated,
            }),
            UiCommand::FetchSteamArtwork { id, app_id } => {
                if let Some(a) = app_id {
                    h.send(Command::UpdateGame {
                        id,
                        edits: vec![SvcEdit::SteamAppId(Some(a))],
                    });
                }
                h.send(Command::FetchOnlineArtwork { id });
            }
            UiCommand::Launch { id, exe } => h.send(Command::Launch {
                id,
                exe,
                origin: LaunchOrigin::Library,
            }),
            UiCommand::Stop { id, force } => {
                let sessions = h.hub.sessions.load();
                if let Some(s) = sessions.for_game(id) {
                    h.send(Command::StopSession {
                        session: s.id,
                        mode: if force {
                            StopMode::Force
                        } else {
                            StopMode::Graceful
                        },
                    });
                }
            }
            UiCommand::AnswerPrompt { prompt, answer } => {
                h.send(Command::AnswerPrompt { prompt, answer });
            }
            UiCommand::ResetPrefix { id, options } => h.send(Command::ResetPrefix { id, options }),
            UiCommand::ProvisionRuntime { target } => h.send(Command::ProvisionRuntime { target }),
            UiCommand::AdoptRuntime { variant } => h.send(Command::AdoptRuntime { variant }),
            UiCommand::RemoveRuntime { name } => h.send(Command::RemoveRuntime { name }),
            UiCommand::RefreshRuntimes => h.send(Command::RefreshRuntimes),
            UiCommand::VerifyRuntime => h.send(Command::VerifyRuntime),
            UiCommand::SetPreference { patch, .. } => self.set_pref(patch),
            UiCommand::RefreshHostFacts => h.send(Command::RefreshHostFacts),
            UiCommand::CompleteOnboarding { .. } => {
                self.set_pref(PreferencePatch::new().set("onboarding.completed", true));
            }
            UiCommand::QueryStats { request, reply_to } => {
                h.send(Command::QueryStats { request, reply_to });
            }
            UiCommand::LoadGame { id, reply_to } => h.send(Command::LoadGame { id, reply_to }),
            UiCommand::ScanFolder { folder, reply_to } => {
                h.send(Command::ScanFolder { folder, reply_to });
            }
            UiCommand::MeasurePrefix { id, reply_to } => {
                h.send(Command::MeasurePrefix { id, reply_to });
            }
            UiCommand::SearchFixes { reply_to, .. } => self.query_unsupported(reply_to),
            UiCommand::Quit => {}
            UiCommand::AddExecutable { id, path, role } => {
                h.send(Command::AddExecutable { id, path, role });
            }
            UiCommand::LocateExecutable { id, exe, new_path } => {
                h.send(Command::LocateExecutable { id, exe, new_path });
            }
            UiCommand::CancelLaunch { id } => h.send(Command::CancelLaunch { id }),
            UiCommand::RunTool { id, tool } => h.send(Command::RunTool { id, tool }),
            UiCommand::ClearStatistics => h.send(Command::ClearStatistics),
            UiCommand::ResetGameStatistics { id } => h.send(Command::ResetGameStatistics { id }),
            UiCommand::RemoveSession { id } => h.send(Command::RemoveSession { id }),
            UiCommand::RescanMissing => self.unsupported(),
            UiCommand::SetDefaultHandler { enable } => {
                h.send(Command::SetDefaultHandler { enable });
            }
            UiCommand::SetShortcut { id, enable } => h.send(Command::SetShortcut { id, enable }),
            UiCommand::OpenFolder(t) => h.send(Command::OpenFolder(folder_request(t))),
            UiCommand::ViewLog { session, game } => h.send(Command::ViewLog { session, game }),
            UiCommand::Inspect { target, reply_to } => {
                h.send(Command::Inspect { target, reply_to })
            }
            UiCommand::ListSessions { game, reply_to } => {
                h.send(Command::ListSessions { game, reply_to });
            }
            UiCommand::ReadLog {
                session,
                file,
                offset,
                reply_to,
            } => h.send(Command::ReadLog {
                session,
                file,
                offset,
                reply_to,
            }),
            UiCommand::ListProcesses { session, reply_to } => {
                h.send(Command::ListProcesses { session, reply_to });
            }
            UiCommand::RunDoctor => h.send(Command::RunDoctor),
            UiCommand::BuildReport {
                game,
                session,
                reply_to,
            } => h.send(Command::BuildReport {
                game,
                session,
                reply_to,
            }),
            UiCommand::SaveReport { files, dest } => h.send(Command::SaveReport { files, dest }),
            UiCommand::DismissCrash => h.send(Command::DismissCrash),
            UiCommand::ResolveDatabase(c) => h.send(Command::ResolveDatabase(c)),
        }
    }

    fn try_event(&self) -> Option<UiEvent> {
        loop {
            let ev = {
                let mut s = self.lock();
                if let Some(e) = s.pending.pop_front() {
                    return Some(e);
                }
                s.events.try_recv().ok()?
            };
            if let Some(ui) = self.map(ev) {
                return Some(ui);
            }
        }
    }

    fn library(&self) -> Arc<LibrarySnapshot> {
        self.handle.hub.library.load()
    }

    fn preferences(&self) -> Arc<Preferences> {
        self.handle.hub.prefs.load()
    }

    fn diagnostics(&self) -> Arc<Diagnostics> {
        self.handle.hub.diagnostics.load()
    }

    fn host(&self) -> Arc<HostView> {
        let facts = self.handle.hub.host.load();
        let integration = self.handle.hub.integration.load();
        let diag = self.handle.hub.diagnostics.load();
        // Any snapshot changing rebuilds the view.
        let mut s = self.lock();
        if let Some(c) = &s.host
            && Arc::ptr_eq(&c.facts, &facts)
            && Arc::ptr_eq(&c.integration, &integration)
            && Arc::ptr_eq(&c.diagnostics, &diag)
        {
            return Arc::clone(&c.view);
        }
        let f = facts.as_ref().as_ref();
        let v = Arc::new(HostView {
            status: host_status(f),
            checks: diag
                .doctor
                .as_ref()
                .map_or_else(|| host_checks(f), doctor_checks),
            handler: handler_state(&integration),
            distro: f.and_then(|f| f.os.pretty_name.clone()).unwrap_or_default(),
            jobs: Vec::new(),
            shortcuts: integration
                .shortcuts
                .iter()
                .map(|s| ShortcutInfo {
                    game: s.id,
                    name: s.name.clone(),
                })
                .collect(),
            reflinks: false,
            input_readable: false,
            mangohud: false,
            x11: false,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            home: self.home.clone(),
            user_dirs: self.user_dirs.clone(),
        });
        s.host = Some(HostCache {
            facts,
            integration,
            diagnostics: diag,
            view: Arc::clone(&v),
        });
        v
    }

    fn runtimes(&self) -> Arc<RuntimesView> {
        let cat = self.handle.hub.runtimes.load();
        let lib = self.handle.hub.library.load();
        let mut s = self.lock();
        if let Some(c) = &s.runtimes
            && Arc::ptr_eq(&c.catalog, &cat)
            && c.revision == lib.revision
        {
            return Arc::clone(&c.view);
        }
        let layers = cat
            .builds
            .iter()
            .map(|b| LayerInfo {
                used_by: lib
                    .games
                    .iter()
                    .filter(|g| *g.runtime_label == *b.display_name)
                    .count() as u32,
                size: b.size_bytes,
                build: b.clone(),
            })
            .collect();
        let v = Arc::new(RuntimesView {
            layers,
            default_label: cat
                .newest(RuntimeFamily::UmuProton)
                .map(|b| b.display_name.clone()),
            slr: cat
                .slr
                .iter()
                .map(|x| {
                    (
                        x.variant.as_str().to_owned(),
                        x.version.clone().unwrap_or_default(),
                        None,
                    )
                })
                .collect(),
            last_checked: cat.scanned_at,
            downloads: Vec::new(),
            adoptable: cat.adoptable().collect(),
        });
        s.runtimes = Some(RuntimesCache {
            catalog: cat,
            revision: lib.revision,
            view: Arc::clone(&v),
        });
        v
    }

    fn assess(
        &self,
        game: Option<GameId>,
        _name: &str,
        root: Option<&Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        let e = self.handle.explain_policy(game, root, None, policy);
        PolicyAssessment {
            statements: e.statements,
            issues: e.issues,
        }
    }

    fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
        let host = self.handle.hub.host.load();
        let facts = host
            .as_ref()
            .clone()
            .unwrap_or_else(|| HostFacts::empty(easyplay_core::clock::Timestamp::from_millis(0)));
        easyplay_core::sandbox::apply_preset(policy, preset, &facts)
    }

    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.handle.set_waker(waker);
    }
}

/// What the events that arrived while no window was open ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unattended {
    /// A setup sheet or a prompt waits for an answer: a flow is parked until a window shows it.
    pub needs_window: bool,
    /// The service stopped.
    pub quit: bool,
}

/// Toasts and failures kept for the next window (the newest ones).
const UNATTENDED_KEEP: usize = 4;

/// Reduce the events queued while no window is open to what a window opened later needs. A
/// new window takes fresh snapshots, so snapshot notices go; nobody asked the answered
/// queries; of a game's launch states only the latest counts, and a launch that ended
/// normally needs nothing at all. Setup requests, prompts and the game to show stay; of
/// toasts and failures the newest [`UNATTENDED_KEEP`] each.
pub fn compact_unattended(queue: VecDeque<UiEvent>) -> (VecDeque<UiEvent>, Unattended) {
    use easyplay_core::launch::LaunchState;
    use easyplay_core::model::EndReason;

    let mut verdict = Unattended::default();
    let mut last_state: Vec<GameId> = Vec::new();
    let mut toasts = 0;
    let mut failures = 0;
    let mut show_seen = false;
    let mut kept: Vec<UiEvent> = Vec::new();
    // Newest first, so "the latest" is the first one met.
    for ev in queue.into_iter().rev() {
        let keep = match &ev {
            UiEvent::LibraryChanged
            | UiEvent::PreferencesChanged
            | UiEvent::HostChanged
            | UiEvent::RuntimesChanged
            | UiEvent::DiagnosticsChanged
            | UiEvent::QueryResult { .. }
            | UiEvent::ArtworkFailed { .. } => false,
            UiEvent::QuitRequested => {
                verdict.quit = true;
                false
            }
            UiEvent::SetupRequested { .. } | UiEvent::Prompt(_) => {
                verdict.needs_window = true;
                true
            }
            UiEvent::LaunchProgress { game, state } => {
                let first = !last_state.contains(game);
                last_state.push(*game);
                let quiet_end = matches!(
                    state,
                    LaunchState::Cancelled
                        | LaunchState::Finished {
                            reason: EndReason::Exited { .. } | EndReason::StoppedByUser,
                        }
                );
                first && !quiet_end
            }
            UiEvent::ShowGame(_) => !std::mem::replace(&mut show_seen, true),
            UiEvent::Toast(_) => {
                toasts += 1;
                toasts <= UNATTENDED_KEEP
            }
            UiEvent::Failure(_) => {
                failures += 1;
                failures <= UNATTENDED_KEEP
            }
        };
        if keep {
            kept.push(ev);
        }
    }
    kept.reverse();
    (kept.into(), verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unattended_events_shrink_to_what_a_window_needs() {
        use easyplay_core::ids::SessionId;
        use easyplay_core::launch::LaunchState;
        use easyplay_core::model::EndReason;

        let (a, b) = (
            GameId::from_uuid(uuid::Uuid::from_u128(1)),
            GameId::from_uuid(uuid::Uuid::from_u128(2)),
        );
        let progress = |game, state| UiEvent::LaunchProgress { game, state };
        let toast = |t: &str| {
            UiEvent::Toast(ToastMsg {
                text: t.to_owned(),
                level: ToastLevel::Info,
                undo: None,
            })
        };
        let session = SessionId::from_uuid(uuid::Uuid::from_u128(3));
        let mut q: VecDeque<UiEvent> = VecDeque::new();
        q.push_back(UiEvent::LibraryChanged);
        q.push_back(progress(a, LaunchState::Validating));
        q.push_back(progress(b, LaunchState::Validating));
        q.push_back(progress(a, LaunchState::Running { session }));
        q.push_back(progress(
            b,
            LaunchState::Finished {
                reason: EndReason::Exited { code: 0 },
            },
        ));
        for i in 0..6 {
            q.push_back(toast(&format!("t{i}")));
        }
        q.push_back(UiEvent::ShowGame(a));
        q.push_back(UiEvent::ShowGame(b));
        q.push_back(UiEvent::HostChanged);
        let (kept, v) = compact_unattended(q);
        assert_eq!(v, Unattended::default());
        let expected: VecDeque<UiEvent> = [
            progress(a, LaunchState::Running { session }),
            toast("t2"),
            toast("t3"),
            toast("t4"),
            toast("t5"),
            UiEvent::ShowGame(b),
        ]
        .into();
        assert_eq!(kept, expected);

        // A parked flow needs a window; the service stopping ends the resident process.
        let (_, v) = compact_unattended([UiEvent::QuitRequested].into());
        assert!(v.quit && !v.needs_window);
        let mut f = Error::new(ErrorCode::LibInvalidField);
        f = f.with("detail", "x");
        let (kept, v) = compact_unattended([UiEvent::Failure(describe(&f))].into());
        assert_eq!(kept.len(), 1);
        assert!(!v.needs_window);
    }

    #[test]
    fn host_checks_name_problems_and_fixes() {
        assert!(
            host_checks(None)
                .iter()
                .all(|c| c.state == CheckState::Pending)
        );
        let mut f = HostFacts::empty(easyplay_core::clock::Timestamp::from_millis(0));
        f.problems.push(easyplay_core::host::HostProblem {
            code: ErrorCode::HostUsernsUnavailable,
            detail: "kernel.unprivileged_userns_clone = 0".into(),
            hint: Some(easyplay_core::host::HostHint::Sysctl {
                setting: "kernel.unprivileged_userns_clone=1".into(),
            }),
        });
        let c = host_checks(Some(&f));
        assert_eq!(c[0].state, CheckState::Fail);
        let p = c.last().unwrap();
        assert_eq!(
            p.fix_command.as_deref(),
            Some("sudo sysctl -w kernel.unprivileged_userns_clone=1")
        );
        assert_eq!(host_status(Some(&f)), HostStatus::NeedsAttention);
    }
}
