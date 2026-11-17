//! Developer Mode views and diagnostics (DEVELOPER_MODE, SCREEN_SPECS_2 §15–16,
//! ERROR_HANDLING §4.11–4.13, §5–7): the launch inspector sheet ([`inspector`]), the log
//! viewer ([`logs`]) with the process tree ([`processes`]), the report preview ([`report`]),
//! recovery mode ([`recovery`]), the Doctor section of Settings → Advanced ([`doctor`]).
//!
//! The entry points here are what the rest of the UI calls: open a view, route a query
//! answer, show the standing banners.

pub mod doctor;
pub mod inspector;
pub mod logs;
pub mod processes;
pub mod recovery;
pub mod report;

use easyplay_core::ids::{GameId, QueryId, SessionId};

use super::Env;
use crate::app::UiState;
use crate::bridge::{Command, InspectTarget, LogFile, QueryPayload};
use crate::route::Overlay;
use crate::screens::shell::Banner;
use crate::strings::debug as s;
use crate::theme::Type;
use crate::vm::AsyncValue;
use crate::vm::debug::{InspectorVm, LogViewerVm, ReportVm};
use crate::widgets::banner::{Note, Severity};

/// Open the launch inspector for the next launch of `game` (dry run).
pub fn inspect_next(state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    open_inspector(
        state,
        env,
        InspectTarget::NextLaunch { game, exe: None },
        Some(game),
    );
}

/// Open the launch inspector.
pub fn open_inspector(
    state: &mut UiState,
    env: &mut Env<'_>,
    target: InspectTarget,
    game: Option<GameId>,
) {
    let q = env.bridge.query_id();
    env.bridge.send(Command::Inspect {
        target,
        reply_to: q,
    });
    state.debug.inspector = Some(InspectorVm::new(target, game, q, env.mono));
    if !state.nav.overlays().contains(&Overlay::Inspector) {
        state.nav.open(Overlay::Inspector);
    }
}

/// Start (or keep) the log viewer for `game`, showing `session` when given (else the newest).
pub fn prepare_logs(
    state: &mut UiState,
    env: &mut Env<'_>,
    game: GameId,
    session: Option<SessionId>,
) {
    let fresh = state
        .debug
        .logs
        .as_ref()
        .is_none_or(|l| l.game != Some(game));
    if fresh {
        let q = env.bridge.query_id();
        env.bridge.send(Command::ListSessions { game, reply_to: q });
        state.debug.logs = Some(LogViewerVm {
            game: Some(game),
            sessions_q: Some(q),
            follow: true,
            ..LogViewerVm::default()
        });
    }
    if let (Some(sid), Some(l)) = (session, state.debug.logs.as_mut())
        && l.session != Some(sid)
    {
        l.open(sid, LogFile::Game);
        request_chunk(l, env);
    }
}

/// *View log*: the viewer as a sheet (also without Developer Mode, LOGGING §3.3).
pub fn open_logs(state: &mut UiState, env: &mut Env<'_>, game: GameId, session: Option<SessionId>) {
    prepare_logs(state, env, game, session);
    if !state.nav.overlays().contains(&Overlay::Logs) {
        state.nav.open(Overlay::Logs);
    }
}

/// Ask for the next chunk of the viewer's file.
pub fn request_chunk(l: &mut LogViewerVm, env: &mut Env<'_>) {
    let (Some(session), Some(file)) = (l.session, l.file) else {
        return;
    };
    if l.query.is_some() {
        return;
    }
    let q = env.bridge.query_id();
    l.query = Some(q);
    l.polled = Some(env.mono);
    env.bridge.send(Command::ReadLog {
        session,
        file,
        offset: l.offset,
        reply_to: q,
    });
}

/// *Create a report…*: build the files, then show the preview.
pub fn open_report(
    state: &mut UiState,
    env: &mut Env<'_>,
    game: Option<GameId>,
    session: Option<SessionId>,
) {
    let q = env.bridge.query_id();
    env.bridge.send(Command::BuildReport {
        game,
        session,
        reply_to: q,
    });
    state.debug.report = Some(ReportVm {
        game,
        session,
        query: Some(q),
        ..ReportVm::default()
    });
    if !state.nav.overlays().contains(&Overlay::Report) {
        state.nav.open(Overlay::Report);
    }
}

/// Route a query answer to the debug views. Returns the payload when it is not theirs.
pub fn on_query(state: &mut UiState, id: QueryId, payload: QueryPayload) -> Option<QueryPayload> {
    let d = &mut state.debug;
    if let Some(i) = &mut d.inspector {
        if i.query == Some(id) {
            i.query = None;
            match payload {
                QueryPayload::Inspector(data) => {
                    if let Some(s) = data.session.as_ref().filter(|s| s.is_open()) {
                        i.procs.session = Some(s.id);
                    }
                    i.data = AsyncValue::Ready(data);
                }
                QueryPayload::Failed(e) => i.data = AsyncValue::Failed(e),
                _ => {}
            }
            return None;
        }
        if i.procs.query == Some(id) {
            i.procs.query = None;
            if let QueryPayload::Processes(p) = payload {
                i.procs.rows = p;
            }
            return None;
        }
    }
    if let Some(l) = &mut d.logs {
        if l.sessions_q == Some(id) {
            l.sessions_q = None;
            l.listed = true;
            if let QueryPayload::Sessions(v) = payload {
                l.sessions = v;
                if l.session.is_none()
                    && let Some(first) = l.sessions.first().map(|s| s.id)
                {
                    l.open(first, LogFile::Game);
                }
            }
            return None;
        }
        if l.query == Some(id) {
            match payload {
                QueryPayload::LogChunk(c) => {
                    // More is fetched by the viewer next frame (`more_wanted`).
                    let more = l.on_chunk(c);
                    if more {
                        l.polled = None;
                    }
                }
                QueryPayload::Failed(e) => {
                    l.query = None;
                    l.error = Some(e);
                }
                _ => l.query = None,
            }
            return None;
        }
    }
    if let Some(r) = &mut d.report
        && r.query == Some(id)
    {
        r.query = None;
        match payload {
            QueryPayload::Report(f) => r.files = Some(f),
            QueryPayload::Failed(e) => r.error = Some(e),
            _ => {}
        }
        return None;
    }
    Some(payload)
}

/// The standing banners of diagnostics: the crash of the last run, the read-only library.
pub fn banners(state: &mut UiState, env: &mut Env<'_>) {
    let diag = env.bridge.diagnostics().clone();
    if diag.crash.is_some() && !state.debug.crash_dismissed {
        if state.banners.is_dismissed("crash") {
            state.debug.crash_dismissed = true;
            env.bridge.send(Command::DismissCrash);
            state.banners.clear("crash");
        } else {
            state.banners.set(Banner {
                key: "crash",
                severity: Severity::Warning,
                text: s::CRASHED().to_owned(),
                action: Some(s::CREATE_REPORT().to_owned()),
                dismissible: true,
            });
        }
    } else {
        state.banners.clear("crash");
    }
    match diag.recovery.as_ref().map(|r| &r.reason) {
        Some(crate::bridge::RecoveryReason::MigrationFailed { .. }) => {
            state.banners.set(Banner {
                key: "recovery",
                severity: Severity::Danger,
                text: s::READ_ONLY().to_owned(),
                action: Some(s::CREATE_REPORT().to_owned()),
                dismissible: false,
            });
        }
        _ => state.banners.clear("recovery"),
    }
    if std::mem::take(&mut state.debug.open_report) {
        if state.banners.top().is_some_and(|b| b.key == "crash") {
            state.debug.crash_dismissed = true;
            env.bridge.send(Command::DismissCrash);
            state.banners.clear("crash");
        }
        open_report(state, env, None, None);
    }
}

/// A banner's action (shell): both diagnostics banners offer *Create a report…*.
pub fn banner_action(state: &mut UiState, key: &str) -> bool {
    if matches!(key, "crash" | "recovery") {
        state.debug.open_report = true;
        return true;
    }
    false
}

/// `Ctrl+L` on a game page in Developer Mode: the inspector for the next launch.
pub fn shortcuts(ctx: &egui::Context, state: &mut UiState, env: &mut Env<'_>) {
    if !env.bridge.prefs().advanced.developer_mode || !state.nav.overlays().is_empty() {
        return;
    }
    if let crate::route::Route::Game(g) = state.nav.current()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::L))
    {
        inspect_next(state, env, g);
    }
}

/// DEVELOPER_MODE §1.4: with Developer Mode off, a game that carries developer-only options
/// shows "Uses advanced options" with *Review* (the options keep working): a quiet note above
/// the tabs; *Review* lists the options under it.
pub fn advanced_note(ui: &mut egui::Ui, state: &mut UiState, env: &mut Env<'_>) {
    if env.bridge.prefs().advanced.developer_mode {
        return;
    }
    let Some(d) = state.details.record() else {
        return;
    };
    let opts = easyplay_core::sandbox::validate::advanced_options(&d.game.policy, &d.game.launch);
    if opts.is_empty() {
        return;
    }
    let p = crate::theme::theme(ui.ctx()).palette.clone();
    let open = &mut state.debug.review_open;
    let r = Note::new(Severity::Info, s::ADVANCED_NOTE())
        .actions(&[s::REVIEW()])
        .show(ui);
    if r.clicked.is_some() {
        *open = !*open;
    }
    if *open {
        ui.add_space(crate::theme::tokens::debug::CONTROLS_GAP);
        crate::widgets::modal::fine_print(ui, s::ADVANCED_HELP());
        for o in &opts {
            Type::Body.paragraph(ui, &o.describe(), p.t2);
        }
    }
    ui.add_space(crate::theme::tokens::game::NOTE_TOP);
}
