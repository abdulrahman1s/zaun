//! Select: a button showing the current value and a caret that opens a menu of the choices.
//!
//! The design has two: the select of a setting row (28 high, `o1`, radius 8, padding
//! 0 7 0 10, the value in primary text and a 13 pt caret in quiet text; `o2` under the
//! pointer and while open) and the small one under a setting's label (22 high, no fill,
//! 12 pt quiet text; `o1` and primary text under the pointer).

use egui::{
    CornerRadius, FontId, Popup, PopupCloseBehavior, Response, Sense, Shape, Ui, pos2, vec2,
};

use super::{context_menu, contrast_outline, focus_ring, glyph_shape, label_galley, pointer_t};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius};
use crate::theme::{self, Icon, Type, icons};

/// Which select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A setting row's select.
    Boxed,
    /// No fill until pointed at, 28 high.
    Ghost,
    /// The small select under a label.
    Inline,
}

/// A select showing `options[*selected]`. `prefix` is drawn before the value in quiet text
/// ("Sort: Name"); `label` is the accessible name; `ghost` leaves out the fill until it is
/// pointed at. Returns the button's response; `changed()` when a new option was picked.
pub fn select(
    ui: &mut Ui,
    label: &str,
    prefix: Option<&str>,
    icon: Option<Icon>,
    selected: &mut usize,
    options: &[&str],
    ghost: bool,
) -> Response {
    let kind = if ghost { Kind::Ghost } else { Kind::Boxed };
    select_impl(ui, label, prefix, icon, selected, options, kind)
}

/// The small select under a setting's label ("All controllers"): 22 high, no fill, 12 pt
/// quiet text. Returns its response; `changed()` when a new option was picked.
pub fn select_inline(ui: &mut Ui, label: &str, selected: &mut usize, options: &[&str]) -> Response {
    select_impl(ui, label, None, None, selected, options, Kind::Inline)
}

fn select_impl(
    ui: &mut Ui,
    label: &str,
    prefix: Option<&str>,
    icon: Option<Icon>,
    selected: &mut usize,
    options: &[&str],
    kind: Kind,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let value = options.get(*selected).copied().unwrap_or_default();
    let (font, h, [pad_l, pad_r]): (FontId, f32, [f32; 2]) = match kind {
        Kind::Inline => (
            Type::Caption.font_id(),
            control::SELECT_INLINE,
            control::SELECT_INLINE_PAD,
        ),
        _ => (Type::Body.font_id(), control::SM, control::SELECT_PAD),
    };
    let gap = layout::WORD_GAP;
    let painter = ui.painter().clone();
    let line = if kind == Kind::Inline {
        Type::Caption.line_height()
    } else {
        Type::Body.line_height()
    };
    let pre = prefix.map(|t| label_galley(&painter, &format!("{t}: "), font.clone(), line));
    let g = label_galley(&painter, value, font, line);
    let lead = if icon.is_some() {
        icon_size::INLINE + gap
    } else {
        0.0
    };
    let pre_w = pre.as_ref().map_or(0.0, |g| g.size().x);
    let w = pad_l + lead + pre_w + g.size().x + gap + icon_size::SMALL + pad_r;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&resp));
    let name = format!("{label}: {value}");
    let enabled = resp.enabled();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, &name));
    if ui.is_rect_visible(rect) {
        let hover = if enabled {
            pointer_t(&resp, &theme).max(f32::from(u8::from(open)))
        } else {
            0.0
        };
        let corner = CornerRadius::same(if kind == Kind::Inline {
            radius::WORD
        } else {
            radius::MD
        });
        let (fill, text, caret) = match kind {
            Kind::Boxed => (mix(p.o1, p.o2, hover), p.t1, p.t3),
            Kind::Ghost => (fade(p.o2, hover), mix(p.t2, p.t1, hover), p.t3),
            Kind::Inline => {
                let t = mix(p.t3, p.t1, hover);
                (fade(p.o1, hover), t, t)
            }
        };
        let mut shapes = vec![Shape::rect_filled(rect, corner, fill)];
        if kind != Kind::Inline
            && let Some(o) = contrast_outline(rect, corner, &theme)
        {
            shapes.push(o);
        }
        let mut x = rect.left() + pad_l;
        let cy = rect.center().y;
        if let Some(icon) = icon {
            shapes.push(glyph_shape(
                &painter,
                pos2(x + icon_size::INLINE / 2.0, cy),
                icon,
                false,
                icon_size::INLINE,
                text,
            ));
            x += lead;
        }
        if let Some(pg) = pre {
            shapes.push(Shape::galley(pos2(x, cy - pg.size().y / 2.0), pg, p.t3));
            x += pre_w;
        }
        let gw = g.size().x;
        shapes.push(Shape::galley(pos2(x, cy - g.size().y / 2.0), g, text));
        x += gw + gap;
        shapes.push(glyph_shape(
            &painter,
            pos2(x + icon_size::SMALL / 2.0, cy),
            icons::CARET_DOWN,
            false,
            icon_size::SMALL,
            caret,
        ));
        painter.extend(shapes);
        if resp.has_focus() {
            focus_ring(&painter, rect, corner, &theme);
        }
    }
    let before = *selected;
    context_menu::popup(
        Popup::menu(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .width(rect.width().max(layout::MENU_W[0])),
        |ui| {
            for (i, opt) in options.iter().enumerate() {
                let r = context_menu::option_item(ui, opt, i == *selected);
                if r.clicked() {
                    *selected = i;
                }
            }
        },
    );
    a11y::describe(&resp, label);
    if *selected != before {
        resp.mark_changed();
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_are_28_and_22_high() {
        let (a, b) = crate::widgets::test_frame(&Default::default(), |ui| {
            let mut i = 0;
            let a = select(
                ui,
                "Version",
                None,
                None,
                &mut i,
                &["UMU-Proton 10.0-4"],
                false,
            );
            let b = select_inline(ui, "Which controllers", &mut i, &["All controllers"]);
            (a.rect, b.rect)
        });
        assert_eq!(a.height(), control::SM);
        assert_eq!(b.height(), control::SELECT_INLINE);
    }
}
