//! Select: a field-like button opening a menu (§7.4). Searchable lists above eight items
//! arrive with the screens that need them.

use egui::{
    Align2, CornerRadius, Popup, PopupCloseBehavior, Response, Sense, Stroke, StrokeKind, Ui,
    WidgetText, pos2, vec2,
};

use super::{context_menu, focus_ring, hover_t};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};

/// A select showing `options[*selected]`. `prefix` is drawn before the value in muted text
/// ("Sort: Name"); `label` is the accessible name. Returns the button response; `changed()`
/// when a new option was picked.
pub fn select(
    ui: &mut Ui,
    label: &str,
    prefix: Option<&str>,
    icon: Option<Icon>,
    selected: &mut usize,
    options: &[&str],
    ghost: bool,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let value = options.get(*selected).copied().unwrap_or_default();
    let font = Type::BodyStrong.font_id();
    let pre = prefix.map(|t| {
        WidgetText::from(format!("{t}: ")).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            Type::Body.font_id(),
        )
    });
    let g = WidgetText::from(value).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        font,
    );
    let lead = if icon.is_some() {
        icon_size::INLINE + space::S2
    } else {
        0.0
    };
    let pre_w = pre.as_ref().map_or(0.0, |g| g.size().x);
    let w = space::S3 * 2.0 + lead + pre_w + g.size().x + space::S2 + 12.0;
    let h = if ghost {
        control::SM + space::S1
    } else {
        control::MD
    };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&resp));
    let name = format!("{label}: {value}");
    let enabled = resp.enabled();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, &name));
    if ui.is_rect_visible(rect) {
        let hover = hover_t(&resp, &theme);
        let corner = CornerRadius::same(radius::MD);
        let painter = ui.painter();
        if ghost {
            if hover > 0.0 || open {
                painter.rect_filled(
                    rect,
                    corner,
                    egui::Color32::TRANSPARENT
                        .lerp_to_gamma(p.bg_raised, hover.max(f32::from(u8::from(open)))),
                );
            }
        } else {
            painter.rect_filled(rect, corner, p.bg_raised);
            painter.rect_stroke(
                rect,
                corner,
                Stroke::new(
                    stroke::BORDER,
                    if open { p.accent } else { p.border_strong },
                ),
                StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + space::S3;
        let cy = rect.center().y;
        let fg = mix(p.text_secondary, p.text_primary, hover);
        if let Some(icon) = icon {
            painter.text(
                pos2(x, cy),
                Align2::LEFT_CENTER,
                icon.text(false),
                icons::font(icon_size::INLINE),
                fg,
            );
            x += lead;
        }
        if let Some(pg) = pre {
            painter.galley(pos2(x, cy - pg.size().y / 2.0), pg, p.text_muted);
            x += pre_w;
        }
        painter.galley(
            pos2(x, cy - g.size().y / 2.0),
            g,
            if ghost { fg } else { p.text_primary },
        );
        painter.text(
            pos2(rect.right() - space::S3, cy),
            Align2::RIGHT_CENTER,
            icons::CARET_DOWN.text(false),
            icons::font(12.0),
            p.text_muted,
        );
        if resp.has_focus() {
            focus_ring(painter, rect, corner, &theme);
        }
    }
    let before = *selected;
    Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .width(rect.width().max(160.0))
        .show(|ui| {
            for (i, opt) in options.iter().enumerate() {
                let r = context_menu::item(ui, None, opt, None, i == *selected);
                if r.clicked() {
                    *selected = i;
                }
            }
        });
    a11y::describe(&resp, label);
    if *selected != before {
        resp.mark_changed();
    }
    resp
}
