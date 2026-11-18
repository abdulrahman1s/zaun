//! Icon-only buttons; the label is both the tooltip and the accessible name.
//!
//! The plain one is the design's `.b.ib`: a 30 square with a 16 pt glyph, no fill until
//! pointed at (`o2`, primary glyph), `o3` and 97 % while pressed. The round ones beside
//! Play (favourite, more) are 44 across on `o1` with a primary glyph, `o2` under the
//! pointer. A favourite that turns on pops its star from 70 % (360 ms, a 5 % overshoot).

use egui::{Color32, CornerRadius, Response, Sense, Shape, Ui, Widget, vec2};

use super::{
    add_scaled, contrast_outline, focus_ring, glyph_shape, pointer_t, press_scale, press_t, tooltip,
};
use crate::a11y;
use crate::theme::motion::{ease_pop, scale_about, since_change};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, radius, travel};
use crate::theme::{self, Dur, Icon};

/// An icon-only control. The `label` is both the tooltip and the accessible name.
#[must_use = "add it with `ui.add(icon_button)`"]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    shortcut: Option<&'a str>,
    selected: bool,
    size: Option<f32>,
    icon_px: f32,
    toggle: Option<bool>,
    round: bool,
    filled: Option<bool>,
}

impl<'a> IconButton<'a> {
    /// A 30 square button with a 16 pt glyph.
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            shortcut: None,
            selected: false,
            size: None,
            icon_px: icon_size::INLINE,
            toggle: None,
            round: false,
            filled: None,
        }
    }

    /// Draw the glyph's filled weight (`true`) or its outline (`false`) whatever the
    /// state: Play in a run button is solid. By default the glyph fills while selected or
    /// toggled on.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = Some(filled);
        self
    }

    /// Show the shortcut in the tooltip.
    pub fn shortcut(mut self, s: &'a str) -> Self {
        self.shortcut = Some(s);
        self
    }

    /// Selected: the glyph's filled weight in primary text over the pointed-at fill.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Expose as a two-state toggle (favourite) with this pressed state; turning on pops
    /// the glyph.
    pub fn toggle(mut self, pressed: bool) -> Self {
        self.toggle = Some(pressed);
        self
    }

    /// Square (or round) size of the button.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// Glyph size.
    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_px = px;
        self
    }

    /// A round button beside Play: 44 across (unless sized), `o1` at rest, `o2` under the
    /// pointer, primary glyph.
    pub fn round(mut self, round: bool) -> Self {
        self.round = round;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = theme::theme(ui.ctx());
        let p = &theme.palette;
        let side = self.size.unwrap_or(if self.round {
            control::ROUND
        } else {
            control::BUTTON
        });
        let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
        match self.toggle {
            Some(pressed) => a11y::toggle(&resp, self.label, pressed),
            None => a11y::button(&resp, self.label),
        }
        if ui.is_rect_visible(rect) {
            let enabled = ui.is_enabled();
            let hover = if enabled {
                pointer_t(&resp, &theme)
            } else {
                0.0
            };
            let pressed = if enabled { press_t(&resp, &theme) } else { 0.0 };
            let corner = CornerRadius::same(if self.round { radius::PILL } else { radius::MD });
            let filled = self
                .filled
                .unwrap_or(self.selected || self.toggle == Some(true));
            let (fill, color) = if !enabled {
                (
                    if self.round {
                        p.o1
                    } else {
                        Color32::TRANSPARENT
                    },
                    p.t4,
                )
            } else if self.round {
                // `.ga .ib`: o1, o2 under the pointer (and while pressed).
                (mix(p.o1, p.o2, hover.max(pressed)), p.t1)
            } else if self.selected {
                (mix(p.o2, p.o3, pressed), p.t1)
            } else {
                (
                    mix(fade(p.o2, hover), p.o3, pressed),
                    mix(p.t2, p.t1, hover),
                )
            };
            let painter = ui.painter();
            let mut shapes = vec![Shape::rect_filled(rect, corner, fill)];
            if let Some(o) = contrast_outline(rect, corner, &theme) {
                shapes.push(o);
            }
            let mut glyph = vec![glyph_shape(
                painter,
                rect.center(),
                self.icon,
                filled,
                self.icon_px,
                color,
            )];
            // The favourite's pop when it turns on.
            if let Some(on) = self.toggle
                && !theme.motion.reduced
            {
                let secs = Dur::Slow.secs();
                if let Some(t) = since_change(ui.ctx(), resp.id.with("pop"), on, secs, false)
                    && on
                {
                    let k = ease_pop((t / secs).clamp(0.0, 1.0));
                    let s = travel::STAR_FROM + (1.0 - travel::STAR_FROM) * k;
                    let tr = scale_about(rect.center(), s, egui::Vec2::ZERO);
                    for g in &mut glyph {
                        g.transform(tr);
                    }
                }
            }
            shapes.extend(glyph);
            add_scaled(painter, rect.center(), press_scale(pressed, &theme), shapes);
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        tooltip::tooltip(resp, self.label, self.shortcut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::icons;

    #[test]
    fn plain_is_30_and_round_is_44() {
        let (a, b) = crate::widgets::test_frame(&Default::default(), |ui| {
            let a = ui.add(IconButton::new(icons::DOTS, "More")).rect;
            let b = ui
                .add(IconButton::new(icons::STAR, "Favourite").round(true))
                .rect;
            (a, b)
        });
        assert_eq!(a.size(), egui::Vec2::splat(control::BUTTON));
        assert_eq!(b.size(), egui::Vec2::splat(control::ROUND));
    }
}
