//! Segmented control: 2–4 exclusive options with optional icons; arrow keys move the
//! selection (§7.4). Exposed as a radio group.

use egui::accesskit::Role;
use egui::{
    Align2, CornerRadius, Key, Response, Sense, Stroke, StrokeKind, Ui, WidgetText, pos2, vec2,
};

use super::{focus_ring, hover_t};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};

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
    let h = control::SM + space::S1;
    let pad = if icon_only { space::S2 } else { space::S3 };
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
                space::S1 + 2.0
            } else {
                0.0
            };
            (pad * 2.0 + icon_w + gap + g.as_ref().map_or(0.0, |g| g.size().x))
                .max(control::MIN_TARGET)
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + space::S0 * 2.0;
    let (outer, mut group) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    a11y::labelled(ui.ctx(), group.id, Role::RadioGroup, group_label);

    let corner = CornerRadius::same(radius::MD);
    let inner_corner = CornerRadius::same(radius::SM + 1);
    let painter = ui.painter().clone();
    painter.rect_filled(outer, corner, p.bg_sunken);
    painter.rect_stroke(
        outer,
        corner,
        Stroke::new(stroke::BORDER, p.border_subtle),
        StrokeKind::Inside,
    );

    let before = *selected;
    let mut x = outer.left() + space::S0;
    let mut any_focus = false;
    for (i, ((opt, g), w)) in options.iter().zip(galleys).zip(&widths).enumerate() {
        let r = egui::Rect::from_min_size(
            pos2(x, outer.top() + space::S0),
            vec2(*w, h - space::S0 * 2.0),
        );
        x += w;
        let resp = ui.interact(r, group.id.with(i), Sense::click());
        super::own_arrow_keys(&resp);
        a11y::option(&resp, Role::RadioButton, opt.label, *selected == i);
        if icon_only {
            let _ = resp.clone().on_hover_text(opt.label);
        }
        if resp.clicked() {
            *selected = i;
        }
        if resp.has_focus() {
            any_focus = true;
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
        let is_sel = *selected == i;
        let hover = hover_t(&resp, &theme);
        if is_sel {
            painter.rect_filled(r, inner_corner, p.bg_raised);
            painter.rect_stroke(
                r,
                inner_corner,
                Stroke::new(
                    stroke::BORDER,
                    if p.allows_translucency() {
                        p.border_strong
                    } else {
                        p.accent
                    },
                ),
                StrokeKind::Inside,
            );
        } else if hover > 0.0 {
            painter.rect_filled(r, inner_corner, fade(p.bg_raised, 0.6 * hover));
        }
        let fg = if is_sel {
            p.text_primary
        } else {
            mix(p.text_muted, p.text_secondary, hover)
        };
        let icon_w = if opt.icon.is_some() {
            icon_size::INLINE
        } else {
            0.0
        };
        let gap = if opt.icon.is_some() && g.is_some() {
            space::S1 + 2.0
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
                if is_sel { p.accent } else { fg },
            );
            cx += icon_w + gap;
        }
        if let Some(g) = g {
            painter.galley(pos2(cx, r.center().y - g.size().y / 2.0), g, fg);
        }
        if resp.has_focus() {
            focus_ring(&painter, r, inner_corner, &theme);
        }
    }
    let _ = any_focus;
    if *selected != before {
        group.mark_changed();
    }
    group
}
