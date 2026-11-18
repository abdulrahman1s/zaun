//! The game you watch (`chat-calls.html` notes F "WATCHING"): a 16:9 stage, centred, at
//! most 46 % of the window's height (72 % larger), or the whole window in full screen.
//! Chips on top ("● Live", whose game, its resolution, who watches); along the bottom while
//! pointed at: sound, size, full screen and Stop watching. "Waiting for the picture…" until
//! the first frame; "<name> stopped sharing" over the dimmed last frame when it ends.

use egui::{
    Align2, Color32, CornerRadius, Id, Key, Modifiers, Order, Painter, Rect, Sense, Stroke, Ui,
    ViewportCommand, pos2, vec2,
};
use zaun_core::model::{ChatRef, ChatSnapshot, Watching};
use zaun_core::service::ChatCommand;

use super::Env;
use super::paint;
use super::stage_picture as pic;
use crate::app::UiState;
use crate::bridge::Command;
use crate::strings::chat::calls as cs;
use crate::strings::chat::details as s;
use crate::theme::palette::fade;
use crate::theme::{self, Dur, Type, icons, motion};

/// The stage's measures (`.stage`, `.sb`).
mod m {
    /// The stage takes at most this share of the window's height, or the second larger.
    pub const STAGE_H: [f32; 2] = [0.46, 0.72];
    /// Under the stage.
    pub const BELOW: f32 = 10.0;
    /// The chips: 10 in (18 in full screen), 28 high, 9 padding, 6 apart.
    pub const CHIP: [f32; 4] = [10.0, 28.0, 9.0, 6.0];
    /// See [`CHIP`].
    pub const CHIP_IN_FULL: f32 = 18.0;
    /// The chips' plate: black at 60 %, 80 % under the pointer.
    pub const CHIP_ALPHA: [f32; 2] = [0.6, 0.8];
    /// The ended stage closes itself after this long.
    pub const ENDED_FOR: std::time::Duration = std::time::Duration::from_secs(8);
    /// In full screen the controls hide after the pointer stood still this long.
    pub const STILL_FOR: f64 = 2.0;
}

/// The stage of `w`, under the conversation's header.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    w: &Watching,
    inset: f32,
) {
    let th = theme::theme(ui.ctx());
    let rect = place(ui, state, inset);
    let alpha = motion::appear(
        ui.ctx(),
        Id::unique(("chat.stage", w.chat)),
        th.motion.fade(Dur::Base),
    );
    full_screen(ui, state, env);
    ui.data_mut(|d| d.insert_temp(rect_key(w.chat), rect));
    if state.chat.call.full {
        let screen = ui.ctx().content_rect();
        egui::Area::new(Id::unique("chat.stage.full"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ui.ctx(), |ui| {
                let (r, _) = ui.allocate_exact_size(screen.size(), Sense::hover());
                ui.painter()
                    .rect_filled(r, CornerRadius::ZERO, Color32::BLACK);
                let fit = (r.width()).min(r.height() * 16.0 / 9.0);
                let inner = Rect::from_center_size(r.center(), vec2(fit, fit * 9.0 / 16.0));
                body(ui, state, env, snap, w, inner, 1.0, true);
            });
    } else {
        body(ui, state, env, snap, w, rect, alpha, false);
    }
}

/// The stage's place in the column (allocated whatever is drawn in it).
fn place(ui: &mut Ui, state: &UiState, inset: f32) -> Rect {
    let share = m::STAGE_H[usize::from(state.chat.stage_large)];
    let col = ui.available_width();
    let max_w = ui.ctx().content_rect().height() * share * 16.0 / 9.0;
    let sw = (col - inset * 2.0).min(max_w).max(1.0);
    let sh = sw * 9.0 / 16.0;
    let (row, _) = ui.allocate_exact_size(vec2(col, sh + m::BELOW), Sense::hover());
    Rect::from_min_size(pos2(row.center().x - sw / 2.0, row.top()), vec2(sw, sh))
}

/// `F` toggles full screen while the stage of the open conversation is pointed at or one of
/// its controls has focus, or while it fills the window; never while a text field has focus.
/// Called before the page draws, so the composer does not take the letter.
pub fn keys(ctx: &egui::Context, state: &mut UiState, snap: &ChatSnapshot) {
    let open = state.chat.open.as_ref().map(|o| o.chat);
    let Some(w) = snap.live.watch.as_ref().filter(|w| Some(w.chat) == open) else {
        return;
    };
    let typing = ctx
        .memory(|mm| mm.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    let last = ctx.data(|d| d.get_temp::<Rect>(rect_key(w.chat)));
    let pointed = last.is_some_and(|r| ctx.pointer_hover_pos().is_some_and(|p| r.contains(p)));
    let here = state.chat.call.full || pointed || stage_focus(ctx);
    if typing || !here {
        return;
    }
    let pressed = ctx.input_mut(|i| {
        let hit = i.consume_key(Modifiers::NONE, Key::F);
        if hit {
            // The letter itself must not reach the message field.
            i.events
                .retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case("f")));
        }
        hit
    });
    if pressed {
        state.chat.call.full = !state.chat.call.full;
    }
}

fn rect_key(chat: ChatRef) -> Id {
    Id::unique(("chat.stage.rect", chat))
}

/// Ask the window to be full screen while the stage fills it, and give back the window's
/// own state afterwards (it stays full screen if it was before).
fn full_screen(ui: &Ui, state: &mut UiState, env: &Env<'_>) {
    let key = Id::unique("chat.stage.full.applied");
    let applied = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let vm = &mut state.chat.call;
    if vm.full && !applied {
        vm.was_full = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
        env.ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        ui.data_mut(|d| d.insert_temp(key, true));
    } else if !vm.full && applied {
        if !vm.was_full {
            env.ctx
                .send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
        ui.data_mut(|d| d.insert_temp(key, false));
    }
}

/// Leave full screen (the stream is gone).
pub fn leave_full(ui: &Ui, state: &mut UiState, env: &Env<'_>) {
    if state.chat.call.full {
        state.chat.call.full = false;
        full_screen(ui, state, env);
    }
}

fn stage_focus(ctx: &egui::Context) -> bool {
    let ids = [
        "chat.stage.sound",
        "chat.stage.size",
        "chat.stage.full",
        "chat.stage.stop",
    ];
    ctx.memory(|mem| {
        mem.focused()
            .is_some_and(|f| ids.iter().any(|k| Id::unique(*k) == f))
    })
}

/// What the stage shows in `rect`.
#[allow(clippy::too_many_arguments)]
fn body(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    w: &Watching,
    rect: Rect,
    alpha: f32,
    full: bool,
) {
    let resp = ui.interact(rect, Id::unique("chat.stage"), Sense::hover());
    let who = s::stage_who(paint::first_name(&w.name), &w.title);
    crate::a11y::item(&resp, egui::Role::Image, &who);
    let stream = &snap.live.stream;
    let dim = stream.watch_paused.then_some(pic::PAUSED);
    let size = pic::picture(ui, rect, &w.title, alpha, env.bridge.frames(), dim);
    if stream.watch_waiting {
        waiting(ui, rect, &w.title, alpha);
    } else if stream.watch_paused {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icons::PAUSE.text(true),
            icons::font(20.0),
            fade(Color32::WHITE, alpha),
        );
    }
    let inn = if full { m::CHIP_IN_FULL } else { m::CHIP[0] };
    let chips = Chips::new(rect, inn);
    top(ui, &chips, w, &who, size, alpha);
    let shown = bottom_shown(ui, rect, full);
    bottom(ui, state, env, &chips, alpha * shown, full);
}

/// Where chips go: from the start or the end, on top or at the bottom, mirrored.
struct Chips {
    rect: Rect,
    inn: f32,
    rtl: bool,
}

impl Chips {
    fn new(rect: Rect, inn: f32) -> Self {
        Self {
            rect,
            inn,
            rtl: crate::i18n::dir() == crate::i18n::Dir::Rtl,
        }
    }

    fn at(&self, from: f32, cw: f32, top: bool) -> Rect {
        let ch = m::CHIP[1];
        let left = if self.rtl {
            self.rect.right() - self.inn - from - cw
        } else {
            self.rect.left() + self.inn + from
        };
        let y = if top {
            self.rect.top() + self.inn
        } else {
            self.rect.bottom() - self.inn - ch
        };
        Rect::from_min_size(pos2(left, y), vec2(cw, ch))
    }

    fn at_end(&self, from_end: f32, cw: f32, top: bool) -> Rect {
        self.at(self.rect.width() - self.inn * 2.0 - from_end - cw, cw, top)
    }
}

/// Top: "● Live", "Jonas, Ninefold Arena", "1080p"; the eye and the count at the end.
fn top(ui: &Ui, chips: &Chips, w: &Watching, who: &str, size: Option<[usize; 2]>, alpha: f32) {
    let painter = ui.painter().clone();
    let [_, _, pad, gap] = m::CHIP;
    let live_w = 6.0 + 5.0 + live_galley(&painter, s::LIVE()).size().x;
    let r = chips.at(0.0, live_w + pad * 2.0, true);
    chip_plate(&painter, r, 0.0, alpha);
    paint::live_mark(
        &painter,
        r.left() + pad,
        r.center().y,
        s::LIVE(),
        fade(paint::LIVE_ON_DARK, alpha),
    );
    let mut x = r.width() + gap;
    let g = Type::BodyStrong.galley(
        ui,
        who,
        Color32::WHITE,
        (chips.rect.width() * 0.5).max(40.0),
        1,
    );
    let r = chips.at(x, g.size().x + pad * 2.0, true);
    text_chip(&painter, r, g, alpha);
    x += r.width() + gap;
    // What is received: the decoded height and the frames of the last second (notes J); the
    // delay is not measured, so the tooltip leaves it out.
    if let Some([_, h]) = size {
        let res = cs::resolution(h as u32);
        let fps = pic::received_fps(ui).map(super::share_quality::rounded_fps);
        let (text, tip) = match fps {
            Some(f) => (cs::chip(&res, f), cs::received(&res, f)),
            None => (res.clone(), res),
        };
        let g = Type::BodyStrong.numeric_galley(ui, &text, Color32::WHITE);
        let r = chips.at(x, g.size().x + pad * 2.0, true);
        text_chip(&painter, r, g, alpha);
        let resp = ui.interact(r, Id::unique("chat.stage.quality"), Sense::hover());
        crate::a11y::item(&resp, egui::Role::Label, &tip);
        crate::widgets::tooltip::attach(resp, &tip);
        if fps.is_some() {
            // The rate is measured over a second: look again then.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
    let count = Type::BodyStrong.numeric_galley(ui, &w.watching.to_string(), Color32::WHITE);
    let r = chips.at_end(0.0, 14.0 + 6.0 + count.size().x + pad * 2.0, true);
    chip_plate(&painter, r, 0.0, alpha);
    let eye = ui.interact(r, Id::unique("chat.stage.count"), Sense::hover());
    crate::widgets::tooltip::attach(eye, &s::watching(w.watching));
    painter.text(
        pos2(r.left() + pad + 7.0, r.center().y),
        Align2::CENTER_CENTER,
        icons::EYE.text(false),
        icons::font(14.0),
        fade(Color32::WHITE, alpha),
    );
    painter.galley_with_override_text_color(
        pos2(r.left() + pad + 20.0, r.center().y - count.size().y / 2.0),
        count,
        fade(Color32::WHITE, alpha),
    );
}

fn text_chip(painter: &Painter, r: Rect, g: std::sync::Arc<egui::Galley>, alpha: f32) {
    chip_plate(painter, r, 0.0, alpha);
    painter.galley_with_override_text_color(
        pos2(r.left() + m::CHIP[2], r.center().y - g.size().y / 2.0),
        g,
        fade(Color32::WHITE, alpha),
    );
}

/// How much the bottom row shows: while pointed at or a control has focus (140 both
/// ways); in full screen only after the pointer moved in the last 2 s.
fn bottom_shown(ui: &Ui, rect: Rect, full: bool) -> f32 {
    let th = theme::theme(ui.ctx());
    let mut on = ui.rect_contains_pointer(rect) || stage_focus(ui.ctx());
    if full {
        let key = Id::unique("chat.stage.moved");
        let now = ui.input(|i| i.time);
        if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            ui.data_mut(|d| d.insert_temp(key, now));
        }
        let moved = ui.data(|d| d.get_temp::<f64>(key)).unwrap_or(f64::MIN);
        let left = m::STILL_FOR - (now - moved);
        if left > 0.0 {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
        on = (on && left > 0.0) || stage_focus(ui.ctx());
    }
    ui.ctx().animate_bool_with_time(
        Id::unique("chat.stage.bottom"),
        on,
        th.motion.fade(Dur::Fast),
    )
}

/// Bottom: sound (off: streams carry no sound yet) · larger · full screen · Stop watching.
fn bottom(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    chips: &Chips,
    alpha: f32,
    full: bool,
) {
    let [_, ch, pad, gap] = m::CHIP;
    let large = state.chat.stage_large;
    let sound_r = chips.at(0.0, ch, false);
    let sound = ui.interact(sound_r, Id::unique("chat.stage.sound"), Sense::hover());
    crate::a11y::button(&sound, cs::VOLUME());
    crate::widgets::tooltip::disabled(sound, cs::NO_SOUND());
    chip_plate(ui.painter(), sound_r, 0.0, alpha);
    ui.painter().text(
        sound_r.center(),
        Align2::CENTER_CENTER,
        icons::SPEAKER_OFF.text(false),
        icons::font(14.0),
        fade(Color32::WHITE, alpha * 0.45),
    );
    let stop_g = Type::BodyStrong.line(ui, s::STOP_WATCHING(), Color32::WHITE);
    let stop_r = chips.at_end(0.0, stop_g.size().x + pad * 2.0, false);
    let stop = stage_button(
        ui,
        stop_r,
        "chat.stage.stop",
        Glyph::Text(stop_g),
        s::STOP_WATCHING(),
        alpha,
    );
    let full_r = chips.at_end(stop_r.width() + gap, ch, false);
    let (full_name, full_glyph) = if full {
        (cs::EXIT_FULL(), Glyph::Corners(true))
    } else {
        (cs::FULL(), Glyph::Corners(false))
    };
    let fs = stage_button(ui, full_r, "chat.stage.full", full_glyph, full_name, alpha);
    let size = (!full).then(|| {
        let r = chips.at_end(stop_r.width() + gap + ch + gap, ch, false);
        let icon = if large { icons::SHRINK } else { icons::EXPAND };
        let name = if large { s::SMALLER() } else { s::LARGER() };
        stage_button(ui, r, "chat.stage.size", Glyph::Icon(icon), name, alpha)
    });
    if size.is_some_and(|r| r.clicked()) {
        state.chat.stage_large = !large;
    }
    if fs.clicked() {
        state.chat.call.full = !full;
    }
    if stop.clicked() {
        state.chat.stage_large = false;
        state.chat.call.full = false;
        env.bridge.send(Command::Chat(ChatCommand::StopWatch));
    }
}

/// "Waiting for the picture…": black, the game's poster 48 wide, a spinner, the words.
fn waiting(ui: &Ui, rect: Rect, title: &str, alpha: f32) {
    let th = theme::theme(ui.ctx());
    let painter = ui.painter();
    let corner = CornerRadius::same(pic::RADIUS);
    painter.rect_filled(rect, corner, fade(Color32::BLACK, alpha));
    let pw = 48.0;
    let ph = pw * theme::tokens::layout::COVER_ASPECT;
    let words = Type::Body.galley(
        ui,
        cs::WATCH_WAITING(),
        Color32::WHITE,
        rect.width() - 24.0,
        1,
    );
    let total = ph + 10.0 + 16.0 + 10.0 + words.size().y;
    let top = rect.center().y - total / 2.0;
    let poster = Rect::from_min_size(pos2(rect.center().x - pw / 2.0, top), vec2(pw, ph));
    crate::widgets::cover::paint(
        painter,
        poster,
        crate::widgets::cover::CoverSource::Placeholder,
        title,
        crate::widgets::cover::CoverLook {
            opacity: alpha,
            ..Default::default()
        },
        &th,
    );
    let spin = pos2(rect.center().x, poster.bottom() + 10.0 + 8.0);
    crate::widgets::progress::spinner(ui, spin, 8.0, fade(Color32::WHITE, alpha));
    let wy = spin.y + 8.0 + 10.0;
    painter.galley_with_override_text_color(
        pos2(rect.center().x - words.size().x / 2.0, wy),
        words,
        fade(Color32::WHITE, 0.78 * alpha),
    );
}

/// The stream of `chat` ended (the friend stopped): the last frame dimmed, "<name> stopped
/// sharing" and Close; it closes itself after 8 s.
pub fn ended(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    inset: f32,
) {
    let th = theme::theme(ui.ctx());
    let rect = place(ui, state, inset);
    let alpha = motion::appear(
        ui.ctx(),
        Id::unique(("chat.stage.ended", chat)),
        th.motion.fade(Dur::Base),
    );
    let name = paint::short_name(snap, chat);
    let title = snap
        .friends
        .iter()
        .find(|f| ChatRef::Direct(f.id) == chat)
        .and_then(|f| f.playing.as_ref())
        .map(|p| p.title.clone())
        .unwrap_or_default();
    pic::picture(ui, rect, &title, alpha, None, Some(pic::ENDED));
    let words = Type::Heading.galley(
        ui,
        &cs::ended(&name),
        Color32::WHITE,
        rect.width() - 24.0,
        2,
    );
    let close_g = Type::BodyStrong.line(ui, cs::CLOSE(), Color32::WHITE);
    let cw = close_g.size().x + m::CHIP[2] * 2.0;
    let total = words.size().y + 12.0 + m::CHIP[1];
    let y = rect.center().y - total / 2.0;
    ui.painter().galley_with_override_text_color(
        pos2(rect.center().x - words.size().x / 2.0, y),
        words.clone(),
        fade(Color32::WHITE, alpha),
    );
    let close_r = Rect::from_min_size(
        pos2(rect.center().x - cw / 2.0, y + words.size().y + 12.0),
        vec2(cw, m::CHIP[1]),
    );
    let close = stage_button(
        ui,
        close_r,
        "chat.stage.close",
        Glyph::Text(close_g),
        cs::CLOSE(),
        alpha,
    );
    let since = *state.chat.call.ended_at.get_or_insert(env.mono);
    let gone = env.mono.saturating_duration_since(since);
    if gone < m::ENDED_FOR {
        ui.ctx().request_repaint_after(m::ENDED_FOR - gone);
    }
    if close.clicked() || gone >= m::ENDED_FOR {
        state.chat.call.ended_at = None;
        // Clears the ended stream from the snapshot (and nothing else: none is watched).
        env.bridge.send(Command::Chat(ChatCommand::StopWatch));
        pic::forget(ui);
    }
}

fn live_galley(painter: &Painter, label: &str) -> std::sync::Arc<egui::Galley> {
    painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(11.0, egui::FontFamily::Name("w600".into())),
        Color32::WHITE,
    )
}

/// A chip's black plate; `hover` 0–1 darkens it.
fn chip_plate(painter: &Painter, r: Rect, hover: f32, alpha: f32) {
    let [rest, over] = m::CHIP_ALPHA;
    let a = rest + (over - rest) * hover;
    painter.rect_filled(
        r,
        CornerRadius::same(theme::tokens::radius::MD),
        fade(Color32::BLACK, a * alpha),
    );
    if theme::theme(painter.ctx()).palette.outlined() {
        painter.rect_stroke(
            r,
            CornerRadius::same(theme::tokens::radius::MD),
            Stroke::new(1.0, fade(Color32::WHITE, 0.6 * alpha)),
            egui::StrokeKind::Inside,
        );
    }
}

enum Glyph {
    Icon(theme::Icon),
    Text(std::sync::Arc<egui::Galley>),
    /// Four corners, pointing out (full screen) or in (leave it).
    Corners(bool),
}

/// A chip that acts (`button.sb`), at `alpha`.
fn stage_button(
    ui: &mut Ui,
    r: Rect,
    id: &str,
    glyph: Glyph,
    name: &str,
    alpha: f32,
) -> egui::Response {
    let th = theme::theme(ui.ctx());
    // Hidden at rest, but still there for the keyboard (focus reveals the row).
    let resp = ui.interact(r, Id::unique(id), Sense::click());
    crate::a11y::button(&resp, name);
    let resp = if matches!(glyph, Glyph::Text(_)) {
        resp
    } else {
        crate::widgets::tooltip::attach(resp, name)
    };
    let hover = crate::widgets::hover_t(&resp, &th);
    let painter = ui.painter();
    chip_plate(painter, r, hover, alpha);
    let white = fade(Color32::WHITE, alpha);
    match glyph {
        Glyph::Icon(i) => {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                i.text(false),
                icons::font(14.0),
                white,
            );
        }
        Glyph::Text(g) => {
            painter.galley_with_override_text_color(
                pos2(r.left() + m::CHIP[2], r.center().y - g.size().y / 2.0),
                g,
                white,
            );
        }
        Glyph::Corners(inward) => corners(painter, r.center(), inward, white),
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(
            painter,
            r,
            CornerRadius::same(theme::tokens::radius::MD),
            &th,
        );
    }
    resp
}

/// The corners-out glyph (12 across, 4 long arms), or corners-in.
fn corners(painter: &Painter, c: egui::Pos2, inward: bool, color: Color32) {
    let (h, arm) = (6.0, 4.0);
    let stroke = Stroke::new(1.5, color);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        // Out: the elbow at the corner, arms back towards the middle. In: the elbow nearer
        // the middle, arms out towards the corner.
        let (reach, dir) = if inward { (h - arm, 1.0) } else { (h, -1.0) };
        let elbow = pos2(c.x + sx * reach, c.y + sy * reach);
        painter.line_segment([elbow, pos2(elbow.x + dir * sx * arm, elbow.y)], stroke);
        painter.line_segment([elbow, pos2(elbow.x, elbow.y + dir * sy * arm)], stroke);
    }
}
