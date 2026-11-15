//! The log viewer (SCREEN_SPECS_2 §15, LOGGING §3.3; DEV-003): session selector, file,
//! filter with match count, wrap, follow-tail while the session runs; virtualized rows in
//! `mono`. Files load in chunks on the service's job pool (64 KiB first, then 1 MiB at a
//! time) and are treated as untrusted plain text (`vm::debug::sanitize`): no links, no
//! markup, control characters as `·`, lines capped at 4 096 bytes, at most 200 000 lines.
//!
//! Every line is classified by the program that wrote it and how serious it is
//! (`diagnostics::log_line`, ADR-0025): a level control shows that level and anything more
//! serious, a program selector one program's lines, and lines are coloured by level. Above
//! the log, how the session ended, in a sentence.
//!
//! Shown as the game page's **Logs** tab, and as a sheet from *View log* (problem panel,
//! Activity); Developer Mode adds the other files, the inspector and the process tree.

use easyplay_core::diagnostics::log_line::{LineLevel, LineSource};
use easyplay_core::ids::GameId;
use egui::{Context, Id, Key, Modifiers, ScrollArea, Ui};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{FolderTarget, InspectTarget, LogFile, UiCommand};
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::theme::tokens::space;
use crate::theme::{self, Palette, Type, icons};
use crate::vm::debug::LogViewerVm;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::segmented::{Segment, segmented_with_hints};
use crate::widgets::select::select;
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;

const FILES: [LogFile; 4] = [
    LogFile::Game,
    LogFile::Events,
    LogFile::Supervisor,
    LogFile::Proton,
];
const PLAIN_FILES: [LogFile; 2] = [LogFile::Game, LogFile::Proton];
const SOURCES: [Option<LineSource>; 5] = [
    None,
    Some(LineSource::Wine),
    Some(LineSource::Proton),
    Some(LineSource::Umu),
    Some(LineSource::Game),
];

/// How the shown session ended, and when it started.
fn ending(ui: &mut Ui, l: &LogViewerVm, env: &Env<'_>) {
    let Some(x) = l
        .session
        .and_then(|sid| l.sessions.iter().find(|x| x.id == sid))
    else {
        return;
    };
    let p = theme::theme(ui.ctx()).palette.clone();
    let (text, bad) = s::how_it_ended(x.end_reason, x.is_open());
    ui.horizontal_wrapped(|ui| {
        let (icon, colour) = if bad {
            (icons::WARNING, p.danger)
        } else {
            (icons::INFO, p.text_secondary)
        };
        ui.label(icons::icon(icon, theme::tokens::icon::INLINE).color(colour));
        ui.label(Type::BodyStrong.rich(text).color(p.text_primary));
    });
    let started = crate::strings::fmt::relative_date(x.started_at, env.now, env.utc_offset_s);
    let took = x.duration_ms.map(crate::strings::fmt::duration);
    ui.label(
        Type::Caption
            .rich(s::ran(&started, took.as_deref()))
            .color(p.text_secondary),
    );
}

fn level_colour(level: LineLevel, p: &Palette) -> egui::Color32 {
    match level {
        LineLevel::Error => p.danger,
        LineLevel::Warning => p.warning,
        LineLevel::Info => p.text_primary,
        LineLevel::Debug => p.text_muted,
    }
}

/// Load what is due: the first chunk, the rest of a file, the tail of a followed one.
fn tick(l: &mut LogViewerVm, env: &mut Env<'_>) {
    if l.query.is_some() || l.session.is_none() {
        return;
    }
    if l.polled.is_none() || l.poll_due(env.mono) {
        super::request_chunk(l, env);
    }
    if l.follow && l.running() {
        env.ctx.request_repaint_after(crate::vm::debug::FOLLOW_POLL);
    }
}

/// The viewer body. `developer` adds the inspector and process-tree actions.
pub fn viewer(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, developer: bool, height: f32) {
    let Some(mut l) = state.debug.logs.take() else {
        return;
    };
    tick(&mut l, env);
    let p = theme::theme(ui.ctx()).palette.clone();
    if l.listed && l.sessions.is_empty() {
        ui.label(Type::Body.rich(s::NO_SESSIONS).color(p.text_secondary));
        state.debug.logs = Some(l);
        return;
    }
    // Selectors.
    let mut open_inspector = None;
    ui.horizontal_wrapped(|ui| {
        let labels: Vec<String> = l
            .sessions
            .iter()
            .map(|x| {
                s::session_label(
                    &crate::strings::fmt::relative_date(x.started_at, env.now, env.utc_offset_s),
                    x.kind.as_str(),
                    &x.end_reason
                        .map(|r| crate::strings::launch::ended(r).to_lowercase())
                        .unwrap_or_else(|| {
                            if x.is_open() {
                                s::RUNNING.to_owned()
                            } else {
                                String::new()
                            }
                        }),
                )
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut sel = l
            .session
            .and_then(|sid| l.sessions.iter().position(|x| x.id == sid))
            .unwrap_or(0);
        if !refs.is_empty()
            && select(
                ui,
                s::SESSION,
                Some(s::SESSION),
                None,
                &mut sel,
                &refs,
                true,
            )
            .changed()
            && let Some(x) = l.sessions.get(sel)
        {
            let f = l.file.unwrap_or(LogFile::Game);
            l.open(x.id, f);
        }
        let (files, names): (&[LogFile], &[&str]) = if developer {
            (&FILES, &s::LOG_FILES)
        } else {
            (&PLAIN_FILES, &s::PLAIN_LOG_FILES)
        };
        let mut fsel = files.iter().position(|f| Some(*f) == l.file).unwrap_or(0);
        if select(
            ui,
            s::LOG_FILE,
            Some(s::LOG_FILE),
            None,
            &mut fsel,
            names,
            true,
        )
        .changed()
            && let Some(sid) = l.session
        {
            l.open(sid, files[fsel]);
        }
        let mut src = SOURCES.iter().position(|x| *x == l.source).unwrap_or(0);
        if select(
            ui,
            s::SOURCE,
            Some(s::SOURCE),
            None,
            &mut src,
            &s::SOURCES,
            true,
        )
        .changed()
        {
            l.source = SOURCES[src];
        }
        let filter = TextField::new(&mut l.filter, "log.filter")
            .accessible_name(s::FILTER)
            .hint(s::FILTER)
            .search()
            .width(200.0)
            .show(ui)
            .response;
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            filter.request_focus();
        }
        ui.checkbox(&mut l.wrap, s::WRAP);
        if l.running() {
            ui.checkbox(&mut l.follow, s::FOLLOW);
        }
        if developer && let Some(sid) = l.session {
            open_inspector = Some(sid);
        }
    });
    ui.add_space(space::S2);
    let mut lvl = LineLevel::ALL
        .iter()
        .position(|x| *x == l.level)
        .unwrap_or(2);
    let levels: Vec<Segment<'_>> = s::LEVELS
        .iter()
        .map(|label| Segment { icon: None, label })
        .collect();
    if segmented_with_hints(ui, s::LEVEL, &mut lvl, &levels, &s::LEVEL_HINTS).changed() {
        l.level = LineLevel::ALL[lvl];
    }
    ui.add_space(space::S2);
    ending(ui, &l, env);
    ui.add_space(space::S2);
    if let Some(e) = &l.error {
        ui.label(Type::Body.rich(&e.message).color(p.text_secondary));
    } else if l.missing && l.file == Some(LogFile::Proton) {
        ui.label(Type::Body.rich(s::NO_PROTON_LOG).color(p.text_secondary));
    } else if l.missing {
        ui.label(Type::Body.rich(s::LOG_REMOVED).color(p.text_secondary));
    } else if l.lines.is_empty() && l.query.is_none() && l.polled.is_some() {
        ui.label(Type::Body.rich(s::LOG_EMPTY).color(p.text_secondary));
    }
    if ui.input(|i| i.key_pressed(Key::End)) {
        l.follow = true;
    }
    let visible = l.visible();
    let row_h = Type::Mono.font_id().size + 4.0;
    let frame = egui::Frame::NONE
        .fill(p.bg_sunken)
        .corner_radius(4)
        .inner_margin(egui::Margin::same(space::S2 as i8));
    frame.show(ui, |ui| {
        ui.set_min_height(height);
        let area = ScrollArea::both()
            .id_salt("log-viewer")
            .auto_shrink([false, false])
            .max_height(height)
            .stick_to_bottom(l.follow);
        if l.wrap {
            area.show(ui, |ui| {
                for i in &visible {
                    let colour = level_colour(l.kinds[*i].level, &p);
                    ui.add(
                        egui::Label::new(Type::Mono.rich(l.lines[*i].as_str()).color(colour))
                            .selectable(true)
                            .wrap(),
                    );
                }
            });
        } else {
            area.show_rows(ui, row_h, visible.len(), |ui, range| {
                for i in &visible[range] {
                    let colour = level_colour(l.kinds[*i].level, &p);
                    ui.add(
                        egui::Label::new(Type::Mono.rich(l.lines[*i].as_str()).color(colour))
                            .selectable(true)
                            .extend(),
                    );
                }
            });
        }
    });
    let (errors, warnings) = l.counts();
    let mut status = format!(
        "{} · {}",
        s::line_count(
            l.lines.len(),
            visible.len(),
            !l.filter.is_empty() || l.narrowed()
        ),
        s::counts(errors, warnings)
    );
    if l.query.is_some() {
        status = format!("{status} · {}", s::LOADING);
    }
    ui.label(Type::Caption.rich(status).color(p.text_secondary));
    if l.dropped > 0 {
        ui.label(
            Type::Caption
                .rich(s::dropped(l.dropped))
                .color(p.text_secondary),
        );
    }
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        if let Some(sid) = l.session
            && ui.add(Button::new(s::OPEN_FOLDER)).clicked()
        {
            env.bridge
                .send(UiCommand::OpenFolder(FolderTarget::Session(sid)));
        }
        if ui.add(Button::new(s::COPY_VISIBLE)).clicked() {
            let text: Vec<&str> = visible.iter().map(|i| l.lines[*i].as_str()).collect();
            env.ctx.copy_text(text.join("\n"));
            state.toast(
                crate::strings::feedback::COPIED,
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        if let Some(sid) = open_inspector
            && ui.add(Button::new(s::OPEN_INSPECTOR)).clicked()
        {
            let game = l.game;
            state.debug.logs = Some(l.clone());
            super::open_inspector(state, env, InspectTarget::Session(sid), game);
        }
    });
    if state.debug.logs.is_none() {
        state.debug.logs = Some(l);
    }
}

/// The Logs tab of the game page.
pub fn tab(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    super::prepare_logs(state, env, game, None);
    let developer = env.bridge.prefs().advanced.developer_mode;
    let h = (ui.available_height() - 200.0).max(320.0);
    viewer(ui, state, env, developer, h);
    let proton_log = state
        .details
        .record()
        .is_some_and(|d| d.game.launch.compat.proton_log);
    if !proton_log {
        let p = theme::theme(ui.ctx()).palette.clone();
        ui.add_space(space::S2);
        ui.horizontal_wrapped(|ui| {
            ui.label(Type::Caption.rich(s::MORE_DETAIL).color(p.text_secondary));
            if ui
                .add(Button::ghost(s::TO_COMPATIBILITY).size(crate::widgets::button::Size::Sm))
                .clicked()
            {
                state.navigate(crate::vm::dialogs::AfterDiscard::Tab(
                    crate::vm::details::DetailsTab::Compatibility.index(),
                ));
            }
        });
    }
}

/// The sheet of *View log* (overlay `Logs`).
pub fn sheet_view(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let title = state
        .debug
        .logs
        .as_ref()
        .and_then(|l| l.game)
        .and_then(|g| env.bridge.library().get(g).map(|x| s::logs_of(&x.name)))
        .unwrap_or_else(|| s::LOGS.to_owned());
    let developer = env.bridge.prefs().advanced.developer_mode;
    let r = sheet(
        ctx,
        id,
        &title,
        false,
        |ui| viewer(ui, state, env, developer, 360.0),
        |_| {},
    );
    if r.close_requested {
        state.nav.close(Overlay::Logs);
    }
}
