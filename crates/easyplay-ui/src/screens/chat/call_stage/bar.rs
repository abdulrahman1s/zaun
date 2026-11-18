//! The stage's control bar (notes D "CONTROL BAR"): the status at the start (quality bars
//! and the clock, or Calling / Connecting / Reconnecting), the round controls in the middle,
//! Show everyone and Make smaller at the end.

use easyplay_core::model::{Call, CallQuality, ChatSnapshot};
use easyplay_core::service::ChatCommand;
use egui::{Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Ui, pos2, vec2};

use super::tiles::{self, Phase};
use crate::app::UiState;
use crate::bridge::Command;
use crate::route::{Route, SettingsTab};
use crate::screens::Env;
use crate::screens::chat::paint;
use crate::strings::chat::calls::{self as cs, Grade};
use crate::strings::chat::details as s;
use crate::theme::palette::{Palette, fade, mix};
use crate::theme::{self, Icon, Type, icons};
use crate::vm::dialogs::AfterDiscard;
use crate::widgets::icon_button::IconButton;

/// The round controls: side, gap, glyph; the mic capsule's caret.
const CC: f32 = 40.0;
const CC_GAP: f32 = 8.0;
const GLYPH: f32 = 18.0;
const CARET: f32 = 24.0;
/// The end buttons.
const END: f32 = 30.0;

/// How a round control looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Inverted with a stroke through the glyph while on (mute, stop hearing).
    Slash,
    /// The accent fill while on (share).
    Accent,
    /// Coral (leave).
    End,
    Plain,
}

/// What the bar asked for this frame.
#[derive(Default)]
pub struct Asked {
    pub smaller: bool,
    pub everyone: bool,
}

/// The bar in `rect` (64 high, the stage's full width).
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    call: &Call,
    rect: Rect,
    focused: bool,
) -> Asked {
    let th = theme::theme(ui.ctx());
    let p = th.palette.clone();
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let inner = rect.shrink2(vec2(12.0, 0.0));
    let ph = tiles::phase(call, &snap.live);
    status(ui, env, call, snap.live.quality, ph, inner, rtl, &p);
    let share_on = snap.live.share.is_some();
    let n = 4.0 + f32::from(u8::from(snap.features.share));
    let w = CC + CARET + (n - 1.0) * CC + (n - 1.0) * CC_GAP;
    let mut x = inner.center().x - w / 2.0;
    let mut next = |width: f32| {
        let r = Rect::from_min_size(pos2(x, inner.center().y - CC / 2.0), vec2(width, CC));
        x += width + CC_GAP;
        super::layout::mirror(r, inner, rtl)
    };
    let (mute, caret) = mic(ui, next(CC + CARET), call.muted, rtl, &p);
    super::super::call_menu::show(ui, state, env, &caret);
    let deaf_tip = if call.deafened {
        s::UNDEAFEN()
    } else {
        s::DEAFEN()
    };
    let deaf = Round::new(
        "deaf",
        icons::HEADPHONES,
        Look::Slash,
        s::DEAFEN(),
        deaf_tip,
    )
    .on(call.deafened)
    .show(ui, next(CC));
    let share = snap.features.share.then(|| {
        // The engine holds one call per friend: a stream would end this call (ADR-0036).
        Round::new(
            "share",
            icons::SHARE_GAME,
            Look::Accent,
            s::SHARE_TIP(),
            s::STOP_SHARE_TIP(),
        )
        .on(share_on)
        .disabled((!share_on).then(cs::SHARE_IN_CALL))
        .show(ui, next(CC))
    });
    let gear = Round::new(
        "settings",
        icons::GEAR,
        Look::Plain,
        cs::SETTINGS(),
        cs::SETTINGS(),
    )
    .show(ui, next(CC));
    let leave_tip = if ph == Phase::Calling {
        s::CANCEL_CALL()
    } else {
        s::LEAVE_CALL()
    };
    let leave =
        Round::new("leave", icons::PHONE_END, Look::End, leave_tip, leave_tip).show(ui, next(CC));
    // The end: Make smaller, and Show everyone before it while a tile is focused.
    let mut end_x = 0.0;
    let mut at_end = || {
        let r = Rect::from_min_size(
            pos2(inner.right() - end_x - END, inner.center().y - END / 2.0),
            vec2(END, END),
        );
        end_x += END + CC_GAP;
        super::layout::mirror(r, inner, rtl)
    };
    let smaller = paint::place(ui, at_end(), IconButton::new(icons::SHRINK, cs::SMALLER()));
    let everyone = focused.then(|| {
        paint::place(
            ui,
            at_end(),
            IconButton::new(icons::GRID, cs::SHOW_EVERYONE()),
        )
    });
    if mute.clicked() {
        env.bridge
            .send(Command::Chat(ChatCommand::CallMute(!call.muted)));
    }
    if deaf.clicked() {
        env.bridge
            .send(Command::Chat(ChatCommand::CallDeafen(!call.deafened)));
    }
    if share_on && share.is_some_and(|r| r.clicked()) {
        env.bridge.send(Command::Chat(ChatCommand::StopShare));
        state.toast(
            s::STOPPED_SHARING(),
            crate::vm::toasts::ToastKind::Info,
            None,
            env.mono,
        );
    }
    if gear.clicked() {
        state.navigate(AfterDiscard::Route(Route::Settings(SettingsTab::Chat)));
    }
    if leave.clicked() {
        env.bridge.send(Command::Chat(ChatCommand::EndCall));
    }
    Asked {
        smaller: smaller.clicked(),
        everyone: everyone.is_some_and(|r| r.clicked()),
    }
}

/// The status at the bar's start.
#[allow(clippy::too_many_arguments)]
fn status(
    ui: &Ui,
    env: &Env<'_>,
    call: &Call,
    quality: Option<CallQuality>,
    ph: Phase,
    inner: Rect,
    rtl: bool,
    p: &Palette,
) {
    let painter = ui.painter();
    let mut x = 0.0;
    let at = |x: f32, w: f32| {
        if rtl {
            inner.right() - x - w
        } else {
            inner.left() + x
        }
    };
    let cy = inner.center().y;
    let bars_for = match ph {
        Phase::Reconnecting => Some(Grade::Poor),
        Phase::Live => quality.map(grade),
        Phase::Calling | Phase::Connecting => None,
    };
    if let Some(g) = bars_for {
        let r = Rect::from_min_size(pos2(at(x, 13.0), cy - 5.0), vec2(13.0, 10.0));
        bars(painter, r, g, p);
        let tip = match (ph, quality) {
            (Phase::Live, Some(q)) => cs::quality(g, q.rtt_ms, q.loss),
            _ => cs::RECONNECTING().to_owned(),
        };
        let resp = ui.interact(
            r.expand(4.0),
            Id::unique("chat.call.quality"),
            Sense::hover(),
        );
        crate::a11y::item(&resp, egui::Role::Image, &tip);
        crate::widgets::tooltip::attach(resp, &tip);
        x += 13.0 + 8.0;
    }
    let (text, color, style) = match ph {
        Phase::Calling => {
            let firsts: Vec<&str> = call.with.iter().map(|n| paint::first_name(n)).collect();
            (s::calling(&s::and_list(&firsts)), p.t1, Type::BodyStrong)
        }
        Phase::Connecting => {
            let c = pos2(at(x, 12.0) + 6.0, cy);
            crate::widgets::progress::spinner(ui, c, 6.0, p.t2);
            x += 12.0 + 8.0;
            (cs::CONNECTING().to_owned(), p.t2, Type::Caption)
        }
        Phase::Reconnecting => (cs::RECONNECTING().to_owned(), p.amber, Type::Caption),
        Phase::Live => {
            let ms = call
                .since
                .and_then(|since| env.now.since(since))
                .map_or(0, |d| d.as_millis() as u64);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
            (paint::clock(ms), p.t2, Type::Caption)
        }
    };
    let g = if ph == Phase::Live {
        style.numeric_galley(ui, &text, color)
    } else {
        style.galley(ui, &text, color, (inner.width() / 3.0 - x).max(1.0), 1)
    };
    painter.galley(pos2(at(x, g.size().x), cy - g.size().y / 2.0), g, color);
}

/// Good under 150 ms and 3 % lost, fair under 300 ms and 10 %, else poor.
pub fn grade(q: CallQuality) -> Grade {
    match (q.rtt_ms, q.loss) {
        (0..150, 0..3) => Grade::Good,
        (0..300, 0..10) => Grade::Fair,
        _ => Grade::Poor,
    }
}

/// Three bars 3 wide, 4 / 7 / 10 high, 2 apart: all `t3` when good, two amber when fair,
/// one coral when poor (neutral when nothing is wrong, on purpose).
fn bars(painter: &egui::Painter, r: Rect, g: Grade, p: &Palette) {
    let lit = match g {
        Grade::Good => (3, p.t3),
        Grade::Fair => (2, p.amber),
        Grade::Poor => (1, p.coral),
    };
    for (i, h) in [4.0, 7.0, 10.0].into_iter().enumerate() {
        let x = r.left() + i as f32 * 5.0;
        let bar = Rect::from_min_max(pos2(x, r.bottom() - h), pos2(x + 3.0, r.bottom()));
        let c = if i < lit.0 { lit.1 } else { p.o3 };
        painter.rect_filled(bar, CornerRadius::same(1), c);
    }
}

/// The mic as a split capsule: the mic (mute) and a caret that opens the device menu. The
/// whole capsule inverts while muted.
fn mic(ui: &mut Ui, rect: Rect, muted: bool, rtl: bool, p: &Palette) -> (Response, Response) {
    let th = theme::theme(ui.ctx());
    let (mic_r, caret_r) = if rtl {
        rect.split_left_right_at_x(rect.left() + CARET)
    } else {
        rect.split_left_right_at_x(rect.right() - CARET)
    };
    let (mic_r, caret_r) = if rtl {
        (caret_r, mic_r)
    } else {
        (mic_r, caret_r)
    };
    let m = ui.interact(mic_r, Id::unique("chat.call.mic"), Sense::click());
    let c = ui.interact(caret_r, Id::unique("chat.call.caret"), Sense::click());
    crate::a11y::toggle(&m, s::MUTE_MIC(), muted);
    crate::a11y::button(&c, cs::DEVICES());
    let m = crate::widgets::tooltip::attach(m, if muted { s::UNMUTE() } else { s::MUTE() });
    let c = crate::widgets::tooltip::attach(c, cs::DEVICES());
    let (fill, ink) = if muted { (p.t1, p.bg) } else { (p.o2, p.t1) };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(20), fill);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&c));
    for (resp, part) in [(&m, mic_r), (&c, caret_r)] {
        let lit = open && part == caret_r;
        let hover = if lit {
            1.0
        } else {
            crate::widgets::hover_t(resp, &th)
        };
        if hover > 0.0 && !muted {
            let corner = if (part == mic_r) != rtl {
                CornerRadius {
                    nw: 20,
                    sw: 20,
                    ne: 0,
                    se: 0,
                }
            } else {
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 20,
                    se: 20,
                }
            };
            painter.rect_filled(part, corner, fade(p.o3, hover));
        }
    }
    if muted {
        tiles::slashed(painter, mic_r.center(), icons::MIC, GLYPH, ink);
    } else {
        glyph(painter, mic_r.center(), icons::MIC, ink);
    }
    painter.text(
        caret_r.center(),
        Align2::CENTER_CENTER,
        icons::CARET_DOWN.text(false),
        icons::font(13.0),
        ink,
    );
    for (resp, part) in [(&m, mic_r), (&c, caret_r)] {
        if resp.has_focus() {
            crate::widgets::focus_ring(painter, part, CornerRadius::same(20), &th);
        }
    }
    (m, c)
}

fn glyph(painter: &egui::Painter, c: egui::Pos2, icon: Icon, color: Color32) {
    painter.text(
        c,
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(GLYPH),
        color,
    );
}

/// A round control of 40: `name` for a screen reader, `tip` its tooltip, or the reason it
/// is off when disabled.
struct Round<'a> {
    key: &'a str,
    icon: Icon,
    look: Look,
    name: &'a str,
    tip: &'a str,
    on: bool,
    disabled: Option<&'a str>,
}

impl<'a> Round<'a> {
    fn new(key: &'a str, icon: Icon, look: Look, name: &'a str, tip: &'a str) -> Self {
        Self {
            key,
            icon,
            look,
            name,
            tip,
            on: false,
            disabled: None,
        }
    }

    fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    fn disabled(mut self, why: Option<&'a str>) -> Self {
        self.disabled = why;
        self
    }

    fn show(self, ui: &mut Ui, rect: Rect) -> Response {
        let th = theme::theme(ui.ctx());
        let p = &th.palette;
        let sense = if self.disabled.is_some() {
            Sense::hover()
        } else {
            Sense::click()
        };
        let resp = ui.interact(rect, Id::unique(("chat.call.cc", self.key)), sense);
        if let Some(why) = self.disabled {
            crate::a11y::button(&resp, self.name);
            let resp = crate::widgets::tooltip::disabled(resp, why);
            ui.painter().circle_filled(rect.center(), CC / 2.0, p.o1);
            glyph(ui.painter(), rect.center(), self.icon, p.t4);
            return resp;
        }
        match self.look {
            Look::Slash | Look::Accent => crate::a11y::toggle(&resp, self.name, self.on),
            Look::End | Look::Plain => crate::a11y::button(&resp, self.name),
        }
        let resp = crate::widgets::tooltip::attach(resp, self.tip);
        let hover = crate::widgets::hover_t(&resp, &th);
        let (fill, ink) = match (self.look, self.on) {
            (Look::Slash, true) => (p.t1, p.bg),
            (Look::Accent, true) => (mix(p.fill, p.fill_hover, hover), p.on_fill),
            (Look::End, _) => (p.coral, p.on_coral),
            _ => (mix(p.o2, p.o3, hover), p.t1),
        };
        let pressed = resp.is_pointer_button_down_on() && !th.motion.reduced;
        let r = if pressed { CC / 2.0 * 0.94 } else { CC / 2.0 };
        let painter = ui.painter();
        painter.circle_filled(rect.center(), r, fill);
        if self.look == Look::Slash && self.on {
            tiles::slashed(painter, rect.center(), self.icon, GLYPH, ink);
        } else {
            glyph(painter, rect.center(), self.icon, ink);
        }
        if resp.has_focus() {
            crate::widgets::focus_ring(painter, rect, CornerRadius::same(20), &th);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_are_neutral_until_the_path_is_slow_or_lossy() {
        let q = |rtt_ms, loss| CallQuality { rtt_ms, loss };
        assert_eq!(grade(q(42, 0)), Grade::Good);
        assert_eq!(grade(q(140, 2)), Grade::Good);
        assert_eq!(grade(q(160, 0)), Grade::Fair);
        assert_eq!(grade(q(80, 5)), Grade::Fair);
        assert_eq!(grade(q(310, 0)), Grade::Poor);
        assert_eq!(grade(q(80, 12)), Grade::Poor);
    }
}
