//! Segmented control: a 28 high track (`o1`, radius 8, 2 padding) holding exclusive
//! options, each as wide as its label + 20 (an icon + 20 when it has no label). Labels are
//! 13 / 500 in quiet text, secondary under the pointer, primary when chosen. The thumb
//! (`thumb`, radius 6) slides to the chosen option's place and width (220 ms, `ease_out`);
//! with nothing chosen there is no thumb. Arrow keys move the choice. Exposed as a radio
//! group.

use egui::accesskit::Role;
use egui::{CornerRadius, Galley, Key, Rect, Response, Sense, Shape, Ui, pos2, vec2};
use std::sync::Arc;

use super::{contrast_outline, focus_ring, glyph_shape, pointer_t, tooltip};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, icon as icon_size, radius};
use crate::theme::{self, Dur, Icon, Type, motion};

/// One option.
#[derive(Clone, Copy, Debug)]
pub struct Segment<'a> {
    /// Optional icon.
    pub icon: Option<Icon>,
    /// Label (shown unless `icon_only`, always the accessible name).
    pub label: &'a str,
}

/// Show the control. `icon_only` hides labels (the covers / list switch) and shows them as
/// tooltips. `*selected` past the last option chooses nothing. Returns the group's
/// response; `changed()` when the choice moved.
pub fn segmented(
    ui: &mut Ui,
    group_label: &str,
    selected: &mut usize,
    options: &[Segment<'_>],
    icon_only: bool,
) -> Response {
    segmented_impl(ui, group_label, selected, options, icon_only, &[])
}

/// [`segmented`] whose options say what they do: `hints[i]` is the tooltip and the
/// accessible description of option `i` (the protection levels).
pub fn segmented_with_hints(
    ui: &mut Ui,
    group_label: &str,
    selected: &mut usize,
    options: &[Segment<'_>],
    hints: &[&str],
) -> Response {
    segmented_impl(ui, group_label, selected, options, false, hints)
}

fn measure(
    ui: &Ui,
    options: &[Segment<'_>],
    icon_only: bool,
) -> (Vec<Option<Arc<Galley>>>, Vec<f32>) {
    let galleys: Vec<_> = options
        .iter()
        .map(|o| {
            (!icon_only).then(|| Type::BodyStrong.line(ui, o.label, egui::Color32::PLACEHOLDER))
        })
        .collect();
    let widths = options
        .iter()
        .zip(&galleys)
        .map(|(o, g)| content_width(o, g.as_ref()) + control::SEGMENT_PAD * 2.0)
        .collect();
    (galleys, widths)
}

fn content_width(o: &Segment<'_>, g: Option<&Arc<Galley>>) -> f32 {
    let icon_w = if o.icon.is_some() {
        icon_size::INLINE
    } else {
        0.0
    };
    let gap = if o.icon.is_some() && g.is_some() {
        control::BUTTON_GAP
    } else {
        0.0
    };
    icon_w + gap + g.map_or(0.0, |g| g.size().x)
}

fn segmented_impl(
    ui: &mut Ui,
    group_label: &str,
    selected: &mut usize,
    options: &[Segment<'_>],
    icon_only: bool,
    hints: &[&str],
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let h = control::SM;
    let inset = control::SEGMENT_INSET;
    let (galleys, widths) = measure(ui, options, icon_only);
    let total: f32 = widths.iter().sum::<f32>() + inset * 2.0;
    let (outer, mut group) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    a11y::labelled(ui.ctx(), group.id, Role::RadioGroup, group_label);

    let corner = CornerRadius::same(radius::MD);
    let inner_corner = CornerRadius::same(radius::SM);
    let painter = ui.painter().clone();
    painter.rect_filled(outer, corner, p.o1);
    if let Some(o) = contrast_outline(outer, corner, &theme) {
        painter.add(o);
    }

    let mut x = outer.left() + inset;
    let rects: Vec<Rect> = widths
        .iter()
        .map(|w| {
            let r = Rect::from_min_size(pos2(x, outer.top() + inset), vec2(*w, h - inset * 2.0));
            x += w;
            r
        })
        .collect();
    let before = *selected;
    let resps: Vec<Response> = options
        .iter()
        .zip(&rects)
        .enumerate()
        .map(|(i, (opt, r))| {
            let resp = ui.interact(*r, group.id.with(i), Sense::click());
            super::own_arrow_keys(&resp);
            a11y::option(&resp, Role::RadioButton, opt.label, before == i);
            if resp.clicked() {
                *selected = i;
            }
            if resp.has_focus() {
                let (left, right) = ui.input(|inp| {
                    (
                        inp.key_pressed(Key::ArrowLeft) || inp.key_pressed(Key::ArrowUp),
                        inp.key_pressed(Key::ArrowRight) || inp.key_pressed(Key::ArrowDown),
                    )
                });
                if left && i > 0 {
                    *selected = i - 1;
                } else if right && i + 1 < options.len() {
                    *selected = i + 1;
                }
                if (left || right) && *selected != i {
                    ui.memory_mut(|m| m.request_focus(group.id.with(*selected)));
                }
            }
            if icon_only {
                tooltip::attach(resp, opt.label)
            } else if let Some(hint) = hints.get(i) {
                a11y::describe(&resp, hint);
                tooltip::attach(resp, hint)
            } else {
                resp
            }
        })
        .collect();

    // The thumb glides along the track: its place is kept relative to the track, so a
    // page that scrolls or a row that moves the control does not make it trail.
    if let Some(target) = rects.get(*selected) {
        let secs = theme.motion.movement(Dur::Base);
        let left = motion::glide(
            ui.ctx(),
            group.id.with("thumb.x"),
            target.left() - outer.left(),
            secs,
        );
        let width = motion::glide(ui.ctx(), group.id.with("thumb.w"), target.width(), secs);
        let thumb = Rect::from_min_size(
            pos2(outer.left() + left, target.top()),
            vec2(width, target.height()),
        );
        painter.rect_filled(thumb, inner_corner, p.thumb);
    }

    for (i, (((opt, g), r), resp)) in options
        .iter()
        .zip(galleys)
        .zip(&rects)
        .zip(&resps)
        .enumerate()
    {
        let is_sel = *selected == i;
        let hover = if ui.is_enabled() {
            pointer_t(resp, &theme)
        } else {
            0.0
        };
        let fg = if is_sel { p.t1 } else { mix(p.t3, p.t2, hover) };
        let content = content_width(opt, g.as_ref());
        let mut cx = r.center().x - content / 2.0;
        if let Some(icon) = opt.icon {
            // The design's icons are outlines whether chosen or not (the thumb shows which).
            painter.add(glyph_shape(
                &painter,
                pos2(cx + icon_size::INLINE / 2.0, r.center().y),
                icon,
                false,
                icon_size::INLINE,
                fg,
            ));
            cx += icon_size::INLINE
                + if g.is_some() {
                    control::BUTTON_GAP
                } else {
                    0.0
                };
        }
        if let Some(g) = g {
            painter.add(Shape::galley(
                pos2(cx, r.center().y - g.size().y / 2.0),
                g,
                fg,
            ));
        }
        if resp.has_focus() {
            focus_ring(&painter, *r, inner_corner, &theme);
        }
    }
    if *selected != before {
        group.mark_changed();
    }
    group
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_is_28_high_and_its_segments_are_their_labels_plus_20() {
        let (r, label_w) = crate::widgets::test_frame(&Default::default(), |ui| {
            let mut chosen = 0;
            let opts = [Segment {
                icon: None,
                label: "Covers",
            }];
            let w = Type::BodyStrong
                .line(ui, "Covers", egui::Color32::WHITE)
                .size()
                .x;
            (segmented(ui, "Show as", &mut chosen, &opts, false).rect, w)
        });
        assert_eq!(r.height(), control::SM);
        assert_eq!(
            r.width(),
            label_w + control::SEGMENT_PAD * 2.0 + control::SEGMENT_INSET * 2.0
        );
    }
}
