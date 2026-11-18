//! Icons from the app's icon font, and the two the font lacks drawn as strokes: the slash
//! over a muted microphone or headphones, and the pin.
use egui::{Align2, Color32, Painter, Pos2, Stroke, vec2};
use zaun_ui::theme::Icon;
use zaun_ui::theme::icons;

pub fn icon(painter: &Painter, center: Pos2, i: Icon, size: f32, colour: Color32) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        i.regular,
        icons::font(size),
        colour,
    );
}

/// A stroke through an icon, with a ring of `under` so it reads over the glyph: rising like
/// the call buttons' `[data-slash]` line, or falling like the widget's mic-off glyph.
pub fn slash(
    painter: &Painter,
    center: Pos2,
    len: f32,
    colour: Color32,
    under: Color32,
    rising: bool,
) {
    let d = vec2(len, if rising { -len } else { len }) * 0.5_f32.sqrt() / 2.0;
    painter.line_segment([center - d, center + d], Stroke::new(3.0, under));
    painter.line_segment([center - d, center + d], Stroke::new(1.3, colour));
}

/// A pin (the "Show while playing" button): a head, a plate and a needle.
pub fn pin(painter: &Painter, center: Pos2, size: f32, colour: Color32) {
    let s = size / 14.0;
    let stroke = Stroke::new(1.3 * s, colour);
    let top = center + vec2(0.0, -6.0 * s);
    let head = egui::Rect::from_min_max(top + vec2(-3.0 * s, 0.0), top + vec2(3.0 * s, 5.0 * s));
    painter.rect_stroke(head, 1.0, stroke, egui::StrokeKind::Middle);
    let plate = top + vec2(0.0, 5.5 * s);
    painter.line_segment(
        [plate + vec2(-5.0 * s, 0.0), plate + vec2(5.0 * s, 0.0)],
        stroke,
    );
    painter.line_segment([plate, plate + vec2(0.0, 6.5 * s)], stroke);
}
