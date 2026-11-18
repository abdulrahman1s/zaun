//! The composer (16 in): the well with a 1-point line, radius 14, the accent ring while it
//! has the keyboard; 14/20 text; "Message Friday Crew" as its placeholder. It has the
//! keyboard when the panel opens, so typing goes there; Enter sends. Only what works here is
//! drawn: no attachments, GIFs, emoji picker or voice messages from inside a game.
use easyplay_core::chat::overlay::FromOverlay;
use easyplay_ui::theme::Type;
use egui::{CornerRadius, Key, Rect, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, vec2};

use crate::model::{Focus, Model};
use crate::words;

pub const HEIGHT: f32 = 44.0;

pub fn draw(ui: &mut Ui, m: &mut Model, rect: Rect) {
    let look = m.look;
    let Some(chat) = m.snap.open.as_ref().map(|o| o.chat) else {
        return;
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), look.well);
    let field = rect.shrink2(vec2(14.0, 11.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(field));
    let (hint_colour, font) = (look.t3, Type::Prose.font_id());
    let labels = &mut m.labels;
    let hint = std::sync::Arc::clone(labels.composer_hint.get_or_insert_with(|| {
        std::sync::Arc::new(
            egui::RichText::new(labels.composer.clone())
                .color(hint_colour)
                .font(font),
        )
    }));
    let edit = TextEdit::singleline(&mut m.draft)
        .frame(egui::Frame::NONE)
        .font(Type::Prose.font_id())
        .text_color(look.t1)
        .hint_text(egui::WidgetText::RichText(hint))
        .align(if words::rtl() {
            egui::Align2::RIGHT_CENTER
        } else {
            egui::Align2::LEFT_CENTER
        })
        .desired_width(field.width())
        .id_salt("ovl-composer");
    let r = child.add(edit);
    let focused = r.has_focus();
    let ring = if focused {
        Stroke::new(1.5, look.amark)
    } else {
        Stroke::new(1.0, look.line)
    };
    ui.painter()
        .rect_stroke(rect, CornerRadius::same(14), ring, StrokeKind::Inside);
    if m.focus == Some(Focus::Composer) {
        r.request_focus();
        m.focus = None;
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if enter {
        if !m.draft.trim().is_empty() {
            let text = std::mem::take(&mut m.draft);
            m.said.push(FromOverlay::SendMessage { chat, text });
        }
        // Keep typing: the composer keeps the keyboard after a send.
        m.focus = Some(Focus::Composer);
    }
}
