//! The log viewer (easyplay-redesign.html `tabLogs`, `logView`; SCREEN_SPECS_2 §15, LOGGING
//! §3.3; DEV-003). A bar: the level (errors, warnings, info, debug: that level and anything
//! more serious), a filter field, *Wrap* and *Copy*; the log in a well, 12 pt mono on 19 pt
//! lines, the supervisor's time stamps quiet, errors coral and warnings amber; under it one
//! line of counts and help. Above the bar, when there is a choice to make: which session,
//! which file, which program's lines, and whether to follow a running session's tail.
//!
//! Files load in chunks on the service's job pool (64 KiB first, then 1 MiB at a time) and
//! are treated as untrusted plain text (`vm::debug::sanitize`): no links, no markup, control
//! characters as `·`, lines capped at 4 096 bytes, at most 200 000 lines. Every line is
//! classified by the program that wrote it and how serious it is (`diagnostics::log_line`,
//! ADR-0025).
//!
//! Shown as the game page's **Logs** tab, and as a sheet from *View log* (the problem note,
//! Activity); Developer Mode adds the other files and the inspector.

use easyplay_core::diagnostics::log_line::{LineLevel, LineSource, without_time};
use easyplay_core::ids::GameId;
use egui::text::LayoutJob;
use egui::{
    Align, Color32, Context, CornerRadius, Id, Key, Layout, Margin, Modifiers, Rect, ScrollArea,
    Sense, Ui, UiBuilder, pos2, vec2,
};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{Command, FolderRequest, InspectTarget, LogFile};
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::strings::details as d;
use crate::theme::tokens::{control, game as gm, layout, radius, stroke};
use crate::theme::{self, Palette, Type};
use crate::vm::debug::LogViewerVm;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::choice::checkbox;
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

/// Where the viewer is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// The Logs tab of the game page.
    Tab,
    /// The sheet of *View log*.
    Sheet,
}

/// The colour of a line's text by how serious it is.
fn level_colour(level: LineLevel, p: &Palette) -> Color32 {
    match level {
        LineLevel::Error => p.coral,
        LineLevel::Warning => p.amber,
        LineLevel::Info | LineLevel::Debug => p.t2,
    }
}

/// One line of the log: its time stamp quiet, the rest in its level's colour.
fn line_job(line: &str, level: LineLevel, p: &Palette, wrap: Option<f32>) -> LayoutJob {
    let mut f = Type::Mono.format(level_colour(level, p), false);
    f.line_height = Some(gm::LOG_LINE);
    let rest = without_time(line);
    let stamp = &line[..line.len() - rest.len()];
    let mut job = LayoutJob::default();
    if !stamp.is_empty() {
        let mut q = f.clone();
        q.color = p.t3;
        job.append(stamp, 0.0, q);
    }
    job.append(rest, 0.0, f);
    if let Some(w) = wrap {
        job.wrap.max_width = w;
        job.wrap.break_anywhere = true;
    }
    job
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

/// Which session, which file, which program's lines, and following the tail.
fn choices(ui: &mut Ui, l: &mut LogViewerVm, env: &Env<'_>, developer: bool) {
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
                            s::RUNNING().to_owned()
                        } else {
                            String::new()
                        }
                    }),
            )
        })
        .collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(gm::LOG_BAR[0], gm::LOG_BAR[0]);
        let mut sel = l
            .session
            .and_then(|sid| l.sessions.iter().position(|x| x.id == sid))
            .unwrap_or(0);
        if !refs.is_empty()
            && select(
                ui,
                s::SESSION(),
                Some(s::SESSION()),
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
            (&PLAIN_FILES, &s::PLAIN_LOG_FILES())
        };
        let mut fsel = files.iter().position(|f| Some(*f) == l.file).unwrap_or(0);
        if select(
            ui,
            s::LOG_FILE(),
            Some(s::LOG_FILE()),
            None,
            &mut fsel,
            names,
            true,
        )
        .changed()
            && let Some(sid) = l.session
        {
            l.open(sid, files[fsel.min(files.len() - 1)]);
        }
        let mut src = SOURCES.iter().position(|x| *x == l.source).unwrap_or(0);
        if select(
            ui,
            s::SOURCE(),
            Some(s::SOURCE()),
            None,
            &mut src,
            &s::SOURCES(),
            true,
        )
        .changed()
        {
            l.source = SOURCES[src.min(SOURCES.len() - 1)];
        }
        if l.running() {
            checkbox(ui, &mut l.follow, s::FOLLOW());
        }
    });
    ui.add_space(gm::LOG_BAR[1]);
}

/// The bar: level, filter, then *Wrap* and *Copy* at the right end. Returns whether
/// *Copy* was pressed.
fn bar(ui: &mut Ui, l: &mut LogViewerVm) -> bool {
    let w = ui.available_width();
    let mut copy = false;
    let levels: Vec<Segment<'_>> = d::LOG_LEVELS()
        .iter()
        .map(|label| Segment { icon: None, label })
        .collect();
    let top = ui.cursor().min;
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt("log.bar")
            .max_rect(Rect::from_min_size(top, vec2(w, control::BUTTON)))
            .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
    );
    row.spacing_mut().item_spacing = vec2(gm::LOG_BAR[0], gm::LOG_BAR[0]);
    let mut lvl = LineLevel::ALL
        .iter()
        .position(|x| *x == l.level)
        .unwrap_or(2);
    if segmented_with_hints(
        &mut row,
        d::LOG_LEVEL(),
        &mut lvl,
        &levels,
        &s::LEVEL_HINTS(),
    )
    .changed()
    {
        l.level = LineLevel::ALL[lvl.min(LineLevel::ALL.len() - 1)];
    }
    let filter = TextField::new(&mut l.filter, "log.filter")
        .accessible_name(d::LOG_FILTER_NAME())
        .hint(d::LOG_FILTER())
        .search()
        .width(gm::LOG_FILTER_W)
        .show(&mut row)
        .response;
    if row.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
        filter.request_focus();
    }
    // Wrap and Copy stand at the right end of whatever room the line has left (the cursor
    // already stands a gap past the filter; a wrapping line's available width is the whole
    // line's, so the room is measured from the cursor).
    let end_w = checkbox_width(&row) + Button::ghost(d::LOG_COPY()).width(&row) + gm::LOG_BAR[0];
    let room = row.max_rect().right() - row.cursor().left();
    if room >= end_w {
        // Whole points, so rounding never pushes them onto a line of their own.
        row.add_space((room - end_w).floor());
    }
    checkbox(&mut row, &mut l.wrap, d::LOG_WRAP());
    if row.add(Button::ghost(d::LOG_COPY())).clicked() {
        copy = true;
    }
    let used = row.min_rect();
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + w, used.bottom())),
        Sense::hover(),
    );
    ui.add_space(gm::LOG_BAR[1]);
    copy
}

/// The width of the *Wrap* checkbox button.
fn checkbox_width(ui: &Ui) -> f32 {
    let g = Type::BodyStrong.line(ui, d::LOG_WRAP(), Color32::PLACEHOLDER);
    control::BUTTON_PAD * 2.0 + control::CHECK + control::BUTTON_GAP + g.size().x
}

/// The log in its well: at least `min_h` and at most `max_h` high, scrolling.
fn view(ui: &mut Ui, l: &LogViewerVm, visible: &[usize], (min_h, max_h): (f32, f32)) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [px, py] = gm::LOG_PAD;
    let frame = egui::Frame::NONE
        .fill(p.well)
        .stroke(egui::Stroke::new(stroke::BORDER, p.line))
        .corner_radius(CornerRadius::same(radius::POP))
        .inner_margin(Margin::symmetric(px as i8, py as i8));
    let r = frame.show(ui, |ui| {
        // Lines follow one another at the log's line height, wherever the view is shown.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_width(ui.available_width());
        // The frame's outline is outside its margin: the well is `min_h` with it.
        ui.set_min_height(min_h - (py + stroke::BORDER) * 2.0);
        let empty = if l.error.is_some() {
            l.error.as_ref().map(|e| e.message.clone())
        } else if l.missing && l.file == Some(LogFile::Proton) {
            Some(s::NO_PROTON_LOG().to_owned())
        } else if l.missing {
            Some(s::LOG_REMOVED().to_owned())
        } else if l.lines.is_empty() && l.query.is_none() {
            Some(d::LOG_NOTHING().to_owned())
        } else if visible.is_empty() && !l.lines.is_empty() {
            Some(d::LOG_NO_MATCH().to_owned())
        } else {
            None
        };
        if let Some(text) = empty {
            // In the text face at the log's size and line.
            let mut f = Type::Caption.format(p.t3, false);
            f.line_height = Some(gm::LOG_LINE);
            let mut job = LayoutJob::single_section(text, f);
            job.wrap.max_width = ui.available_width();
            ui.add(egui::Label::new(job).wrap());
            return;
        }
        let area = ScrollArea::both()
            .id_salt("log-viewer")
            .auto_shrink([false, true])
            .max_height(max_h - (py + stroke::BORDER) * 2.0)
            .stick_to_bottom(l.follow);
        if l.wrap {
            area.show(ui, |ui| {
                let w = ui.available_width();
                for i in visible {
                    let level = l.kinds.get(*i).map_or(LineLevel::Info, |k| k.level);
                    let job = line_job(&l.lines[*i], level, &p, Some(w));
                    ui.add(egui::Label::new(job).selectable(true).wrap());
                }
            });
        } else {
            area.show_rows(ui, gm::LOG_LINE, visible.len(), |ui, range| {
                for i in &visible[range] {
                    let level = l.kinds.get(*i).map_or(LineLevel::Info, |k| k.level);
                    let job = line_job(&l.lines[*i], level, &p, None);
                    ui.add(egui::Label::new(job).selectable(true).extend());
                }
            });
        }
    });
    crate::a11y::labelled(ui.ctx(), r.response.id, egui::Role::Document, d::LOG_VIEW());
}

/// The line under the log: what is shown and, on the tab, where more detail comes from
/// ("Compatibility" is a link inside the sentence). Returns whether the link was pressed.
fn fine_print(ui: &mut Ui, l: &LogViewerVm, visible: usize, more_detail: bool) -> bool {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (errors, warnings) = l.counts();
    let mut said = d::log_counts(visible, l.lines.len(), errors, warnings);
    if l.dropped > 0 {
        said = format!("{said} {}", s::dropped(l.dropped));
    }
    if l.query.is_some() {
        said = format!("{said} {}", s::LOADING());
    }
    let fine = |c: Color32| {
        let mut f = Type::Caption.format(c, false);
        f.line_height = Some(gm::FINE_LINE);
        f
    };
    let mut job = LayoutJob::default();
    job.append(&said, 0.0, fine(p.t3));
    let link_at = said.chars().count() + d::LOG_MORE_BEFORE().chars().count();
    if more_detail {
        job.append(d::LOG_MORE_BEFORE(), 0.0, fine(p.t3));
        job.append(d::LOG_MORE_LINK(), 0.0, fine(p.accent_text));
        job.append(d::LOG_MORE_AFTER(), 0.0, fine(p.t3));
    }
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    let spoken = galley.text().to_owned();
    crate::a11y::item(&resp, egui::Role::Label, &spoken);
    if !more_detail {
        ui.painter().galley(rect.min, galley, p.t3);
        return false;
    }
    // The link's place in the sentence, row by row.
    use egui::text::CCursor;
    let end = link_at + d::LOG_MORE_LINK().chars().count();
    let a = galley.pos_from_cursor(CCursor::new(link_at));
    let b = galley.pos_from_cursor(CCursor::new(end));
    let spans = if (a.top() - b.top()).abs() < 1.0 {
        vec![Rect::from_min_max(a.min, pos2(b.right(), a.bottom()))]
    } else {
        let row_end = |y: f32| {
            galley
                .rows
                .iter()
                .find(|r| (r.pos.y - y).abs() < 1.0 || r.rect().contains(pos2(a.left(), y + 1.0)))
                .map_or(a.right(), |r| r.rect().right())
        };
        vec![
            Rect::from_min_max(a.min, pos2(row_end(a.top()), a.bottom())),
            Rect::from_min_max(pos2(0.0, b.top()), pos2(b.right(), b.bottom())),
        ]
    };
    ui.painter().galley(rect.min, galley, p.t3);
    let mut pressed = false;
    for (i, span) in spans.iter().enumerate() {
        let r = span.translate(rect.min.to_vec2());
        let link = ui.interact(r, Id::unique(("log.more", i)), Sense::click());
        crate::a11y::labelled(ui.ctx(), link.id, egui::Role::Link, d::LOG_MORE_LINK());
        if link.hovered() || link.has_focus() {
            ui.painter().hline(
                r.x_range(),
                r.bottom() - stroke::BORDER,
                egui::Stroke::new(stroke::BORDER, p.accent_text),
            );
        }
        pressed |= link.clicked();
    }
    pressed
}

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
    let started = crate::strings::fmt::relative_date(x.started_at, env.now, env.utc_offset_s);
    let took = x.duration_ms.map(crate::strings::fmt::play_time);
    let line = format!("{text} {}", s::ran(&started, took.as_deref()));
    crate::screens::details::fine(ui, &line, if bad { p.coral } else { p.t3 });
}

/// The viewer. `developer` adds the other files and the inspector.
fn viewer(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    developer: bool,
    place: Place,
    heights: (f32, f32),
) {
    let Some(mut l) = state.debug.logs.take() else {
        return;
    };
    tick(&mut l, env);
    if l.listed && l.sessions.is_empty() {
        let visible: Vec<usize> = Vec::new();
        bar(ui, &mut l);
        view(ui, &l, &visible, heights);
        ui.add_space(gm::LOG_NOTE_TOP);
        let proton_log = state
            .details
            .record()
            .is_some_and(|d| d.game.launch.compat.proton_log);
        let to_compat = fine_print(ui, &l, 0, place == Place::Tab && !proton_log);
        state.debug.logs = Some(l);
        if to_compat {
            state.navigate(crate::vm::dialogs::AfterDiscard::Tab(
                crate::vm::details::DetailsTab::Compatibility.index(),
            ));
        }
        return;
    }
    if l.sessions.len() > 1 || developer || l.running() {
        choices(ui, &mut l, env, developer);
    }
    let copy = bar(ui, &mut l);
    if ui.input(|i| i.key_pressed(Key::End)) {
        l.follow = true;
    }
    let visible = l.visible();
    view(ui, &l, &visible, heights);
    ui.add_space(gm::LOG_NOTE_TOP);
    let proton_log = state
        .details
        .record()
        .is_some_and(|d| d.game.launch.compat.proton_log);
    let to_compat = fine_print(ui, &l, visible.len(), place == Place::Tab && !proton_log);
    ending(ui, &l, env);
    if copy {
        let text: Vec<&str> = visible.iter().map(|i| l.lines[*i].as_str()).collect();
        env.ctx.copy_text(text.join("\n"));
        state.toast(
            crate::strings::feedback::COPIED(),
            ToastKind::Success,
            None,
            env.mono,
        );
    }
    // The session's folder, and the inspector in Developer Mode.
    let mut open_inspector = None;
    if let Some(sid) = l.session {
        ui.add_space(gm::LOG_BAR[0]);
        let at = ui.cursor().min;
        let w = ui.available_width();
        let mut row = ui.new_child(
            UiBuilder::new()
                .id_salt("log.actions")
                .max_rect(Rect::from_min_size(
                    pos2(at.x - gm::ROW_BUTTONS[1], at.y),
                    vec2(w + gm::ROW_BUTTONS[1], f32::INFINITY),
                ))
                .layout(Layout::left_to_right(Align::Min)),
        );
        if row.add(Button::ghost(s::OPEN_FOLDER())).clicked() {
            env.bridge
                .send(Command::OpenFolder(FolderRequest::Session(sid)));
        }
        if developer && row.add(Button::ghost(s::OPEN_INSPECTOR())).clicked() {
            open_inspector = Some(sid);
        }
        let h = row.min_rect().height();
        ui.allocate_exact_size(vec2(w, h), Sense::hover());
    }
    if let Some(sid) = open_inspector {
        let game = l.game;
        state.debug.logs = Some(l.clone());
        super::open_inspector(state, env, InspectTarget::Session(sid), game);
    }
    if state.debug.logs.is_none() {
        state.debug.logs = Some(l);
    }
    if to_compat {
        state.navigate(crate::vm::dialogs::AfterDiscard::Tab(
            crate::vm::details::DetailsTab::Compatibility.index(),
        ));
    }
}

/// The Logs tab of the game page.
pub fn tab(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    super::prepare_logs(state, env, game, None);
    let developer = env.bridge.prefs().advanced.developer_mode;
    let [min_h, max_h] = layout::LOG_H;
    viewer(ui, state, env, developer, Place::Tab, (min_h, max_h));
}

/// The sheet of *View log* (overlay `Logs`).
pub fn sheet_view(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let title = state
        .debug
        .logs
        .as_ref()
        .and_then(|l| l.game)
        .and_then(|g| env.bridge.library().get(g).map(|x| s::logs_of(&x.name)))
        .unwrap_or_else(|| s::LOGS().to_owned());
    let developer = env.bridge.prefs().advanced.developer_mode;
    let [min_h, max_h] = layout::LOG_H;
    let r = sheet(
        ctx,
        id,
        &title,
        false,
        |ui| viewer(ui, state, env, developer, Place::Sheet, (min_h, max_h)),
        |_| {},
    );
    if r.close_requested {
        state.nav.close(Overlay::Logs);
    }
}
