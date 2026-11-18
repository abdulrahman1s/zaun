//! What the Chat page paints itself: QR codes, call buttons, and text that keeps its own
//! direction (a message in Arabic reads right to left whatever the interface's language).

use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, FontId, Galley, Id, Painter, Rect, Response, Sense, Ui, pos2,
    vec2,
};

use crate::a11y;
use crate::i18n::bidi;
use crate::theme::palette;
use crate::theme::{self, Type};

/// Whether `text` reads right to left: its first strong letter is right-to-left.
pub fn reads_rtl(text: &str) -> bool {
    bidi::has_rtl(text) && unicode_bidi::get_base_direction(text) == unicode_bidi::Direction::Rtl
}

/// Text laid out in its own direction: the lines in order and whether they stand at the
/// right end of the measure.
pub struct Para {
    /// Lines, top first.
    pub lines: Vec<Arc<Galley>>,
    /// Right to left: each line is aligned to the right.
    pub rtl: bool,
}

impl Para {
    /// `text` in `style`, wrapped at `width`.
    pub fn new(ui: &Ui, text: &str, style: Type, color: Color32, width: f32) -> Self {
        if !bidi::has_rtl(text) {
            let job = style.job(text, color, width, usize::MAX);
            return Self {
                lines: vec![ui.fonts_mut(|f| f.layout_job(job))],
                rtl: false,
            };
        }
        // Broken into lines in logical order and each put in visual order, with the
        // paragraph's direction taken from the text itself.
        let format = style.format(color, false);
        let lines = ui.fonts_mut(|f| {
            let mut measure = |s: &str| {
                f.layout_job(LayoutJob::single_section(s.to_owned(), format.clone()))
                    .size()
                    .x
            };
            let shaped = bidi::wrapped(&mut measure, text, width.max(1.0), usize::MAX, None);
            shaped
                .split('\n')
                .map(|l| f.layout_job(LayoutJob::single_section(l.to_owned(), format.clone())))
                .collect()
        });
        Self {
            lines,
            rtl: reads_rtl(text),
        }
    }

    /// The widest line and the height of all.
    pub fn size(&self) -> egui::Vec2 {
        let w = self.lines.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let h = self.lines.iter().map(|g| g.size().y).sum();
        vec2(w, h)
    }

    /// Paint the lines in `rect` (as wide as [`Para::size`] or wider).
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let mut y = rect.top();
        for g in self.lines {
            let h = g.size().y;
            let x = if self.rtl {
                rect.right() - g.size().x
            } else {
                rect.left()
            };
            painter.galley(pos2(x, y), g, color);
            y += h;
        }
    }
}

/// The modules of a QR code: its width and the dark ones, row by row.
#[derive(Clone)]
struct Modules(usize, Arc<Vec<bool>>);

fn modules(ctx: &egui::Context, link: &str) -> Option<Modules> {
    let key = Id::unique(("chat.qr", link));
    if let Some(m) = ctx.data(|d| d.get_temp::<Modules>(key)) {
        return Some(m);
    }
    // Low error correction: a code on a screen is not scuffed, and fewer modules make
    // each one larger (a 149-character link fits 53 modules with the quiet zone).
    let code =
        qrcode::QrCode::with_error_correction_level(link.as_bytes(), qrcode::EcLevel::L).ok()?;
    let w = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    let m = Modules(w, Arc::new(dark));
    ctx.data_mut(|d| d.insert_temp(key, m.clone()));
    Some(m)
}

/// Quiet zone around a QR code, in modules.
const QUIET: usize = 4;

/// A QR code of `link`, at most `side` square: dark modules on a light square with its
/// quiet zone, in every theme (scanners read dark on light). Each module is a whole number
/// of pixels. `name` is what a screen reader says; the link itself is never shown.
pub fn qr(ui: &mut Ui, link: &str, side: f32, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    a11y::item(&resp, egui::Role::Image, name);
    let Some(Modules(w, dark)) = modules(ui.ctx(), link) else {
        return resp;
    };
    let ppp = ui.ctx().pixels_per_point();
    let total = (w + QUIET * 2) as f32;
    // Whole pixels per module.
    let px = ((side * ppp) / total).floor().max(1.0);
    let module = px / ppp;
    let full = module * total;
    let c = rect.center() - vec2(full, full) / 2.0;
    let origin = pos2((c.x * ppp).round() / ppp, (c.y * ppp).round() / ppp);
    let square = Rect::from_min_size(origin, vec2(full, full));
    let painter = ui.painter();
    painter.rect_filled(
        square,
        CornerRadius::same(theme::tokens::radius::XS),
        Color32::WHITE,
    );
    let mut mesh = egui::Mesh::default();
    for y in 0..w {
        for x in 0..w {
            if dark[y * w + x] {
                let min = origin + vec2((x + QUIET) as f32, (y + QUIET) as f32) * module;
                mesh.add_colored_rect(
                    Rect::from_min_size(min, vec2(module, module)),
                    Color32::BLACK,
                );
            }
        }
    }
    painter.add(mesh);
    if theme::theme(ui.ctx()).palette.outlined() {
        painter.rect_stroke(
            square,
            CornerRadius::same(theme::tokens::radius::XS),
            egui::Stroke::new(
                theme::tokens::stroke::BORDER,
                theme::theme(ui.ctx()).palette.line,
            ),
            egui::StrokeKind::Outside,
        );
    }
    resp
}

// ── Calls (chat.html `.cc`, `.lv`, `.sb`) ──────────────────────────────────────────────

/// What a round call button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cc {
    /// Mute or stop hearing: inverted while on, with a stroke through the icon.
    Slash,
    /// Share your game: the accent fill while on.
    Share,
    /// Leave: coral.
    End,
    /// Stop sharing (the top bar): plain.
    Plain,
}

/// A round call button filling `rect` (36, or 24 in the top bar): `o2`, `o3` under the
/// pointer. `name` is what a screen reader says; `tip` the tooltip.
#[allow(clippy::too_many_arguments)]
pub fn call_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    icon: theme::Icon,
    kind: Cc,
    on: bool,
    name: &str,
    tip: &str,
) -> Response {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let resp = ui.interact(rect, id, Sense::click());
    match kind {
        Cc::Slash | Cc::Share => a11y::toggle(&resp, name, on),
        Cc::End | Cc::Plain => a11y::button(&resp, name),
    }
    let resp = crate::widgets::tooltip::attach(resp, tip);
    let hover = crate::widgets::hover_t(&resp, &th);
    let (fill, ink) = match (kind, on) {
        (Cc::Slash, true) => (p.t1, p.bg),
        (Cc::Share, true) => (palette::mix(p.fill, p.fill_hover, hover), p.on_fill),
        (Cc::End, _) => (p.coral, p.on_coral),
        _ => (palette::mix(p.o2, p.o3, hover), p.t1),
    };
    let side = rect.width();
    let painter = ui.painter();
    painter.circle_filled(rect.center(), side / 2.0, fill);
    let glyph = if side < 30.0 { 13.0 } else { 16.0 };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.text(false),
        theme::icons::font(glyph),
        ink,
    );
    if kind == Cc::Slash && on {
        // A stroke through the icon, ringed in the button's fill.
        let len = if side < 30.0 { 15.0 } else { 22.0 };
        let d = vec2(len, -len) / (2.0 * std::f32::consts::SQRT_2);
        let (a, b) = (rect.center() - d, rect.center() + d);
        painter.line_segment([a, b], egui::Stroke::new(2.0 + 3.0, fill));
        painter.line_segment([a, b], egui::Stroke::new(2.0, ink));
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(
            painter,
            rect,
            CornerRadius::same(theme::tokens::radius::PILL),
            &th,
        );
    }
    resp
}

/// The alpha of a live or recording dot: 1 ↔ .45 every 1.6 s while the window is in front
/// and motion is allowed; 1 otherwise.
pub fn pulse_alpha(ctx: &egui::Context) -> f32 {
    if !moving(ctx) {
        return 1.0;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
    let t = (ctx.input(|i| i.time) / 1.6).fract() as f32;
    // ease-in-out 1 → .45 → 1.
    let x = 1.0 - (t * std::f32::consts::TAU).cos();
    1.0 - 0.55 * x / 2.0
}

/// Whether continuous effects run: motion is allowed and the window is in front (as the
/// running game's bars do).
pub fn moving(ctx: &egui::Context) -> bool {
    theme::theme(ctx).motion.shimmer() && ctx.input(|i| i.viewport().focused == Some(true))
}

/// The live mark (`.lv`): a 6 pt dot and, when `label` is not empty, the word in 11 / 600,
/// starting at `left` on the line `y`. Returns its width.
pub fn live_mark(painter: &Painter, left: f32, y: f32, label: &str, color: Color32) -> f32 {
    let alpha = pulse_alpha(painter.ctx());
    let r = 3.0;
    painter.circle_filled(pos2(left + r, y), r, palette::fade(color, alpha));
    if label.is_empty() {
        return r * 2.0;
    }
    let g = painter.layout_no_wrap(
        label.to_owned(),
        FontId::new(11.0, egui::FontFamily::Name("w600".into())),
        color,
    );
    let w = g.size().x;
    painter.galley(pos2(left + r * 2.0 + 5.0, y - g.size().y / 2.0), g, color);
    r * 2.0 + 5.0 + w
}

/// The coral of the live mark on the black stage chips.
pub const LIVE_ON_DARK: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x7C);

/// "1:05", "12:30": a call's clock from `ms`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// How a conversation shows its face: the name, the avatar's pair and its shape.
pub fn face(
    snap: &zaun_core::model::ChatSnapshot,
    chat: zaun_core::model::ChatRef,
) -> (String, Option<u8>, super::avatar::Shape) {
    use zaun_core::model::ChatRef;
    match chat {
        ChatRef::Direct(id) => {
            let f = snap.friend(id);
            (
                f.map(|f| f.name.clone()).unwrap_or_default(),
                f.and_then(|f| f.paper),
                super::avatar::Shape::Person,
            )
        }
        ChatRef::Group(id) => (
            snap.group(id).map(|g| g.name.clone()).unwrap_or_default(),
            None,
            super::avatar::Shape::Group,
        ),
    }
}

/// A person's first name ("Mira" of "Mira Okafor"); a group keeps its whole name.
pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The short name of a conversation: a friend's first name, a group's name.
pub fn short_name(
    snap: &zaun_core::model::ChatSnapshot,
    chat: zaun_core::model::ChatRef,
) -> String {
    let (name, _, shape) = face(snap, chat);
    match shape {
        super::avatar::Shape::Person => first_name(&name).to_owned(),
        super::avatar::Shape::Group => name,
    }
}

/// A button 34 high (or `rect`'s height) painted by hand: the boxed look (`o1`, `o2` under
/// the pointer) or the accent fill, with an optional leading icon.
pub fn block_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    icon: Option<theme::Icon>,
    primary: bool,
) -> Response {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let resp = ui.interact(rect, id, Sense::click());
    a11y::button(&resp, label);
    let hover = crate::widgets::hover_t(&resp, &th);
    let pressed = crate::widgets::press_t(&resp, &th);
    let (fill, ink, weight) = if primary {
        (
            palette::mix(
                palette::mix(p.fill, p.fill_hover, hover),
                p.fill_pressed,
                pressed,
            ),
            p.on_fill,
            Type::BodyBold,
        )
    } else {
        (
            palette::mix(palette::mix(p.o1, p.o2, hover), p.o3, pressed),
            p.t1,
            Type::BodyStrong,
        )
    };
    let painter = ui.painter();
    let corner = CornerRadius::same(theme::tokens::radius::MD);
    painter.rect_filled(rect, corner, fill);
    let g = weight.line(ui, label, ink);
    let icon_w = if icon.is_some() { 16.0 + 7.0 } else { 0.0 };
    let w = icon_w + g.size().x;
    let x = rect.center().x - w / 2.0;
    if let Some(i) = icon {
        painter.text(
            pos2(x + 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            i.text(false),
            theme::icons::font(16.0),
            ink,
        );
    }
    painter.galley(pos2(x + icon_w, rect.center().y - g.size().y / 2.0), g, ink);
    if p.outlined() {
        painter.rect_stroke(
            rect,
            corner,
            egui::Stroke::new(theme::tokens::stroke::BORDER, p.line),
            egui::StrokeKind::Inside,
        );
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(painter, rect, corner, &th);
    }
    resp
}

/// `widget` in `r`, laid out in a child of `ui`: unlike `Ui::put`, the parent's cursor
/// does not move (the caller has already allocated the row `r` sits in), and `r` may hang
/// past the column.
pub fn place(ui: &mut Ui, r: Rect, widget: impl egui::Widget) -> Response {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(r)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(widget)
}

/// A date or a clock ("12/11/2026 21:40", "Today at 13:59", "اليوم في 13:59") in `style`
/// with tabular figures. Each date and clock is one left-to-right run; in a right-to-left
/// interface the runs read from the right (the date first, then the time), even when the
/// text has no Arabic letter to set the line's direction (notes A, Arabic).
pub fn time_galley(ui: &Ui, text: &str, style: Type, color: Color32) -> Arc<Galley> {
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let job = LayoutJob::single_section(time_order(text, rtl), style.format(color, true));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The order [`time_galley`] gives the engine, left to right on screen.
fn time_order(text: &str, rtl: bool) -> String {
    if !rtl {
        text.to_owned()
    } else if bidi::has_rtl(text) {
        bidi::visual_line(text, Some(crate::i18n::Dir::Rtl)).into_owned()
    } else {
        text.split(' ').rev().collect::<Vec<_>>().join(" ")
    }
}

/// The full-date tooltip of a time: 12 medium in the tooltip tokens, radius 6, padded 5 · 9,
/// centred 6 above the time (6 below when there is no room). It waits the style's 500 ms;
/// moving to another time keeps it open (egui's grace time) and it fades in over 100.
pub fn time_tip(resp: &Response, text: &str) {
    let mut tip = egui::Tooltip::for_enabled(resp);
    tip.popup = tip
        .popup
        .frame(egui::Frame::NONE)
        .gap(TIP_GAP)
        .align(egui::RectAlign::TOP)
        .align_alternatives(&[egui::RectAlign::BOTTOM]);
    tip.show(|ui| {
        let theme = theme::theme(ui.ctx());
        let p = &theme.palette;
        let a = theme::motion::appear(
            ui.ctx(),
            ui.layer_id().id.with("time.tip"),
            theme.motion.fade(theme::Dur::Tip),
        );
        ui.multiply_opacity(a);
        let g = Type::Label.galley(ui, text, p.tip_fg, f32::INFINITY, 1);
        let size = g.size() + vec2(TIP_PAD[0], TIP_PAD[1]) * 2.0;
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        a11y::item(&resp, egui::Role::Tooltip, text);
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::tokens::radius::SM),
            p.tip_bg,
        );
        ui.painter()
            .galley(rect.min + vec2(TIP_PAD[0], TIP_PAD[1]), g, p.tip_fg);
    });
}

/// The time tooltip's padding (across, down) and its distance from the time.
const TIP_PAD: [f32; 2] = [9.0, 5.0];
const TIP_GAP: f32 = 6.0;

/// The side a QR code of `link` needs for whole modules of at least `min_px` pixels at
/// one pixel per point (quiet zone included), at least `side` (the design's size).
pub fn qr_side(ctx: &egui::Context, link: &str, side: f32, min_px: f32) -> f32 {
    modules(ctx, link).map_or(side, |Modules(w, _)| {
        side.max((w + QUIET * 2) as f32 * min_px)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_a_right_to_left_line_a_date_reads_first_from_the_right_and_stays_one_run() {
        assert_eq!(time_order("12/11/2026 21:40", true), "21:40 12/11/2026");
        assert_eq!(time_order("12/11/2026 21:40", false), "12/11/2026 21:40");
        assert_eq!(time_order("13:59", true), "13:59");
        // An Arabic head: the clock stays one left-to-right run, at the left end.
        let shown = time_order("اليوم في 13:59", true);
        assert!(shown.starts_with("13:59 "), "{shown}");
    }

    /// The engine's short links (about 103 characters, 149 on a custom relay) get modules
    /// of at least 3 px at 1× in a 168 pt code.
    #[test]
    fn short_links_fit_168_points_at_three_pixels_a_module() {
        let ctx = egui::Context::default();
        for n in [70, 103, 116, 149] {
            let link = format!("https://smp14.simplex.im/i#{}", "x".repeat(n - 27));
            assert_eq!(link.len(), n);
            assert_eq!(qr_side(&ctx, &link, 168.0, 3.0), 168.0, "{n} characters");
        }
    }
}
