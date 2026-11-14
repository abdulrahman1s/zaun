//! Segmented control: 2–4 exclusive options with optional icons; arrow keys move the
//! selection (§7.4). Exposed as a radio group. The selected segment is a thumb that glides
//! to its new place when the selection changes.

use egui::accesskit::Role;
use egui::{
    Align2, CornerRadius, Key, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetText, pos2,
    vec2,
};

use super::{focus_ring, hover_t, tooltip};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Dur, Icon, Type, icons, motion};

/// One option.
#[derive(Clone, Copy, Debug)]
pub struct Segment<'a> {
    /// Optional icon.
    pub icon: Option<Icon>,
    /// Label (shown unless `icon_only`, always the accessible name).
    pub label: &'a str,
}

/// Show the control. `icon_only` hides labels (view switch in the toolbar) and shows them as
/// tooltips. Returns the group's response; `changed()` when the selection moved.
pub fn segmented(
    ui: &mut Ui,
    group_label: &str,
    selected: &mut usize,
    options: &[Segment<'_>],
    icon_only: bool,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let h = control::MD;
    let inset = control::SEGMENT_INSET;
    let pad = if icon_only {
        space::S2 + 2.0
    } else {
        space::S3
    };
    let icon_gap = space::S1 + 2.0;
    let galleys: Vec<_> = options
        .iter()
        .map(|o| {
            (!icon_only).then(|| {
                WidgetText::from(o.label).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    Type::BodyStrong.font_id(),
                )
            })
        })
        .collect();
    let widths: Vec<f32> = options
        .iter()
        .zip(&galleys)
        .map(|(o, g)| {
            let icon_w = if o.icon.is_some() {
                icon_size::INLINE
            } else {
                0.0
            };
            let gap = if o.icon.is_some() && g.is_some() {
                icon_gap
            } else {
                0.0
            };
            (pad * 2.0 + icon_w + gap + g.as_ref().map_or(0.0, |g| g.size().x))
                .max(control::MIN_TARGET)
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + inset * 2.0;
    let (outer, mut group) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    a11y::labelled(ui.ctx(), group.id, Role::RadioGroup, group_label);

    let corner = CornerRadius::same(radius::MD);
    let inner_corner = CornerRadius::same(radius::ROW - 1);
    let painter = ui.painter().clone();
    painter.rect_filled(outer, corner, p.fill_hover);
    painter.rect_stroke(
        outer,
        corner,
        Stroke::new(stroke::BORDER, p.hairline),
        StrokeKind::Inside,
    );

    // Segment rectangles, then interaction, so the thumb can be painted under the labels.
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
                if left && *selected > 0 {
                    *selected -= 1;
                }
                if right && *selected + 1 < options.len() {
                    *selected += 1;
                }
                if (left || right) && *selected != i {
                    ui.memory_mut(|m| m.request_focus(group.id.with(*selected)));
                }
            }
            if icon_only {
                tooltip::attach(resp, opt.label)
            } else {
                resp
            }
        })
        .collect();

    // The thumb.
    if let Some(target) = rects.get(*selected) {
        let secs = theme.motion.movement(Dur::Base);
        let left = motion::glide(ui.ctx(), group.id.with("thumb.l"), target.left(), secs);
        let right = motion::glide(ui.ctx(), group.id.with("thumb.r"), target.right(), secs);
        let thumb = Rect::from_min_max(pos2(left, target.top()), pos2(right, target.bottom()));
        // Lifted above the track: lighter than it in every theme.
        let fill = if p.kind.is_dark() {
            mix(p.bg_overlay, p.text_primary, 0.06)
        } else {
            p.bg_surface
        };
        painter.rect_filled(thumb, inner_corner, fill);
        painter.rect_stroke(
            thumb,
            inner_corner,
            Stroke::new(
                stroke::BORDER,
                if p.allows_translucency() {
                    p.hairline
                } else {
                    p.accent
                },
            ),
            StrokeKind::Inside,
        );
    }

    for (i, (((opt, g), r), resp)) in options
        .iter()
        .zip(galleys)
        .zip(&rects)
        .zip(&resps)
        .enumerate()
    {
        let is_sel = *selected == i;
        let hover = hover_t(resp, &theme);
        let fg = if is_sel {
            p.text_primary
        } else {
            mix(p.text_muted, p.text_primary, hover)
        };
        let icon_w = if opt.icon.is_some() {
            icon_size::INLINE
        } else {
            0.0
        };
        let gap = if opt.icon.is_some() && g.is_some() {
            icon_gap
        } else {
            0.0
        };
        let content = icon_w + gap + g.as_ref().map_or(0.0, |g| g.size().x);
        let mut cx = r.center().x - content / 2.0;
        if let Some(icon) = opt.icon {
            painter.text(
                pos2(cx + icon_w / 2.0, r.center().y),
                Align2::CENTER_CENTER,
                icon.text(is_sel),
                icons::font(icon_size::INLINE),
                fg,
            );
            cx += icon_w + gap;
        }
        if let Some(g) = g {
            painter.galley(pos2(cx, r.center().y - g.size().y / 2.0), g, fg);
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
