//! Reusable controls with built-in states, keyboard behaviour and accessible names. Widgets never
//! send commands, read snapshots or allocate textures. See MODULES §8 and UI_DESIGN_SYSTEM §7.
//!
//! Every widget reads its colours and sizes from [`crate::theme::theme`]; none contains a
//! literal colour, size or duration.

pub mod banner;
pub mod bar_chart;
pub mod brand;
pub mod button;
pub mod card;
pub mod choice;
pub mod context_menu;
pub mod cover;
pub mod empty_state;
pub mod icon_button;
pub mod key_hint;
pub mod modal;
pub mod progress;
pub mod rows;
pub mod segmented;
pub mod select;
pub mod sheet;
pub mod skeleton;
pub mod slider;
pub mod tabs;
pub mod text_field;
pub mod toast;
pub mod toggle;
pub mod tooltip;
pub mod word;

use egui::{Context, CornerRadius, Id, Painter, Rect, Response, Stroke, StrokeKind};

use crate::theme::{Theme, tokens::stroke};

fn focus_source_id() -> Id {
    Id::unique("easyplay.focus_from_keyboard")
}

/// Remember whether focus was last moved with the keyboard (or a controller) or with the
/// pointer. Call once at the start of every frame (the shell does).
pub fn track_focus_source(ctx: &Context) {
    let (key, pointer) = ctx.input(|i| {
        (
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. })),
            i.pointer.any_pressed(),
        )
    });
    ctx.data_mut(|d| {
        // Until there is input, focus was placed by the application: no ring.
        let from_keyboard = d.get_temp_mut_or_insert_with(focus_source_id(), || false);
        if pointer || key {
            *from_keyboard = key && !pointer;
        }
    });
}

/// Whether focus rings are shown: after keyboard or controller input, not after a click.
/// A control that takes text shows its own focus state regardless. Without a shell (a widget
/// on its own) rings always show.
pub fn focus_visible(ctx: &Context) -> bool {
    crate::theme::controller_mode(ctx)
        || ctx
            .data(|d| d.get_temp::<bool>(focus_source_id()))
            .unwrap_or(true)
}

/// Paint the focus ring: `focus` colour, outside `rect` with a gap (ACCESSIBILITY §2). Only
/// while focus is being moved with the keyboard ([`focus_visible`]).
pub fn focus_ring(painter: &Painter, rect: Rect, corner: CornerRadius, theme: &Theme) {
    if focus_visible(painter.ctx()) {
        default_ring(painter, rect, corner, theme);
    }
}

/// The ring of a default action (the button `Enter` activates in a dialog): painted however
/// focus arrived, so the safe choice is always marked.
pub fn default_ring(painter: &Painter, rect: Rect, corner: CornerRadius, theme: &Theme) {
    // Controller mode: a larger ring, visible from the couch (USER_EXPERIENCE §5.3).
    let w = theme.focus_width()
        + if crate::theme::controller_mode(painter.ctx()) {
            1.0
        } else {
            0.0
        };
    let grow = stroke::FOCUS_GAP + w / 2.0;
    let corner = CornerRadius {
        nw: corner.nw.saturating_add(grow as u8),
        ne: corner.ne.saturating_add(grow as u8),
        sw: corner.sw.saturating_add(grow as u8),
        se: corner.se.saturating_add(grow as u8),
    };
    painter.rect_stroke(
        rect.expand(grow),
        corner,
        Stroke::new(w, theme.palette.accent_mark),
        StrokeKind::Middle,
    );
}

/// Opacity of a hover effect, animated with `dur.fast`.
pub fn hover_t(resp: &Response, theme: &Theme) -> f32 {
    resp.ctx.animate_bool_with_time(
        resp.id.with("hover"),
        resp.hovered() || resp.has_focus(),
        theme.motion.fade(crate::theme::Dur::Fast),
    )
}

/// How far the pointer is over a control (0–1), animated with `dur.fast`, linear. Unlike
/// [`hover_t`], keyboard focus does not count: the design marks focus with the ring only.
pub fn pointer_t(resp: &Response, theme: &Theme) -> f32 {
    resp.ctx.animate_bool_with_time(
        resp.id.with("pointer"),
        resp.hovered(),
        theme.motion.fade(crate::theme::Dur::Fast),
    )
}

/// How far a control is pressed (0–1): the press goes in over `dur.press` (80 ms) and
/// releases over `dur.fast`, linearly; it deepens a fill or shrinks a control.
pub fn press_t(resp: &Response, theme: &Theme) -> f32 {
    let down = resp.is_pointer_button_down_on();
    let secs = if down {
        theme.motion.fade(crate::theme::Dur::Press)
    } else {
        theme.motion.fade(crate::theme::Dur::Fast)
    };
    resp.ctx
        .animate_value_with_time(resp.id.with("press"), f32::from(u8::from(down)), secs)
}

/// The scale of a button for a press of `pressed` (0–1): 97 % while held; always 1 under
/// reduced motion, where nothing scales.
pub fn press_scale(pressed: f32, theme: &Theme) -> f32 {
    if theme.motion.reduced {
        1.0
    } else {
        1.0 - (1.0 - crate::theme::tokens::travel::PRESS) * pressed.clamp(0.0, 1.0)
    }
}

/// Add `shapes` to `painter`, scaled by `scale` about `pivot` (a pressed button, Play
/// growing in on a cover). At a scale of 1 they are added as they are.
pub fn add_scaled(painter: &Painter, pivot: egui::Pos2, scale: f32, shapes: Vec<egui::Shape>) {
    if (scale - 1.0).abs() < f32::EPSILON {
        painter.extend(shapes);
        return;
    }
    let t = crate::theme::motion::scale_about(pivot, scale, egui::Vec2::ZERO);
    painter.extend(shapes.into_iter().map(|mut s| {
        s.transform(t);
        s
    }));
}

/// One line of `text` in `font` set in a CSS line box `line_height` high (so it centres as
/// the design's text does), without a colour of its own: paint it with
/// [`egui::Shape::galley`] and the colour of the moment.
pub fn label_galley(
    painter: &Painter,
    text: &str,
    font: egui::FontId,
    line_height: f32,
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: font,
        line_height: Some(line_height),
        color: egui::Color32::PLACEHOLDER,
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(
        crate::theme::typography::shaped(text).into_owned(),
        format,
    );
    painter.ctx().fonts_mut(|f| f.layout_job(job))
}

/// A glyph of the icon font as a shape, centred at `center`, `size` points.
pub fn glyph_shape(
    painter: &Painter,
    center: egui::Pos2,
    icon: crate::theme::Icon,
    filled: bool,
    size: f32,
    color: egui::Color32,
) -> egui::Shape {
    let g = painter.layout_no_wrap(icon.text(filled), crate::theme::icons::font(size), color);
    egui::Shape::galley(center - g.size() / 2.0, g, color)
}

/// The 1 pt `line` outline the high-contrast theme draws inside buttons, the search field,
/// segmented controls, fields and selects. Nothing in the other themes.
pub fn contrast_outline(rect: Rect, corner: CornerRadius, theme: &Theme) -> Option<egui::Shape> {
    theme.palette.outlined().then(|| {
        egui::Shape::rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, theme.palette.line),
            StrokeKind::Inside,
        )
    })
}

/// Keep egui's own arrow-key focus movement away from a focused composite widget (grid
/// cells, list rows, segmented options, tabs) that moves focus itself.
pub fn own_arrow_keys(resp: &Response) {
    if resp.has_focus() {
        resp.ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
    }
}

/// Lay out one frame with the theme installed and return what `f` returned: the rig of the
/// widgets' unit tests. The fonts are loaded by a frame before it.
#[cfg(test)]
pub(crate) fn test_frame<R>(
    appearance: &crate::theme::Appearance,
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let ctx = Context::default();
    crate::theme::install(&ctx, appearance);
    let mut first = ctx.run_ui(egui::RawInput::default(), |_| {});
    first.textures_delta.clear();
    let mut f = Some(f);
    let mut out = None;
    let mut o = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        if let Some(f) = f.take() {
            out = Some(f(ui));
        }
    });
    o.textures_delta.clear();
    #[allow(clippy::expect_used)]
    out.expect("the frame ran")
}
