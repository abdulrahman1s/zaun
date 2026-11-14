//! Icon-only button: ghost style, tooltip and accessible name always (USER_EXPERIENCE §8).

use egui::{Align2, CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, Widget, pos2, vec2};

use super::{focus_ring, hover_t, press_t, tooltip};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{alpha, control, icon as icon_size, radius, stroke};
use crate::theme::{self, Icon, icons};

/// An icon-only control. The `label` is both the tooltip and the accessible name.
#[must_use = "add it with `ui.add(icon_button)`"]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    shortcut: Option<&'a str>,
    selected: bool,
    size: f32,
    icon_px: f32,
    toggle: Option<bool>,
}

impl<'a> IconButton<'a> {
    /// A 32 px button with a 20 px icon.
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            shortcut: None,
            selected: false,
            size: control::MIN_TARGET,
            icon_px: icon_size::BUTTON,
            toggle: None,
        }
    }

    /// Show the shortcut in the tooltip.
    pub fn shortcut(mut self, s: &'a str) -> Self {
        self.shortcut = Some(s);
        self
    }

    /// Selected: fill-weight icon in the accent colour with a subtle fill.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Expose as a two-state toggle (favourite) with this pressed state.
    pub fn toggle(mut self, pressed: bool) -> Self {
        self.toggle = Some(pressed);
        self.selected = pressed;
        self
    }

    /// Square size of the hit target.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Glyph size.
    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_px = px;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = theme::theme(ui.ctx());
        let p = &theme.palette;
        let (rect, resp) = ui.allocate_exact_size(vec2(self.size, self.size), Sense::click());
        match self.toggle {
            Some(pressed) => a11y::toggle(&resp, self.label, pressed),
            None => a11y::button(&resp, self.label),
        }
        if ui.is_rect_visible(rect) {
            let hover = hover_t(&resp, &theme);
            let pressed = press_t(&resp, &theme);
            let corner = CornerRadius::same(radius::ROW);
            let painter = ui.painter();
            let fill = if self.selected {
                mix(p.accent_subtle, fade(p.accent, 0.22), hover.max(pressed))
            } else {
                mix(
                    fade(p.fill_selected, hover),
                    mix(p.fill_selected, p.text_primary, alpha::HOVER_LIGHTEN),
                    pressed,
                )
            };
            painter.rect_filled(rect, corner, fill);
            if !p.allows_translucency() {
                painter.rect_stroke(
                    rect,
                    corner,
                    Stroke::new(stroke::BORDER, p.border_strong),
                    StrokeKind::Inside,
                );
            }
            let color = if !ui.is_enabled() {
                fade(p.text_secondary, alpha::DISABLED)
            } else if self.selected {
                p.accent
            } else {
                mix(p.text_secondary, p.text_primary, hover)
            };
            painter.text(
                pos2(rect.center().x, rect.center().y),
                Align2::CENTER_CENTER,
                self.icon.text(self.selected),
                icons::font(self.icon_px),
                color,
            );
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        tooltip::tooltip(resp, self.label, self.shortcut)
    }
}
