//! The game page (easyplay-redesign.html §4 "GAME PAGE", CSS §7, script §9): two columns
//! under the top bar, neither of them a box. The left column holds the cover and, under it,
//! the play record; it does not scroll. The right column scrolls on its own: the back link,
//! the title, the protection level and the tags, the actions (Play, favourite, more), the
//! notes (a problem, a missing program, a game still running in the background), then the
//! tabs and the selected tab's body. Once the actions have scrolled away the tab bar stays
//! at the column's top with a compact Play at its right end.
//!
//! The page renders at once from the `GameSummary`; the full record and the thirty days of
//! the play record fill in. On arrival the title, the actions and the tabs rise into place;
//! a change of run state, tags or protection only redraws the part it touches.

pub mod activity;
pub mod launch_options;
pub mod launch_panel;
pub mod overview;
pub mod permissions;
pub mod runtime;
pub mod steam;

use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, ArtSlot, Availability, GameSummary};
use egui::emath::TSTransform;
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Key, Layout, Modifiers, Popup, Rect, ScrollArea,
    Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};

use super::Env;
use super::actions::{self, GameAction};
use crate::app::UiState;
use crate::bridge::{GameEdit, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::strings;
use crate::strings::details as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{
    control, game as gm, icon as icon_size, layout, radius, space, stroke, travel,
};
use crate::theme::{self, Dur, Icon, Type, icons, motion};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, DetailsVm, RECORD_RANGE, range_for, record_days};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, PillSize, PlayState, play_pill};
use crate::widgets::card::{play_state, tick_elapsed};
use crate::widgets::context_menu::item;
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::icon_button::IconButton;
use crate::widgets::tabs::Tabs;
use crate::widgets::word::Word;
use crate::widgets::{focus_ring, hover_t};

/// Reset the page state for `id` and ask for its record and playtime.
fn enter(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let tab = if state.details.game.is_none() {
        state.details.tab
    } else {
        DetailsTab::Overview
    };
    state.details = DetailsVm {
        game: Some(id),
        tab,
        ..DetailsVm::default()
    };
    load(state, env, id);
    overview::request(state, env, id);
}

pub(super) fn load(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let q = env.bridge.query_id();
    state.details.detail_q = Some(q);
    if !matches!(state.details.detail, AsyncValue::Ready(_)) {
        state.details.detail = AsyncValue::Loading { since: env.mono };
    }
    env.bridge.send(UiCommand::LoadGame { id, reply_to: q });
    let q = env.bridge.query_id();
    state.details.about_q = Some(q);
    if !matches!(state.details.about, AsyncValue::Ready(_)) {
        state.details.about = AsyncValue::Loading { since: env.mono };
    }
    env.bridge.send(UiCommand::AboutGame { id, reply_to: q });
}

/// Keep the play record's thirty days of `g` current: asked for when the page opens and
/// again whenever a session has been counted. The days on screen stay until their
/// successors arrive.
fn keep_days(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let stamp = (g.session_count, g.total_playtime_ms);
    if state.details.week_q.is_some() || state.details.week_for == Some(stamp) {
        return;
    }
    let Some(range) = range_for(RECORD_RANGE, env.now, env.utc_offset_s) else {
        return;
    };
    let q = env.bridge.query_id();
    state.details.week_q = Some(q);
    state.details.week_for = Some(stamp);
    if state.details.week.ready().is_none() {
        state.details.week = AsyncValue::Loading { since: env.mono };
    }
    env.bridge.send(UiCommand::QueryStats {
        request: easyplay_core::stats::StatsRequest::GameOverview { game: g.id, range },
        reply_to: q,
    });
}

// ---- Measures of the page ----------------------------------------------------------------

/// How the page is laid out for the width it has.
#[derive(Clone, Copy, Debug)]
struct Frame {
    /// The cover, when the page is wide enough to have a cover column.
    cover: Option<Rect>,
    /// The scrolling column, padding included.
    column: Rect,
    /// The scrolling column's padding: left, right, bottom.
    pad: [f32; 3],
    /// The page area is narrower than [`layout::MEDIUM_BELOW`].
    medium: bool,
    /// The page area is narrower than [`layout::NARROW_BELOW`].
    narrow: bool,
}

fn frame(page: Rect) -> Frame {
    let w = page.width();
    let narrow = w < layout::NARROW_BELOW;
    let medium = w < layout::MEDIUM_BELOW;
    let top = page.top() + layout::GAME_TOP;
    if narrow {
        return Frame {
            cover: None,
            column: Rect::from_min_max(pos2(page.left(), top), page.max),
            // The gutter is the narrow one on both sides.
            pad: [
                layout::GUTTER_NARROW,
                layout::GUTTER_NARROW,
                layout::GAME_BOTTOM,
            ],
            medium,
            narrow,
        };
    }
    let (cw, gap) = if medium {
        (layout::GAME_COVER_MEDIUM, layout::GAME_GAP_MEDIUM)
    } else {
        (layout::GAME_COVER, layout::GAME_GAP)
    };
    // The cover and the column are one block, which stands in the middle of a wider page.
    let block = cw + gap + layout::GAME_COL_PAD + layout::GAME_COL_MAX;
    let indent = crate::screens::centre_indent(w - layout::GUTTER * 2.0, block);
    let left = page.left() + layout::GUTTER + indent;
    let cover = Rect::from_min_size(pos2(left, top), vec2(cw, cw * layout::COVER_ASPECT));
    Frame {
        cover: Some(cover),
        column: Rect::from_min_max(pos2(left + cw + gap, top), page.max),
        pad: [layout::GAME_COL_PAD, layout::GUTTER, layout::GAME_BOTTOM],
        medium,
        narrow,
    }
}

/// Where the page puts its cover in a page area `page`, if it shows one: where a cover
/// flying in from the wall is headed before the page has been laid out.
pub fn cover_slot(page: Rect) -> Option<Rect> {
    frame(page).cover
}

/// The title's type for the page's width.
fn title_type(f: &Frame) -> Type {
    if f.narrow {
        Type::DisplaySmall
    } else if f.medium {
        Type::DisplayMedium
    } else {
        Type::Display
    }
}

// ---- Shared parts of the tabs ----------------------------------------------------------------

/// How a note is tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoteTone {
    /// Coral: a problem.
    Problem,
    /// Amber: a missing program, a warning.
    Warning,
    /// `o1`: information ("still running in the background").
    Info,
}

/// A note: a tinted block with a glyph, a bold title (and a code after it), its text, and up
/// to a few ghost buttons. `break_after_title` puts the text on a line of its own.
pub(super) struct Note<'a> {
    /// Tint and glyph colour.
    pub tone: NoteTone,
    /// The glyph.
    pub icon: Icon,
    /// The bold title.
    pub title: &'a str,
    /// A code after the title (`EP-SBX-010`).
    pub code: Option<&'a str>,
    /// The text.
    pub text: &'a str,
    /// The text starts on a line of its own.
    pub break_after_title: bool,
    /// Ghost buttons under the text.
    pub actions: &'a [&'a str],
}

/// What was done with a note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NoteOut {
    /// The action pressed.
    pub action: Option<usize>,
    /// The code after the title was pressed: copy the details.
    pub copy: bool,
}

/// Show a note (CSS `.note`): radius 10, padding 10 10 10 12, glyph and text 10 apart, line
/// height 19; its actions wrap under the text.
pub(super) fn note(ui: &mut Ui, n: &Note<'_>) -> NoteOut {
    // The shared note draws every note but a problem's, whose code is a button of its own
    // (it copies the details).
    if n.tone != NoteTone::Problem {
        use crate::widgets::banner::{Note as Plate, Severity};
        let plate = match n.tone {
            NoteTone::Warning => Plate::missing(n.title, n.text),
            _ if n.icon == icons::CLOCK => Plate::lingering(n.title, n.text),
            _ => Plate::new(Severity::Info, n.text).title(n.title),
        };
        let action = plate.actions(n.actions).show(ui).clicked;
        return NoteOut {
            action,
            copy: false,
        };
    }
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let w = ui.available_width();
    let [pad_l, pad] = gm::NOTE_PAD;
    let text_x = pad_l + icon_size::INLINE + gm::NOTE_GAP;
    let text_w = (w - text_x - pad).max(space::S16);
    let line = |t: Type, c: Color32| {
        let mut f = t.format(c, false);
        f.line_height = Some(gm::NOTE_LINE);
        f
    };
    let mut job = LayoutJob::default();
    job.append(
        &theme::typography::shaped(n.title),
        0.0,
        line(Type::BodyBold, p.t1),
    );
    if let Some(code) = n.code {
        let mut f = line(Type::Mono, p.t3);
        f.font_id.size = gm::NOTE_CODE[0];
        job.append(code, gm::NOTE_CODE[1], f);
    }
    if !n.text.is_empty() {
        let sep = if n.break_after_title || crate::i18n::bidi::has_rtl(n.text) {
            "\n"
        } else {
            " "
        };
        // Right-to-left text arrives broken into lines and ordered (`wrap_text`).
        let text = Type::Body.wrap_text(ui, n.text, text_w);
        job.append(&format!("{sep}{text}"), 0.0, line(Type::Body, p.t1));
    }
    job.wrap.max_width = text_w;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    // The actions wrap; their height is known from the last pass.
    let acts_key = Id::unique(("details.note.acts", n.title));
    let acts_h = if n.actions.is_empty() {
        0.0
    } else {
        let rows = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(acts_key))
            .unwrap_or(control::BUTTON);
        gm::NOTE_ACTS[0] + rows - gm::NOTE_ACTS[2]
    };
    let h = pad * 2.0 + galley.size().y + acts_h;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(radius::POP), p.coral_tint);
    if p.outlined() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(radius::POP),
            Stroke::new(stroke::BORDER, p.line),
            egui::StrokeKind::Inside,
        );
    }
    let text_top = rect.top() + pad;
    painter.text(
        pos2(
            rect.left() + pad_l + icon_size::INLINE / 2.0,
            text_top + gm::NOTE_GLYPH_TOP + icon_size::INLINE / 2.0,
        ),
        Align2::CENTER_CENTER,
        n.icon.text(false),
        icons::font(icon_size::INLINE),
        p.coral,
    );
    let text_h = galley.size().y;
    let text_at = pos2(rect.left() + text_x, text_top);
    // Where the code stands in the text, to press it.
    let code_rect = n.code.map(|code| {
        use egui::text::CCursor;
        let from = n.title.chars().count();
        let a = galley.pos_from_cursor(CCursor::new(from));
        let b = galley.pos_from_cursor(CCursor::new(from + code.chars().count()));
        Rect::from_min_max(
            pos2(a.left() + gm::NOTE_CODE[1], a.top()),
            pos2(b.right(), a.bottom()),
        )
        .translate(text_at.to_vec2())
    });
    painter.galley(text_at, galley, p.t1);
    let spoken = match n.code {
        Some(c) => format!("{} {c}. {}", n.title, n.text),
        None => format!("{} {}", n.title, n.text),
    };
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Alert, &spoken);
    let mut out = NoteOut::default();
    if let Some(r) = code_rect {
        let code = ui.interact(
            r,
            Id::unique(("details.note.code", n.title)),
            Sense::click(),
        );
        crate::a11y::button(&code, strings::launch::COPY_DETAILS());
        if code.hovered() || code.has_focus() {
            ui.painter().hline(
                r.x_range(),
                r.bottom() - stroke::BORDER,
                Stroke::new(stroke::BORDER, p.t3),
            );
        }
        if code.has_focus() {
            focus_ring(ui.painter(), r, CornerRadius::same(radius::XS), &theme);
        }
        let code = crate::widgets::tooltip::tooltip(code, strings::launch::COPY_DETAILS(), None);
        out.copy = code.clicked();
    }
    if n.actions.is_empty() {
        return out;
    }
    // The actions run left to right and wrap (each button is placed where it fits: a
    // button lays itself out in a scope of its own, which a wrapping line cannot wrap).
    let left = rect.left() + text_x - gm::NOTE_ACTS[1];
    let right = rect.left() + text_x + text_w;
    let mut at = pos2(left, text_top + text_h + gm::NOTE_ACTS[0]);
    for (i, a) in n.actions.iter().enumerate() {
        let b = Button::ghost(a);
        let bw = b.width(ui);
        if at.x > left && at.x + bw > right {
            at = pos2(left, at.y + control::BUTTON + gm::NOTE_ACTS_GAP);
        }
        let mut cell = ui.new_child(
            UiBuilder::new()
                .id_salt(("note.act", n.title, i))
                .max_rect(Rect::from_min_size(at, vec2(bw, control::BUTTON)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if cell.add(b).clicked() {
            out.action = Some(i);
        }
        at.x += bw + gm::NOTE_ACTS_GAP;
    }
    let used = at.y + control::BUTTON - (text_top + text_h + gm::NOTE_ACTS[0]);
    let known = ui.ctx().data(|d| d.get_temp::<f32>(acts_key));
    if known.is_none_or(|k| (k - used).abs() > 0.5) {
        ui.ctx().data_mut(|d| d.insert_temp(acts_key, used));
        ui.ctx().request_discard("note-actions-measured");
    }
    out
}

/// Fine print: 12 / 17 in quiet text, wrapped to the width at hand.
pub(crate) fn fine(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    Type::Caption.paragraph_lh(ui, text, color, false, Some(gm::FINE_LINE))
}

/// A column of controls (Compatibility, Launch options): at most 620 wide, at the left.
pub(super) fn column<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width().min(layout::COL_MAX);
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        add(ui)
    })
    .inner
}

/// Skeleton lines while a query is slow.
pub fn skeleton_lines(ui: &mut Ui, n: usize) {
    for _ in 0..n {
        let (r, _) = ui.allocate_exact_size(
            vec2(
                ui.available_width().min(layout::TEXT_MAX_W),
                Type::Body.line_height(),
            ),
            Sense::hover(),
        );
        crate::widgets::skeleton::block(ui, r, Some(CornerRadius::same(radius::XS)));
        ui.add_space(space::S1);
    }
}

// ---- The cover column ----------------------------------------------------------------------

/// The cover: click opens the artwork overlay; a secondary click offers the quick choices
/// (a file, the program's icon, remove); a dropped image becomes the cover.
fn cover(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary, rect: Rect) {
    let theme = theme::theme(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("details.cover", g.id)), Sense::click());
    state.flight.note_page(g.id, rect);
    if !state.flight.hides(g.id) {
        let src = match g.cover {
            Some(a) => match env.textures.get_or_other_size(
                TexKey {
                    art: a,
                    size: ThumbSize::cover_for(rect.width() * ui.ctx().pixels_per_point()),
                },
                true,
                env.art,
            ) {
                TexState::Ready(t) => CoverSource::Texture(t, 1.0),
                TexState::Pending(c) => c.map_or(CoverSource::Placeholder, CoverSource::Pending),
            },
            None => CoverSource::Placeholder,
        };
        // The page draws the cover as it is, even for a game whose program is missing (the
        // design greys covers out on the wall only; the note above the tabs says it here).
        cover::paint_shadow(ui.painter(), rect, theme.palette.shade_cover, 1.0);
        cover::paint(
            ui.painter(),
            rect,
            src,
            &g.name,
            CoverLook::default(),
            &theme,
        );
    }
    if resp.has_focus() {
        focus_ring(
            ui.painter(),
            rect.expand(stroke::COVER_RING_GAP - stroke::FOCUS_GAP),
            cover::corner(rect),
            &theme,
        );
    }
    crate::a11y::button(&resp, s::CHANGE_ARTWORK());
    state.dnd.target(rect, DropTarget::Cover(g.id));
    let resp = crate::widgets::tooltip::tooltip(resp, s::CHANGE_ARTWORK(), None);
    if resp.clicked() {
        actions::apply(state, env, g.id, GameAction::Artwork);
    }
    if resp.secondary_clicked() {
        state.details.cover_menu = true;
    }
    if state.details.cover_menu {
        cover_menu(state, env, g, &resp);
    }
}

/// The quick artwork choices of a secondary click on the cover.
fn cover_menu(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary, at: &egui::Response) {
    let mut open = true;
    let mut chosen = None;
    Popup::menu(at)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if item(ui, None, strings::artwork::CHOOSE(), None, false).clicked() {
                chosen = Some(0);
            }
            if item(ui, None, strings::artwork::USE_ICON(), None, false).clicked() {
                chosen = Some(1);
            }
            if g.cover.is_some()
                && item(ui, None, strings::artwork::REMOVE(), None, false).clicked()
            {
                chosen = Some(2);
            }
            if item(ui, None, strings::menu::ARTWORK(), None, false).clicked() {
                chosen = Some(3);
            }
        });
    if !open || chosen.is_some() {
        state.details.cover_menu = false;
    }
    match chosen {
        Some(0) => state.picks.push(PickPurpose::Artwork(g.id, ArtSlot::Cover)),
        Some(1) => env.bridge.send(UiCommand::ImportArtwork {
            id: g.id,
            slot: ArtSlot::Cover,
            source: crate::bridge::ArtworkSource::ProgramIcon,
        }),
        Some(2) => env.bridge.send(UiCommand::ClearArtwork {
            id: g.id,
            slot: ArtSlot::Cover,
        }),
        Some(3) => actions::apply(state, env, g.id, GameAction::Artwork),
        _ => {}
    }
}

/// What the play record says.
enum Said {
    /// Starting: "Starting…" over "Getting its protected space ready".
    Starting,
    /// Running: the session clock over "Playing now".
    Playing(String),
    /// Never played.
    Never,
    /// Played: the time, when last, and the last thirty days.
    Played { time: String, last: Option<String> },
}

fn said(state: &UiState, env: &Env<'_>, g: &GameSummary) -> Said {
    let launching = matches!(g.activity, Activity::Launching(_))
        || state.launches.active(g.id).is_some_and(|l| {
            !matches!(l.state, easyplay_core::launch::LaunchState::Running { .. })
        });
    match g.activity {
        Activity::Running { since } => {
            let ms = env.now.since(since).map_or(0, |d| d.as_millis() as u64);
            Said::Playing(strings::fmt::session_clock(ms))
        }
        _ if launching => Said::Starting,
        _ if g.total_playtime_ms == 0 && g.last_played_at.is_none() => Said::Never,
        _ => Said::Played {
            time: strings::fmt::play_time(g.total_playtime_ms),
            last: g
                .last_played_at
                .map(|t| strings::fmt::last_played(Some(t), env.now, env.utc_offset_s)),
        },
    }
}

/// The play record under the cover (CSS `.rec`): the time played, when last, thirty days as
/// thirty bars and their total. The whole record opens Activity.
fn record(ui: &mut Ui, state: &mut UiState, env: &Env<'_>, g: &GameSummary, at: Rect) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let painter = ui.painter().clone();
    let w = at.width();
    // The bars grow when the page arrives and when the days first come in, whatever the
    // record shows now: a game that stops shows them grown.
    let days = state
        .details
        .week
        .ready()
        .map(|o| (record_days(o, layout::RECORD_BARS), o.totals.ms));
    let grow_id = Id::unique(("details.record", g.id, days.is_some()));
    let growth: Vec<f32> = (0..layout::RECORD_BARS)
        .map(|i| motion::bar_growth(ui.ctx(), grow_id, theme.motion, i))
        .collect();
    tick_elapsed(ui.ctx(), g.activity, env.now);
    let mut y = at.top();
    let two = |ui: &Ui, y: f32, big: &str, small: &str, c: Color32| {
        record_lines(ui, &painter, at, y, (big, c), small)
    };
    match said(state, env, g) {
        Said::Starting => {
            let bottom = two(ui, y, s::STARTING(), s::GETTING_READY(), p.accent_text);
            let r = Rect::from_min_max(at.min, pos2(at.right(), bottom));
            let resp = ui.interact(r, Id::unique(("details.record.live", g.id)), Sense::hover());
            crate::a11y::labelled(
                ui.ctx(),
                resp.id,
                egui::Role::Status,
                &format!("{} {}", s::STARTING(), s::GETTING_READY()),
            );
        }
        Said::Playing(clock) => {
            let bottom = two(ui, y, &clock, s::PLAYING_NOW(), p.accent_text);
            let r = Rect::from_min_max(at.min, pos2(at.right(), bottom));
            let resp = ui.interact(r, Id::unique(("details.record.live", g.id)), Sense::hover());
            crate::a11y::item(&resp, egui::Role::Label, &s::playing_sentence(&clock));
        }
        Said::Never => {
            let g1 = Type::Body.galley(ui, s::NOT_PLAYED_YET(), p.t2, w, usize::MAX);
            let r = Rect::from_min_size(pos2(at.left(), y), g1.size());
            painter.galley(r.min, g1, p.t2);
            let resp = ui.interact(
                r,
                Id::unique(("details.record.never", g.id)),
                Sense::hover(),
            );
            crate::a11y::item(&resp, egui::Role::Label, s::NOT_PLAYED_YET());
        }
        Said::Played { time, last } => {
            let line = s::record_line(last.as_deref().unwrap_or_default());
            let line_h = Type::Body.galley(ui, &line, p.t2, w, usize::MAX).size().y;
            let h = Type::Record.line_height()
                + gm::RECORD_LINE_TOP
                + line_h
                + gm::RECORD_BARS_TOP
                + layout::RECORD_BARS_H
                + gm::RECORD_FOOT_TOP
                + Type::Micro.line_height();
            let r = Rect::from_min_size(at.min, vec2(w, h));
            let resp = ui.interact(r, Id::unique(("details.record", g.id)), Sense::click());
            let hot = hover_t(&resp, &theme);
            y = two(ui, y, &time, &line, p.t1);
            // Thirty bars, two apart; a day without play is a stub.
            let top = y + gm::RECORD_BARS_TOP;
            let n = layout::RECORD_BARS as f32;
            let bw = (w - layout::RECORD_BAR_GAP * (n - 1.0)) / n;
            let values = days.as_ref().map(|d| d.0.clone()).unwrap_or_default();
            let max = values.iter().copied().max().unwrap_or(0).max(1) as f32;
            for (i, grown) in growth.iter().enumerate() {
                let v = values.get(i).copied().unwrap_or(0);
                let (h, c) = if v > 0 {
                    let full = (v as f32 / max * layout::RECORD_BARS_H).max(layout::RECORD_STUB);
                    (full * grown, mix(p.t2, p.t1, hot))
                } else {
                    (layout::RECORD_STUB, p.o3)
                };
                if h <= 0.0 {
                    continue;
                }
                let x = at.left() + i as f32 * (bw + layout::RECORD_BAR_GAP);
                let bottom = top + layout::RECORD_BARS_H;
                painter.rect_filled(
                    Rect::from_min_max(pos2(x, bottom - h), pos2(x + bw, bottom)),
                    CornerRadius::same(radius::BAR),
                    c,
                );
            }
            let foot = top + layout::RECORD_BARS_H + gm::RECORD_FOOT_TOP;
            let total = days.as_ref().map_or(0, |d| d.1);
            let total = strings::fmt::play_time(total);
            let l = Type::Micro.line(ui, s::LAST_30_DAYS(), p.t3);
            painter.galley(pos2(at.left(), foot), l, p.t3);
            let t = Type::Micro.numeric_galley(ui, &total, p.t3);
            painter.galley(pos2(at.right() - t.size().x, foot), t, p.t3);
            if resp.has_focus() {
                focus_ring(&painter, r, CornerRadius::same(gm::RECORD_RADIUS), &theme);
            }
            crate::a11y::button(&resp, &s::record_name(&time, &line));
            crate::a11y::describe(&resp, &s::days_name(&total));
            let resp = crate::widgets::tooltip::tooltip(resp, s::OPEN_ACTIVITY(), None);
            if resp.clicked() {
                state.navigate(crate::vm::dialogs::AfterDiscard::Tab(
                    DetailsTab::Activity.index(),
                ));
            }
        }
    }
}

/// The record's number (display 34, tabular) and the line under it; returns the bottom.
fn record_lines(
    ui: &Ui,
    painter: &egui::Painter,
    at: Rect,
    y: f32,
    (big, c): (&str, Color32),
    small: &str,
) -> f32 {
    let p = theme::theme(ui.ctx()).palette.clone();
    let g1 = Type::Record.numeric_galley(ui, big, c);
    painter.galley(pos2(at.left(), y), g1, c);
    let y2 = y + Type::Record.line_height() + gm::RECORD_LINE_TOP;
    let g2 = Type::Body.galley(ui, small, p.t2, at.width(), usize::MAX);
    let h2 = g2.size().y;
    painter.galley(pos2(at.left(), y2), g2, p.t2);
    y2 + h2
}

/// The record as one sentence, in a page too narrow for the cover column.
fn record_sentence(ui: &mut Ui, state: &UiState, env: &Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    tick_elapsed(ui.ctx(), g.activity, env.now);
    let (text, c) = match said(state, env, g) {
        Said::Playing(clock) => (s::playing_sentence(&clock), p.accent_text),
        Said::Starting => (
            format!("{} {}", s::STARTING(), s::GETTING_READY()),
            p.accent_text,
        ),
        Said::Never => (s::NOT_PLAYED_SENTENCE().to_owned(), p.t2),
        Said::Played { time, last } => (s::played_sentence(&time, last.as_deref()), p.t2),
    };
    ui.add_space(gm::SENTENCE_TOP);
    Type::Lead.paragraph(ui, &text, c);
}

// ---- The header ------------------------------------------------------------------------------

/// "‹ Library": 26 high, its plate hanging 6 left of the axis.
fn back_link(ui: &mut Ui, state: &mut UiState) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let word = Type::BodyStrong.line(ui, s::BACK_LINK(), Color32::PLACEHOLDER);
    let w = gm::BACK_PAD[0] + icon_size::INLINE + gm::BACK_GAP + word.size().x + gm::BACK_PAD[1];
    let top = ui.cursor().min;
    let (slot, _) = ui.allocate_exact_size(vec2(w - gm::BACK_HANG, gm::BACK_H), Sense::hover());
    let rect = Rect::from_min_size(pos2(top.x - gm::BACK_HANG, slot.top()), vec2(w, gm::BACK_H));
    let resp = ui.interact(rect, Id::unique("details.back"), Sense::click());
    let hot = hover_t(&resp, &theme);
    let corner = CornerRadius::same(radius::WORD);
    let painter = ui.painter();
    painter.rect_filled(rect, corner, fade(p.o1, hot));
    let c = mix(p.t3, p.t1, hot);
    let mut x = rect.left() + gm::BACK_PAD[0];
    painter.text(
        pos2(x + icon_size::INLINE / 2.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::CARET_LEFT.text(false),
        icons::font(icon_size::INLINE),
        c,
    );
    x += icon_size::INLINE + gm::BACK_GAP;
    painter.galley(pos2(x, rect.center().y - word.size().y / 2.0), word, c);
    if resp.has_focus() {
        focus_ring(painter, rect, corner, &theme);
    }
    crate::a11y::button(&resp, s::BACK_LINK());
    let resp =
        crate::widgets::tooltip::tooltip(resp, s::BACK_TIP(), Some(strings::shortcuts::ESC()));
    if resp.clicked() {
        state.navigate(crate::vm::dialogs::AfterDiscard::Back);
    }
}

/// The title: the display face, wrapping, selectable. F2 (or the game menu) renames it in
/// place.
fn title(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary, style: Type) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if let Some(text) = &mut state.details.rename {
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .font(style.font_id())
                .desired_width(ui.available_width())
                .char_limit(80),
        );
        crate::a11y::labelled(ui.ctx(), r.id, egui::Role::TextInput, s::RENAME_FIELD());
        crate::a11y::describe(&r, s::RENAME_HINT());
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if esc {
            state.details.rename = None;
        } else if enter || r.lost_focus() {
            let name = text.trim().to_owned();
            state.details.rename = None;
            if !name.is_empty()
                && name != *g.name
                && easyplay_core::model::validate_name(&name).is_ok()
            {
                env.bridge.send(UiCommand::UpdateGame {
                    id: g.id,
                    edits: vec![GameEdit::Name(name.clone())],
                });
                state.toast(
                    strings::feedback::renamed(&name),
                    ToastKind::Success,
                    Some(UiCommand::UpdateGame {
                        id: g.id,
                        edits: vec![GameEdit::Name(g.name.to_string())],
                    }),
                    env.mono,
                );
            }
        }
        return;
    }
    // Balanced lines (CSS `text-wrap: balance`): the narrowest width that keeps the number
    // of lines the full width needs.
    let full = ui.available_width();
    let w = balanced_width(ui, &g.name, style, full);
    let r = ui
        .allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_max_width(w);
            ui.add(
                egui::Label::new(style.rich(&*g.name).color(p.t1))
                    .wrap()
                    .selectable(true),
            )
        })
        .inner;
    crate::a11y::labelled(ui.ctx(), r.id, egui::Role::Heading, &g.name);
}

/// The narrowest width at which `text` in `style` takes as many lines as it does at `full`.
fn balanced_width(ui: &Ui, text: &str, style: Type, full: f32) -> f32 {
    let rows = |w: f32| {
        style
            .galley(ui, text, Color32::PLACEHOLDER, w, usize::MAX)
            .rows
            .len()
    };
    let n = rows(full);
    if n < 2 {
        return full;
    }
    let (mut lo, mut hi) = (full / n as f32, full);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if rows(mid) > n {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi.ceil().min(full)
}

/// Under the title: "Offline" or "Online" (to the Protection tab), the tags and "+ Tag" (to
/// the tag menu). The row starts 8 left of the axis, so the words' text keeps it.
fn meta_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    // Derived from the internet permission, never stored; the draft decides while the
    // Protection tab edits it, so the word follows the switch.
    let level = s::net_word(crate::vm::details::is_online(&state.details, g));
    let top = ui.cursor().min;
    let w = ui.available_width() + layout::WORD_PAD;
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt("details.meta")
            .max_rect(Rect::from_min_size(
                pos2(top.x - layout::WORD_PAD, top.y),
                vec2(w, control::SM),
            ))
            .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
    );
    row.spacing_mut().item_spacing = vec2(layout::WORD_SPACING, layout::WORD_SPACING);
    let mut to_tab = false;
    if row
        .add(
            Word::new(level)
                .icon(icons::SHIELD_STANDARD)
                .strong()
                .tip(s::PROTECTION_TIP(), None),
        )
        .clicked()
    {
        to_tab = true;
    }
    tag_words(&mut row, state, env, g);
    let used = row.min_rect();
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + ui.available_width(), used.bottom())),
        Sense::hover(),
    );
    if to_tab {
        state.navigate(crate::vm::dialogs::AfterDiscard::Tab(
            DetailsTab::Protection.index(),
        ));
    }
}

/// The meta row's tags and "+ Tag", each opening the tag menu, where tags are ticked and
/// unticked.
fn tag_words(row: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(row.ctx()).palette.clone();
    let lib = env.bridge.library().clone();
    let id = g.id;
    let tag_word =
        |row: &mut Ui, state: &mut UiState, env: &mut Env<'_>, word: Word<'_>, k: usize| {
            let key = Id::unique(("details.tags.open", id, k));
            let was = row.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
            let resp = row.add(word.open(was));
            let open = Popup::is_id_open(row.ctx(), Popup::default_response_id(&resp));
            row.ctx().data_mut(|d| d.insert_temp(key, open));
            let popup = Popup::menu(&resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .align(egui::RectAlign::BOTTOM_START);
            crate::widgets::context_menu::popup(popup, |ui| {
                crate::screens::dialogs::tags::tag_menu(ui, state, env, &[id]);
            });
        };
    for (k, t) in g.tags.iter().enumerate() {
        if let Some(t) = lib.tag(*t) {
            let word = Word::new(&t.name)
                .dot(p.tag_dot(&t.name))
                .strong()
                .tip(s::TAGS_TIP(), Some("T"));
            tag_word(row, state, env, word, k);
        }
    }
    let plus = Word::new(s::TAG_WORD())
        .icon(icons::PLUS)
        .tip(s::TAGS_TIP(), Some("T"));
    tag_word(row, state, env, plus, g.tags.len());
}

/// Whether the game has a problem open (a failed start, or one that ended at once).
fn has_problem(state: &UiState, id: GameId) -> bool {
    state.launches.failures.contains_key(&id) || state.launches.ended_early.contains_key(&id)
}

/// What Play does, by state: play, stop, locate; a game still running in the background is
/// stopped by force; a game with an open problem shows the problem instead of starting.
fn press_play(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    if g.activity == Activity::Lingering {
        env.bridge.send(UiCommand::Stop {
            id: g.id,
            force: true,
        });
        return;
    }
    let st = play_state(g);
    if matches!(
        st,
        PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer
    ) && has_problem(state, g.id)
    {
        let title = state.launches.failures.get(&g.id).map_or_else(
            || strings::launch::CLOSED_EARLY().to_owned(),
            |e| e.title.clone(),
        );
        crate::a11y::Announcer::announce(&env.ctx, title, crate::a11y::Politeness::Assertive);
        // The note says what is wrong: bring it into view (Play may be the compact one).
        env.ctx
            .data_mut(|d| d.insert_temp(Id::unique(("details.show_problem", g.id)), true));
        return;
    }
    actions::play(state, env, g);
}

/// Play, favourite, more.
fn actions_row(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gm::ACTIONS_GAP;
        let st = play_state(g);
        let pill = play_pill(ui, &st, &g.name, PillSize::Page, 1.0);
        if !state.details.play_focused {
            // The page's first focus: Play.
            state.details.play_focused = true;
            pill.request_focus();
        }
        if pill.clicked() {
            press_play(state, env, g);
        }
        let tip = if g.favorite {
            s::UNFAVOURITE
        } else {
            s::FAVOURITE
        };
        let fav = ui.add(
            IconButton::new(icons::STAR, tip())
                .round(true)
                .toggle(g.favorite)
                .selected(g.favorite)
                .shortcut("F"),
        );
        if fav.clicked() {
            actions::apply(state, env, g.id, GameAction::Favorite);
        }
        let more = ui.add(IconButton::new(icons::DOTS, s::MORE()).round(true));
        if more.clicked() {
            state.library.menu = Some((g.id, more.rect.left_bottom()));
        }
    });
}

/// The notes above the tabs, each 20 below what precedes it: launch progress worth a
/// word, a problem, a missing program, a game stopping or still running in the
/// background, changed shared files.
fn notes(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let id = g.id;
    if let Some(launch) = state.launches.active(id).cloned()
        && !matches!(
            launch.state,
            easyplay_core::launch::LaunchState::Running { .. }
        )
    {
        let elapsed = env
            .mono
            .saturating_duration_since(launch.since)
            .as_secs_f32();
        let pr = crate::vm::launch::present(&launch.state, elapsed);
        if pr.cancellable || pr.reassure || pr.offline_note {
            let mut text: Vec<String> = Vec::new();
            if let Some(b) = pr.bytes {
                text.push(strings::launch::bytes_rate(b, launch.rate));
            }
            if pr.offline_note {
                text.push(strings::launch::OFFLINE_NOTE().to_owned());
            }
            if pr.reassure {
                text.push(strings::launch::REASSURE().to_owned());
            }
            let text = text.join(" ");
            let acts: &[&str] = if pr.cancellable {
                &[strings::launch::CANCEL()]
            } else {
                &[]
            };
            ui.add_space(gm::NOTE_TOP);
            if note(
                ui,
                &Note {
                    tone: NoteTone::Info,
                    icon: icons::CLOCK,
                    title: &pr.phase,
                    code: None,
                    text: &text,
                    break_after_title: true,
                    actions: acts,
                },
            )
            .action
                == Some(0)
            {
                env.bridge.send(UiCommand::CancelLaunch { id });
            }
        }
    }
    if has_problem(state, id) {
        ui.add_space(gm::NOTE_TOP);
        let top = ui.cursor().min;
        launch_panel::problem_note(ui, state, env, id);
        let show = Id::unique(("details.show_problem", id));
        if ui
            .ctx()
            .data_mut(|d| d.remove_temp::<bool>(show))
            .unwrap_or(false)
        {
            let rect = Rect::from_min_max(top, ui.min_rect().right_bottom());
            ui.scroll_to_rect(rect, Some(Align::Min));
        }
    }
    match g.availability {
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable => {
            let title = s::availability_title(g.availability);
            let (_, body) = s::availability(g.availability);
            // A drive can come back, and a folder's permissions can change: those offer to
            // look again. A missing program is located with Play.
            let acts: &[&str] = if g.availability == Availability::Missing {
                &[]
            } else {
                &[s::CHECK_AGAIN()]
            };
            ui.add_space(gm::NOTE_TOP);
            if note(
                ui,
                &Note {
                    tone: NoteTone::Warning,
                    icon: icons::WARNING,
                    title: &title,
                    code: None,
                    text: body,
                    break_after_title: false,
                    actions: acts,
                },
            )
            .action
                == Some(0)
            {
                env.bridge.send(UiCommand::RescanMissing);
            }
        }
        _ if g.flags.suspected_tamper => {
            ui.add_space(gm::NOTE_TOP);
            note(
                ui,
                &Note {
                    tone: NoteTone::Warning,
                    icon: icons::WARNING,
                    title: s::TAMPER(),
                    code: None,
                    text: s::TAMPER_BODY(),
                    break_after_title: true,
                    actions: &[],
                },
            );
        }
        _ => {}
    }
    let (title, body, icon) = match g.activity {
        Activity::Lingering => (s::LINGERING_TITLE(), s::LINGERING_BODY(), icons::CLOCK),
        Activity::Stopping => (s::STOPPING_TITLE(), s::STOPPING_BODY(), icons::STOP),
        _ => return,
    };
    let acts: &[&str] = if g.activity == Activity::Stopping {
        &[strings::launch::FORCE_STOP()]
    } else {
        &[]
    };
    ui.add_space(gm::NOTE_TOP);
    if note(
        ui,
        &Note {
            tone: NoteTone::Info,
            icon,
            title,
            code: None,
            text: body,
            break_after_title: false,
            actions: acts,
        },
    )
    .action
        == Some(0)
    {
        env.bridge.send(UiCommand::Stop { id, force: true });
    }
}

// ---- The right column ------------------------------------------------------------------------

/// Run `add` risen `offset` and at `opacity` (a part of the page arriving).
fn risen<R>(ui: &mut Ui, (opacity, offset): (f32, f32), add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.multiply_opacity(opacity);
        ui.with_visual_transform(TSTransform::from_translation(vec2(0.0, offset)), add)
            .inner
    })
    .inner
}

/// The selected tab's body.
fn body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    match state.details.tab {
        DetailsTab::Overview => overview::show(ui, state, env, g),
        DetailsTab::Protection => permissions::show(ui, state, env, g),
        DetailsTab::Compatibility => runtime::show(ui, state, env, g),
        DetailsTab::LaunchOptions => launch_options::show(ui, state, env, g),
        DetailsTab::Activity => activity::show(ui, state, env, g),
        DetailsTab::Logs => crate::screens::debug::logs::tab(ui, state, env, g.id),
    }
}

/// The scrolling column: back link, title block, actions, notes, tabs and the tab's body.
fn column_body(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    g: &GameSummary,
    f: &Frame,
    content_w: f32,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let rise = |k: usize| {
        motion::arrive(
            ui.ctx(),
            Id::unique(("details.rise", g.id, k)),
            theme.motion,
            travel::PAGE_RISE_AT[k],
            Dur::Page,
            travel::PAGE_RISE,
        )
    };
    let (r0, r1, r2) = (rise(0), rise(1), rise(2));
    let w = ui.available_width().min(layout::GAME_COL_MAX);
    ui.set_width(w);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    back_link(ui, state);
    risen(ui, r0, |ui| {
        ui.add_space(gm::TITLE_TOP);
        title(ui, state, env, g, title_type(f));
        ui.add_space(gm::META_TOP);
        meta_row(ui, state, env, g);
        if f.narrow {
            record_sentence(ui, state, env, g);
        }
    });
    ui.add_space(gm::ACTIONS_TOP);
    let actions_bottom = risen(ui, r1, |ui| {
        actions_row(ui, state, env, g);
        ui.min_rect().bottom()
    });
    // The compact Play joins the tab bar once the actions have scrolled away.
    let stuck = actions_bottom < ui.clip_rect().top() + gm::PIN_SLACK;
    let compact = ui.ctx().animate_bool_with_time_and_easing(
        Id::unique(("details.compact_play", g.id)),
        stuck,
        theme.motion.fade(Dur::Fast),
        egui::emath::easing::linear,
    );
    let gap = if content_w < layout::TABS_TIGHT_BELOW {
        layout::TAB_GAP[1]
    } else {
        layout::TAB_GAP[0]
    };
    let mut picked = None;
    let (slot, pinned) = risen(ui, r2, |ui| {
        notes(ui, state, env, g);
        ui.add_space(gm::TABS_TOP);
        let (slot, _) = ui.allocate_exact_size(vec2(w, layout::TAB_H), Sense::hover());
        // In its place the bar comes before the body (and so before it in focus order);
        // pinned to the column's top it is laid out after the body, to lie over it.
        let pinned = slot.top() < ui.clip_rect().top();
        if !pinned {
            let mut bar_ui = ui.new_child(
                UiBuilder::new()
                    .id_salt("details.tabs")
                    .max_rect(slot)
                    .layout(Layout::top_down(Align::Min)),
            );
            picked = Some(tab_bar(&mut bar_ui, state, g, gap, compact));
        }
        ui.add_space(gm::PANEL_TOP);
        // A new tab's body (and a new range or table of Activity) arrives.
        let key = (
            state.details.tab,
            state.details.activity.range,
            state.details.activity.table,
        );
        let body_in = motion::since_change(
            ui.ctx(),
            Id::unique(("details.body", g.id)),
            key,
            theme
                .motion
                .fade(Dur::Base)
                .max(theme.motion.movement(Dur::Base)),
            false,
        )
        .map_or((1.0, 0.0), |t| {
            let fade = theme.motion.fade(Dur::Base);
            let mv = theme.motion.movement(Dur::Base);
            let a = if fade > 0.0 { (t / fade).min(1.0) } else { 1.0 };
            let o = if mv > 0.0 {
                travel::RISE * (1.0 - motion::ease_out(t / mv))
            } else {
                0.0
            };
            (a, o)
        });
        risen(ui, body_in, |ui| body(ui, state, env, g));
        (slot, pinned)
    });
    if pinned {
        let bar = Rect::from_min_size(
            pos2(slot.left(), ui.clip_rect().top()),
            vec2(w, layout::TAB_H),
        );
        let mut bar_ui = ui.new_child(
            UiBuilder::new()
                .id_salt("details.tabs")
                .max_rect(bar)
                .layout(Layout::top_down(Align::Min)),
        );
        bar_ui.painter().rect_filled(bar, CornerRadius::ZERO, p.bg);
        // Clicks on the bar's empty part must not reach the body under it.
        let _ = bar_ui.interact(bar, Id::unique("details.tabs.plate"), Sense::click());
        picked = Some(risen(&mut bar_ui, (r2.0, 0.0), |ui| {
            tab_bar(ui, state, g, gap, compact)
        }));
    }
    if let Some((sel, pressed)) = picked {
        if pressed {
            press_play(state, env, g);
        }
        if let Some(sel) = sel {
            state.navigate(crate::vm::dialogs::AfterDiscard::Tab(sel));
        }
    }
}

/// The tab bar, with the compact Play at its right end while `compact` (its opacity) is
/// above 0. Returns the tab chosen, and whether Play was pressed.
fn tab_bar(
    ui: &mut Ui,
    state: &UiState,
    g: &GameSummary,
    gap: f32,
    compact: f32,
) -> (Option<usize>, bool) {
    let names: Vec<&str> = DetailsTab::visible().iter().map(|t| s::tab(*t)).collect();
    let mut sel = state.details.tab.index();
    let st = play_state(g);
    let (list, pressed) = Tabs::new(s::TABS(), &names)
        .gap(gap)
        .show_with(ui, &mut sel, |ui| {
            compact > 0.0 && play_pill(ui, &st, &g.name, PillSize::Compact, compact).clicked()
        });
    (list.changed().then_some(sel), pressed)
}

/// Show the game page.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(id).cloned() else {
        return;
    };
    // A game Steam runs has a page of its own (ADR-0031).
    if g.is_steam() {
        return steam::show(ui, state, env, &g);
    }
    if state.details.game != Some(id) {
        enter(state, env, id);
    } else if state.details.detail_q.is_none()
        && state.details.loaded_rev != 0
        && state.details.loaded_rev != lib.revision
    {
        load(state, env, id);
    }
    keep_days(state, env, &g);
    // Page shortcuts not in the global table.
    if state.nav.overlays().is_empty()
        && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S))
    {
        permissions::save(state, env);
    }
    if state.details.dirty() {
        let theme = theme::theme(ui.ctx());
        let gutter = if ui.ctx().content_rect().width() < layout::NARROW_BELOW {
            layout::GUTTER_NARROW
        } else {
            layout::GUTTER
        };
        egui::Panel::bottom("details_save_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme.palette.bg)
                    .inner_margin(egui::Margin::symmetric(gutter as i8, space::S3 as i8)),
            )
            .show_separator_line(true)
            .show(ui, |ui| save_bar(ui, state, env));
    }
    let page = ui.available_rect_before_wrap();
    let f = frame(page);
    if let Some(c) = f.cover {
        cover(ui, state, env, &g, c);
        let at = Rect::from_min_max(
            pos2(c.left(), c.bottom() + gm::RECORD_TOP),
            pos2(c.right(), page.bottom()),
        );
        record(ui, state, env, &g, at);
    }
    let [pad_l, pad_r, pad_b] = f.pad;
    let content_w = (f.column.width() - pad_l - pad_r).max(0.0);
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::unique("details.content_w"), content_w));
    // An id of its own, not one counted from what precedes it: the save bar coming and going
    // must not change the ids of the controls (and so take their focus away).
    let mut col = ui.new_child(
        UiBuilder::new()
            .scope_id(Id::unique(("details.column", id)))
            .max_rect(f.column)
            .layout(Layout::top_down(Align::Min)),
    );
    ScrollArea::vertical()
        .id_salt(("details", id))
        .auto_shrink([false, false])
        .show(&mut col, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: pad_l as i8,
                    right: pad_r as i8,
                    top: 0,
                    bottom: pad_b as i8,
                })
                .show(ui, |ui| column_body(ui, state, env, &g, &f, content_w));
        });
    ui.advance_cursor_after_rect(page);
    // The game menu ("More").
    if let Some((mid, at)) = state.library.menu
        && mid == id
    {
        let mut open = true;
        let facts = super::library::menu::MenuFacts {
            has_shortcut: env.bridge.host().shortcuts.iter().any(|x| x.game == id),
            developer: env.bridge.prefs().advanced.developer_mode,
        };
        let chosen = super::library::menu::show(&env.ctx, &lib, id, at, &mut open, facts);
        if !open {
            state.library.menu = None;
        }
        if let Some(a) = chosen {
            actions::apply(state, env, id, a);
        }
    }
}

/// While a tab has unsaved changes: what they are, and Save / Discard.
fn save_bar(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (errors, widens) = match state.details.tab {
        DetailsTab::Protection => state
            .details
            .protection
            .as_ref()
            .map_or((0, false), |e| (e.errors(), !e.widenings().is_empty())),
        _ => (0, false),
    };
    let launch_error = state
        .details
        .launch
        .as_ref()
        .is_some_and(|l| l.resolved().is_err() || launch_options::env_errors(l) > 0);
    ui.horizontal(|ui| {
        let sentence = if widens {
            s::UNSAVED_WIDENS
        } else {
            s::UNSAVED
        };
        Type::Body.paragraph(ui, sentence(), p.t1);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let reason = if errors > 0 {
                Some(s::fix_problems(errors))
            } else if launch_error {
                Some(s::FIX_LAUNCH().to_owned())
            } else {
                None
            };
            let save = ui.add(Button::primary(s::SAVE()).disabled(reason.as_deref()));
            if save.clicked() {
                permissions::save(state, env);
            }
            if ui.add(Button::ghost(s::DISCARD())).clicked() {
                state.details.discard();
            }
            if let Some(r) = &reason {
                ui.label(Type::Caption.rich(r).color(p.coral));
            }
        });
    });
}
