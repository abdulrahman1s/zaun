//! [`Backend`] over the real application service: maps [`UiCommand`] onto
//! `service::Command`, `service::Event` onto [`UiEvent`], and the [`SnapshotHub`]'s values
//! onto the seam's views. Setup requests go to the setup sheet and prompts to their dialogs;
//! notices become toasts worded in `strings::service`.
//!
//! Commands the service does not implement yet answer at once: queries with
//! `QueryPayload::Failed` (so screens leave their loading state), actions with a warning
//! toast. `assess` uses the policy-level explainer of [`crate::demo::assess_in`] until core
//! offers `sandbox::explain` without a full launch plan; `preset_policy` is the real
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
use easyplay_core::host::{HostFacts, HostHint};
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::library::LibrarySnapshot;
use easyplay_core::model::{Activity, BasePreset, RuntimeFamily, SandboxPolicy, UserDir};
use easyplay_core::service::{
    AfterCreate as SvcAfter, Command, Event, FolderRequest, GameEdit as SvcEdit, LaunchOrigin,
    Notice, OpenOrigin as SvcOrigin, QueryResult, ServiceHandle, StopMode,
};

use crate::bridge::{
    AfterCreate, ArtworkSource, Backend, CheckState, FolderScan, FolderTarget, GameDetail,
    GameEdit, HandlerState, HostCheck, HostStatus, HostView, LayerInfo, OpenOrigin,
    PolicyAssessment, QueryPayload, RuntimesView, SetupContext, SetupOrigin, ShortcutInfo,
    ToastLevel, ToastMsg, UiCommand, UiEvent,
};
use crate::strings::service as text;

struct State {
    events: Receiver<Event>,
    pending: VecDeque<UiEvent>,
    host: Option<(usize, Arc<HostView>)>,
    runtimes: Option<(usize, u64, Arc<RuntimesView>)>,
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
                    QueryResult::Failed(e) => QueryPayload::Failed(*e),
                },
            },
        })
    }

    fn update(&self, id: GameId, edits: Vec<GameEdit>) {
        let mut out = Vec::new();
        let mut skipped = false;
        for e in edits {
            out.push(match e {
                GameEdit::Name(n) => SvcEdit::Name(n),
                GameEdit::Notes(n) => SvcEdit::Notes(n),
                GameEdit::Policy(p) => SvcEdit::Policy(p),
                GameEdit::Launch(l) => SvcEdit::Launch(l),
                GameEdit::Runtime { reference, follow } => SvcEdit::Runtime { reference, follow },
                GameEdit::UmuId(u) => SvcEdit::UmuId(u),
                GameEdit::SteamAppId(a) => SvcEdit::SteamAppId(a),
                GameEdit::ResetName
                | GameEdit::Kind(_)
                | GameEdit::ExeRole(..)
                | GameEdit::ExeLabel(..)
                | GameEdit::RemoveExe(_) => {
                    skipped = true;
                    continue;
                }
            });
        }
        if !out.is_empty() {
            self.handle.send(Command::UpdateGame { id, edits: out });
        }
        if skipped {
            self.unsupported();
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
            fix_command: match &p.hint {
                Some(HostHint::Sysctl { setting }) => Some(text::sysctl(setting)),
                Some(HostHint::AppArmor { sysctl, .. }) => Some(text::sysctl(sysctl)),
                _ => None,
            },
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
            UiCommand::ScanFolder { folder, reply_to } => self.push(UiEvent::QueryResult {
                id: reply_to,
                payload: QueryPayload::Folder(FolderScan {
                    folder,
                    error: Some(text::SCAN_NOT_AVAILABLE.to_owned()),
                    ..FolderScan::default()
                }),
            }),
            UiCommand::MeasurePrefix { reply_to, .. } | UiCommand::SearchFixes { reply_to, .. } => {
                self.query_unsupported(reply_to)
            }
            UiCommand::Quit => {}
            UiCommand::AddExecutable { .. }
            | UiCommand::LocateExecutable { .. }
            | UiCommand::RescanMissing
            | UiCommand::CancelLaunch { .. }
            | UiCommand::RunTool { .. }
            | UiCommand::ClearStatistics
            | UiCommand::ResetGameStatistics { .. }
            | UiCommand::RemoveSession { .. } => self.unsupported(),
            UiCommand::SetDefaultHandler { enable } => {
                h.send(Command::SetDefaultHandler { enable });
            }
            UiCommand::SetShortcut { id, enable } => h.send(Command::SetShortcut { id, enable }),
            UiCommand::OpenFolder(t) => h.send(Command::OpenFolder(folder_request(t))),
            UiCommand::ViewLog { session, game } => h.send(Command::ViewLog { session, game }),
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

    fn host(&self) -> Arc<HostView> {
        let facts = self.handle.hub.host.load();
        let integration = self.handle.hub.integration.load();
        // Either snapshot changing rebuilds the view.
        let key =
            (Arc::as_ptr(&facts) as usize) ^ (Arc::as_ptr(&integration) as usize).rotate_left(17);
        let mut s = self.lock();
        if let Some((k, v)) = &s.host
            && *k == key
        {
            return Arc::clone(v);
        }
        let f = facts.as_ref().as_ref();
        let v = Arc::new(HostView {
            status: host_status(f),
            checks: host_checks(f),
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
        s.host = Some((key, Arc::clone(&v)));
        v
    }

    fn runtimes(&self) -> Arc<RuntimesView> {
        let cat = self.handle.hub.runtimes.load();
        let lib = self.handle.hub.library.load();
        let key = Arc::as_ptr(&cat) as usize;
        let mut s = self.lock();
        if let Some((k, rev, v)) = &s.runtimes
            && *k == key
            && *rev == lib.revision
        {
            return Arc::clone(v);
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
        });
        s.runtimes = Some((key, lib.revision, Arc::clone(&v)));
        v
    }

    fn assess(
        &self,
        _game: Option<GameId>,
        name: &str,
        root: Option<&Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        crate::demo::assess_in(&self.home, name, root, policy)
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

#[cfg(test)]
mod tests {
    use super::*;

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
            hint: Some(HostHint::Sysctl {
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
