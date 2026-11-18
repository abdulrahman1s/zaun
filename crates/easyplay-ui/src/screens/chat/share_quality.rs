//! Stream quality (`chat-calls.html` notes J; ADR-0036): the picture size and frame rate a
//! share sends. The sharer's live chip ("1080p · 60") opens one menu with both choices and
//! sends `ChatCommand::SetShareQuality`, which applies at once; the chip shows what is sent,
//! in amber while that is less than what was chosen. The encoder is named only in the
//! sharer's tooltip.

use easyplay_core::config::schema::{Preferences, ShareFps, ShareQuality};
use easyplay_core::model::{StreamLive, VideoEncoding};
use easyplay_core::service::{ChatCommand, ShareHeight};
use egui::{
    Align2, Color32, CornerRadius, Id, Popup, PopupCloseBehavior, Rect, RectAlign, Response, Sense,
    Ui, pos2, vec2,
};

use super::Env;
use crate::bridge::Command;
use crate::strings::chat::calls as cs;
use crate::theme::palette::{fade, mix};
use crate::theme::{self, Type, icons};
use crate::widgets::context_menu;

/// The sizes in the order they are offered.
pub const HEIGHTS: [ShareQuality; 4] = [
    ShareQuality::P720,
    ShareQuality::P1080,
    ShareQuality::P1440,
    ShareQuality::Source,
];
/// The frame rates offered.
pub const RATES: [u32; 2] = [30, 60];

/// The live chip: height, radius, side padding, the caret's size; the menu's width.
const CHIP: [f32; 4] = [28.0, 8.0, 10.0, 12.0];
const MENU_W: f32 = 260.0;
/// A rate this far below the chosen one counts as less than chosen.
const FPS_SLACK: u32 = 5;

/// The most pixels high a size sends (`None`: the game's own size).
pub fn height_px(q: ShareQuality) -> Option<u32> {
    match q {
        ShareQuality::P720 => Some(720),
        ShareQuality::P1080 => Some(1080),
        ShareQuality::P1440 => Some(1440),
        ShareQuality::Source => None,
    }
}

/// "1080p", or "As in the game".
pub fn height_label(q: ShareQuality) -> String {
    height_px(q).map_or_else(|| cs::SOURCE().to_owned(), cs::resolution)
}

/// The frame rate of a setting.
pub fn fps_of(f: ShareFps) -> u32 {
    match f {
        ShareFps::F30 => 30,
        ShareFps::F60 => 60,
    }
}

/// The game window's height when the share tells it: sent at the game's own size, or sent
/// smaller than chosen (a share never sends more pixels than the game draws).
pub fn game_height(stream: &StreamLive) -> Option<u32> {
    let sent = stream.sending?;
    match stream.share_height {
        None => Some(sent.height),
        Some(h) if sent.height < h => Some(sent.height),
        Some(_) => None,
    }
}

/// What was chosen: the size and the rate (the preferences until the share reports them).
fn chosen(stream: &StreamLive, prefs: &Preferences) -> (ShareQuality, u32) {
    let height = if stream.sending.is_some() || stream.share_fps != 0 {
        match stream.share_height {
            None => ShareQuality::Source,
            Some(h) => HEIGHTS
                .into_iter()
                .find(|q| height_px(*q) == Some(h))
                .unwrap_or(prefs.chat.share_quality),
        }
    } else {
        prefs.chat.share_quality
    };
    let fps = if stream.share_fps == 0 {
        fps_of(prefs.chat.share_fps)
    } else {
        stream.share_fps
    };
    (height, fps)
}

/// The chip's words, whether what is sent is less than what was chosen, and the tooltip.
pub fn chip_text(stream: &StreamLive, prefs: &Preferences) -> (String, bool, Option<String>) {
    let (q, fps) = chosen(stream, prefs);
    let Some(sent) = stream.sending else {
        return (cs::chip(&height_label(q), fps), false, None);
    };
    let res = cs::resolution(sent.height);
    let lower = height_px(q).is_some_and(|h| sent.height < h) || sent.fps + FPS_SLACK < fps;
    let tip = cs::sending(&res, sent.fps, sent.encoder == VideoEncoding::Hardware);
    (cs::chip(&res, sent.fps), lower, Some(tip))
}

/// The chip's width for `text`.
pub fn chip_width(ui: &Ui, text: &str) -> f32 {
    let [_, _, pad, caret] = CHIP;
    let g = Type::Caption.numeric_galley(ui, text, Color32::PLACEHOLDER);
    pad + g.size().x + 4.0 + caret + pad
}

/// The sharer's chip in `rect` (a select on the share tray), and its menu.
pub fn chip(
    ui: &mut Ui,
    env: &mut Env<'_>,
    rect: Rect,
    text: &str,
    lower: bool,
    tip: Option<&str>,
) {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let resp = ui.interact(rect, Id::unique("chat.quality.chip"), Sense::click());
    crate::a11y::button(&resp, &crate::strings::chat::spoken(&[cs::CHANGE(), text]));
    let resp = match tip {
        Some(t) => crate::widgets::tooltip::attach(resp, t),
        None => resp,
    };
    let hover = crate::widgets::hover_t(&resp, &th);
    let [_, radius, pad, caret] = CHIP;
    let corner = CornerRadius::same(radius as u8);
    ui.painter()
        .rect_filled(rect, corner, mix(p.o1, p.o2, hover));
    let ink = if lower { p.amber } else { p.t1 };
    let g = Type::Caption.numeric_galley(ui, text, ink);
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let (tx, cx) = if rtl {
        (
            rect.right() - pad - g.size().x,
            rect.left() + pad + caret / 2.0,
        )
    } else {
        (rect.left() + pad, rect.right() - pad - caret / 2.0)
    };
    ui.painter()
        .galley(pos2(tx, rect.center().y - g.size().y / 2.0), g, ink);
    ui.painter().text(
        pos2(cx, rect.center().y),
        Align2::CENTER_CENTER,
        icons::CARET_DOWN.text(false),
        icons::font(caret),
        fade(p.t2, 1.0),
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), rect, corner, &th);
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            Popup::open_id(ui.ctx(), Popup::default_response_id(&resp));
        }
    }
    menu(env, &resp);
}

/// One menu with both choices; it stays open after a pick (Esc or a click outside closes
/// it).
fn menu(env: &mut Env<'_>, chip: &Response) {
    let snap = env.bridge.chat().clone();
    let prefs = env.bridge.prefs().clone();
    let stream = &snap.live.stream;
    let (q, fps) = chosen(stream, &prefs);
    let game = game_height(stream);
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let popup = Popup::menu(chip)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(if rtl {
            RectAlign::BOTTOM_START
        } else {
            RectAlign::BOTTOM_END
        })
        .width(MENU_W);
    let mut pick: Option<(ShareHeight, u32)> = None;
    context_menu::popup(popup, |ui| {
        ui.set_width(MENU_W - 10.0);
        context_menu::section(ui, cs::PICTURE());
        for h in HEIGHTS {
            let label = height_label(h);
            let too_big = height_px(h).zip(game).is_some_and(|(want, has)| want > has);
            if too_big {
                ui.add_enabled_ui(false, |ui| {
                    let r = context_menu::check_item_with(ui, &label, h == q, Some(cs::TOO_BIG()));
                    crate::a11y::describe(&r, cs::TOO_BIG());
                });
                continue;
            }
            let after = (h == ShareQuality::Source)
                .then(|| game.map(cs::resolution))
                .flatten();
            if context_menu::check_item_with(ui, &label, h == q, after.as_deref()).clicked() {
                pick = Some((h, fps));
            }
        }
        context_menu::separator(ui);
        context_menu::section(ui, cs::FRAME_RATE());
        for f in RATES {
            let hint = (f == 30).then(cs::FPS30_HINT);
            if context_menu::check_item_with(ui, &cs::fps(f), f == fps, hint).clicked() {
                pick = Some((q, f));
            }
        }
        context_menu::separator(ui);
        let p = theme::theme(ui.ctx()).palette.clone();
        let w = ui.available_width() - 16.0;
        let g = Type::Micro.galley(ui, cs::LIVE_NOTE(), p.t3, w.max(1.0), usize::MAX);
        let (r, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), g.size().y + 8.0), Sense::hover());
        ui.painter()
            .galley(pos2(r.left() + 8.0, r.top() + 2.0), g, p.t3);
    });
    if let Some((height, fps)) = pick {
        env.bridge
            .send(Command::Chat(ChatCommand::SetShareQuality { height, fps }));
    }
}

/// Frames a second as a watcher sees them: 30 or 60 when within 3 of them.
pub fn rounded_fps(measured: f32) -> u32 {
    let n = measured.round() as u32;
    RATES.into_iter().find(|r| r.abs_diff(n) <= 3).unwrap_or(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::model::StreamInfo;

    fn sent(height: u32, fps: u32) -> Option<StreamInfo> {
        Some(StreamInfo {
            encoder: VideoEncoding::Hardware,
            width: height * 16 / 9,
            height,
            fps,
        })
    }

    #[test]
    fn the_game_height_is_known_only_from_its_own_size_or_a_smaller_send() {
        let mut s = StreamLive {
            share_height: None,
            share_fps: 60,
            sending: sent(1080, 60),
            ..StreamLive::default()
        };
        assert_eq!(game_height(&s), Some(1080));
        s.share_height = Some(1440);
        assert_eq!(game_height(&s), Some(1080), "sent below the chosen 1440");
        s.share_height = Some(720);
        s.sending = sent(720, 60);
        assert_eq!(game_height(&s), None, "the game may be larger");
    }

    #[test]
    fn the_chip_shows_what_is_sent_and_turns_amber_below_the_choice() {
        let prefs = Preferences::default();
        let mut s = StreamLive {
            share_height: Some(1080),
            share_fps: 60,
            sending: sent(1080, 60),
            ..StreamLive::default()
        };
        let (text, lower, tip) = chip_text(&s, &prefs);
        assert_eq!(text, "1080p · 60");
        assert!(!lower);
        assert!(tip.is_some_and(|t| t.contains("hardware")));
        s.share_height = Some(1440);
        assert!(chip_text(&s, &prefs).1, "1080 sent of 1440 chosen");
        s.share_height = Some(1080);
        s.sending = sent(1080, 41);
        assert!(chip_text(&s, &prefs).1, "41 of 60 frames");
        s.sending = None;
        s.share_fps = 0;
        assert_eq!(
            chip_text(&s, &prefs).0,
            "1080p · 60",
            "the settings before it runs"
        );
    }

    #[test]
    fn a_watchers_rate_snaps_to_30_or_60_when_close() {
        assert_eq!(rounded_fps(58.6), 60);
        assert_eq!(rounded_fps(31.2), 30);
        assert_eq!(rounded_fps(45.0), 45);
    }
}
