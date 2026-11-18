//! The call box under a conversation's header (`chat-calls.html` notes D, E): the stage by
//! default, the 60 pt tray as its smaller form, one rounded box whose height animates both
//! ways (out growing, in shrinking, 300 ms; the two layers cross-fade over 140). The grip
//! under the stage resizes it; let go under 160 it becomes the tray.

mod bar;
mod layout;
mod tiles;

use easyplay_core::model::{Call, ChatRef, ChatSnapshot};
use egui::{CornerRadius, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::Env;
use crate::app::UiState;
use crate::theme::palette::fade;
use crate::theme::tokens::stroke;
use crate::theme::{self, Dur, motion};
use crate::vm::chat_call::Tile;
use layout::m;

/// The box's corner and the space under it.
const RADIUS: u8 = 14;
const BELOW: f32 = 10.0;
/// The grip: its hit area and its bar.
const GRIP: [f32; 4] = [64.0, 12.0, 32.0, 4.0];

/// The call box of `call` in its conversation, `inset` from the column's sides.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    call: &Call,
    inset: f32,
) {
    let th = theme::theme(ui.ctx());
    let p = th.palette.clone();
    let watched = snap.live.watch.as_ref().filter(|w| w.chat == call.chat);
    let vm = &state.chat.call;
    let stage_on = !vm.smaller;
    let window_h = ui.ctx().content_rect().height();
    let stage_h = vm
        .height
        .unwrap_or_else(|| layout::stage_height(window_h, watched.is_some()));
    let target = if stage_on { stage_h } else { m::TRAY };
    let id = Id::unique(("chat.call.box", call.chat));
    // The box follows the grip at once while it is dragged.
    let dragging = ui.ctx().dragged_id() == Some(grip_id());
    let secs = if dragging {
        0.0
    } else {
        th.motion.movement(Dur::Move)
    };
    let h = height(ui, id, call.chat, target, secs);
    let shown = motion::appear(ui.ctx(), id, th.motion.fade(Dur::Base));
    ui.ctx()
        .animate_bool_with_time(id.with("open"), true, th.motion.fade(Dur::Base));
    let col = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(col, h + BELOW * shown), Sense::hover());
    let rect = Rect::from_min_size(
        pos2(row.left() + inset, row.top()),
        vec2((col - inset * 2.0).max(1.0), h),
    );
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(id.with("shown"), (h, pass)));
    let corner = CornerRadius::same(RADIUS);
    ui.painter().rect_filled(rect, corner, fade(p.o1, shown));
    if p.outlined() {
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, p.line),
            StrokeKind::Inside,
        );
    }
    let stage_a =
        ui.ctx()
            .animate_bool_with_time(id.with("stage"), stage_on, th.motion.fade(Dur::Fast));
    if stage_a < 1.0 {
        let tray = Rect::from_min_size(rect.min, vec2(rect.width(), m::TRAY));
        let mut child = layer(ui, rect, (1.0 - stage_a) * shown);
        super::trays::call_tray_in(
            &mut child,
            state,
            env,
            snap,
            call,
            tray,
            (1.0 - stage_a) * shown,
        );
    }
    if stage_a > 0.0 {
        let mut child = layer(ui, rect, stage_a * shown);
        stage(&mut child, state, env, snap, call, rect, stage_a * shown);
        grip(&mut child, state, rect, stage_h);
    }
}

/// The box of a call that just ended in `chat`: it closes (height to 0 over 300, `in`; its
/// fill fades over 220), then nothing.
pub fn closing(ui: &mut Ui, chat: ChatRef, inset: f32) {
    let th = theme::theme(ui.ctx());
    let id = Id::unique(("chat.call.box", chat));
    let pass = ui.ctx().cumulative_pass_nr();
    let open = ui
        .data(|d| d.get_temp::<(f32, u64)>(id.with("shown")))
        .is_some_and(|(h, p)| h > 0.5 && p + 1 >= pass);
    if !open {
        return;
    }
    let h = height(ui, id, chat, 0.0, th.motion.movement(Dur::Move));
    let alpha = ui
        .ctx()
        .animate_bool_with_time(id.with("open"), false, th.motion.fade(Dur::Base));
    ui.data_mut(|d| d.insert_temp(id.with("shown"), (h, pass)));
    let col = ui.available_width();
    let (row, _) = ui.allocate_exact_size(
        vec2(col, h + BELOW * (h / m::TRAY).min(1.0)),
        Sense::hover(),
    );
    let rect = Rect::from_min_size(
        pos2(row.left() + inset, row.top()),
        vec2((col - inset * 2.0).max(1.0), h),
    );
    let p = &th.palette;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(RADIUS), fade(p.o1, alpha));
}

/// A child of `ui` over `rect`, clipped to it, at `alpha`.
fn layer(ui: &mut Ui, rect: Rect, alpha: f32) -> Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.multiply_opacity(alpha);
    child
}

/// The stage's inside: the tiles (or a focused tile over the strip) and the bar.
fn stage(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    call: &Call,
    rect: Rect,
    alpha: f32,
) {
    let watched = snap.live.watch.as_ref().filter(|w| w.chat == call.chat);
    let people = tiles::people(snap, call);
    let vm = &state.chat.call;
    // A friend's shared game takes the big tile unless the user chose otherwise.
    let focus = if vm.everyone {
        None
    } else {
        vm.focus
            .clone()
            .filter(|f| *f != Tile::Stream || watched.is_some())
            .or_else(|| watched.map(|_| Tile::Stream))
    };
    let mut entries: Vec<Entry<'_>> = Vec::new();
    if let Some(w) = watched {
        entries.push(Entry::Stream(w));
    }
    entries.extend(people.iter().map(Entry::Person));
    let area = Rect::from_min_max(
        pos2(rect.left() + m::PAD, rect.top() + m::PAD),
        pos2(rect.right() - m::PAD, rect.bottom() - m::BAR),
    );
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let frames = env.bridge.frames();
    let mut clicked: Option<Tile> = None;
    let mut draw = |ui: &mut Ui, e: &Entry<'_>, r: Rect, size: tiles::Size| {
        let resp = match e {
            Entry::Stream(w) => {
                let who = match size {
                    tiles::Size::Grid => crate::strings::chat::details::stage_who(
                        super::paint::first_name(&w.name),
                        &w.title,
                    ),
                    tiles::Size::Strip => super::paint::first_name(&w.name).to_owned(),
                };
                tiles::stream(ui, r, &w.title, &who, frames, alpha)
            }
            Entry::Person(p) => tiles::person(ui, r, p, size, alpha),
        };
        if resp.clicked() {
            clicked = Some(e.key());
        }
    };
    match &focus {
        None => {
            for (e, r) in entries.iter().zip(layout::grid(entries.len(), area)) {
                draw(ui, e, layout::mirror(r, area, rtl), tiles::Size::Grid);
            }
        }
        Some(key) => {
            let (big, rest): (Vec<_>, Vec<_>) = entries.iter().partition(|e| e.key() == *key);
            let (big_r, strip) = layout::focused(rest.len(), area);
            for e in big {
                draw(ui, e, big_r, tiles::Size::Grid);
            }
            for (e, r) in rest.into_iter().zip(strip) {
                draw(ui, e, layout::mirror(r, area, rtl), tiles::Size::Strip);
            }
        }
    }
    let bar_r = Rect::from_min_max(pos2(rect.left(), rect.bottom() - m::BAR), rect.max);
    let asked = bar::show(ui, state, env, snap, call, bar_r, focus.is_some());
    let vm = &mut state.chat.call;
    if let Some(t) = clicked {
        // The big tile goes back to everyone; any other becomes the big one.
        if focus.as_ref() == Some(&t) {
            vm.focus = None;
            vm.everyone = true;
        } else {
            vm.focus = Some(t);
            vm.everyone = false;
        }
    }
    if asked.everyone {
        vm.focus = None;
        vm.everyone = true;
    }
    if asked.smaller {
        vm.smaller = true;
    }
}

/// A tile on the stage: a friend's shared game or a person.
enum Entry<'a> {
    Stream(&'a easyplay_core::model::Watching),
    Person(&'a tiles::Person<'a>),
}

impl Entry<'_> {
    fn key(&self) -> Tile {
        match self {
            Entry::Stream(_) => Tile::Stream,
            Entry::Person(p) => p.key.clone(),
        }
    }
}

/// The grip at the stage's bottom centre: drag to resize, double-click or Enter to make the
/// box the tray, ↑ ↓ by 24.
fn grip(ui: &mut Ui, state: &mut UiState, rect: Rect, stage_h: f32) {
    let th = theme::theme(ui.ctx());
    let [w, h, bw, bh] = GRIP;
    let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - h / 2.0), vec2(w, h));
    let resp = ui.interact(r, grip_id(), Sense::click_and_drag());
    crate::a11y::button(&resp, crate::strings::chat::calls::RESIZE());
    let resp = crate::widgets::tooltip::attach(resp, crate::strings::chat::calls::RESIZE())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let lit = resp.hovered() || resp.has_focus() || resp.dragged();
    let p = &th.palette;
    ui.painter().rect_filled(
        Rect::from_center_size(r.center(), vec2(bw, bh)),
        CornerRadius::same(2),
        if lit { p.t2 } else { p.t4 },
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), r, CornerRadius::same(6), &th);
    }
    let convo_h = ui.ctx().content_rect().height();
    let vm = &mut state.chat.call;
    let set = |h: f32| Some(layout::grip_clamp(h, convo_h));
    if resp.dragged() {
        // Free while held (down to the tray's height), so letting go low can mean "tray".
        let most = layout::grip_clamp(f32::MAX, convo_h);
        vm.height = Some((stage_h + resp.drag_delta().y).clamp(m::TRAY, most));
    }
    if resp.drag_stopped() {
        if stage_h < m::GRIP_TRAY {
            vm.smaller = true;
            vm.height = None;
        } else {
            vm.height = set(stage_h);
        }
    }
    if resp.has_focus() {
        let (up, down, enter) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if up {
            vm.height = set(stage_h - m::GRIP_STEP);
        }
        if down {
            vm.height = set(stage_h + m::GRIP_STEP);
        }
        if enter {
            vm.smaller = true;
        }
    }
    if resp.double_clicked() {
        vm.smaller = true;
    }
}

fn grip_id() -> Id {
    Id::unique("chat.call.grip")
}

/// The box's height on its way to `target`: out (`1 − (1 − t)⁵`) while growing, in (`t³`)
/// while shrinking, over `secs`. It opens from 0 when the conversation was already showing
/// (a call that starts), and starts at its target when the conversation just opened.
fn height(ui: &Ui, id: Id, chat: ChatRef, target: f32, secs: f32) -> f32 {
    #[derive(Clone, Copy)]
    struct Tween {
        from: f32,
        to: f32,
        at: f64,
        pass: u64,
    }
    let ctx = ui.ctx();
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let key = id.with("tween");
    let seen = seen_last_pass(ui, chat);
    let mut t = ctx
        .data(|d| d.get_temp::<Tween>(key))
        .filter(|t| t.pass + 1 >= pass)
        .unwrap_or(Tween {
            from: if seen { 0.0 } else { target },
            to: target,
            at: now,
            pass,
        });
    let value = |t: &Tween| {
        let x = ((now - t.at) as f32 / secs.max(f32::EPSILON)).clamp(0.0, 1.0);
        let e = if t.to >= t.from {
            motion::ease_out(x)
        } else {
            motion::ease_in(x)
        };
        (t.from + (t.to - t.from) * e, x)
    };
    if (t.to - target).abs() > f32::EPSILON {
        t.from = value(&t).0;
        t.to = target;
        t.at = now;
    }
    let (v, x) = value(&t);
    t.pass = pass;
    ctx.data_mut(|d| d.insert_temp(key, t));
    if secs > 0.0 && x < 1.0 {
        ctx.request_repaint();
        v
    } else {
        target
    }
}

/// Whether the trays of `chat` were drawn in the last pass (so a box that appears now is a
/// call that started, and opens).
fn seen_last_pass(ui: &Ui, chat: ChatRef) -> bool {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data(|d| d.get_temp::<(ChatRef, u64)>(pass_key()))
        .is_some_and(|(c, p)| c == chat && p + 1 >= pass)
}

fn pass_key() -> Id {
    Id::unique("chat.call.trays_pass")
}

/// Note that the trays of `chat` were drawn in this pass (see [`seen_last_pass`]); call it
/// after drawing them.
pub fn note_pass(ui: &Ui, chat: ChatRef) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(pass_key(), (chat, pass)));
}
