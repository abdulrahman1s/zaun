//! Chips: `r.pill`, height 24/28; filter (toggle), tag (dot + label + optional remove),
//! removable (an active filter: label + remove), status (non-interactive) (§7.4).

use egui::{Align2, CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, WidgetText, pos2, vec2};

use super::{focus_ring, hover_t};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};

/// What a chip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipKind {
    /// A toggle in a filter row.
    Filter {
        /// Active.
        selected: bool,
    },
    /// A tag with its hue index; `removable` adds a ✕ (activated by click or Delete).
    Tag {
        /// Tag hue `tag.N`.
        color: u8,
        /// Show a remove affordance.
        removable: bool,
    },
    /// An active filter that is not a tag: the label and a ✕ (click or Delete removes it).
    Removable,
    /// Display only.
    Status,
}

/// The result of interacting with a chip.
#[derive(Debug)]
pub struct ChipResponse {
    /// The chip's response.
    pub response: Response,
    /// The chip's remove control was activated.
    pub removed: bool,
}

/// A chip. `small` uses the 24 px height.
pub fn chip(
    ui: &mut Ui,
    kind: ChipKind,
    label: &str,
    icon: Option<Icon>,
    small: bool,
) -> ChipResponse {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let h = if small {
        control::CHIP_SM
    } else {
        control::CHIP_MD
    };
    let font = Type::Caption.font_id();
    let g = WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        font,
    );
    let dot = matches!(kind, ChipKind::Tag { .. });
    let removable = matches!(
        kind,
        ChipKind::Removable
            | ChipKind::Tag {
                removable: true,
                ..
            }
    );
    let lead = if dot {
        space::S3
    } else if icon.is_some() {
        14.0 + space::S1
    } else {
        0.0
    };
    let trail = if removable { 14.0 + space::S1 } else { 0.0 };
    let pad = space::S2 + 2.0;
    let w = pad * 2.0 + lead + g.size().x + trail;
    let sense = if kind == ChipKind::Status {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), sense);
    let mut removed = false;
    match kind {
        ChipKind::Filter { selected } => a11y::toggle(&resp, label, selected),
        ChipKind::Removable
        | ChipKind::Tag {
            removable: true, ..
        } => {
            a11y::button(&resp, &crate::strings::library::remove_chip(label));
            if resp.clicked()
                || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Delete)))
            {
                removed = true;
            }
        }
        ChipKind::Tag { .. } => a11y::button(&resp, label),
        ChipKind::Status => a11y::labelled(ui.ctx(), resp.id, egui::Role::Label, label),
    }
    if ui.is_rect_visible(rect) {
        let hover = if kind == ChipKind::Status {
            0.0
        } else {
            hover_t(&resp, &theme)
        };
        let corner = CornerRadius::same(radius::PILL);
        let painter = ui.painter();
        let quiet = mix(p.fill_hover, p.fill_selected, hover);
        let (fill, border, fg) = match kind {
            ChipKind::Filter { selected: true } => (p.accent_subtle, p.accent, p.text_primary),
            ChipKind::Filter { selected: false } => (quiet, p.hairline, p.text_secondary),
            ChipKind::Tag { .. } | ChipKind::Removable => (quiet, p.hairline, p.text_primary),
            ChipKind::Status => (p.fill_hover, p.hairline, p.text_secondary),
        };
        painter.rect_filled(rect, corner, fill);
        painter.rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, border),
            StrokeKind::Inside,
        );
        let mut x = rect.left() + pad;
        let cy = rect.center().y;
        if let ChipKind::Tag { color, .. } = kind {
            painter.circle_filled(pos2(x + 3.5, cy), 3.5, p.tag(color));
            x += lead;
        } else if let Some(icon) = icon {
            painter.text(
                pos2(x + 7.0, cy),
                Align2::CENTER_CENTER,
                icon.text(false),
                icons::font(14.0),
                fg,
            );
            x += lead;
        }
        painter.galley(
            pos2(x, cy - g.size().y / 2.0),
            g.clone(),
            mix(fg, p.text_primary, hover),
        );
        if removable {
            painter.text(
                pos2(rect.right() - pad - 6.0, cy),
                Align2::CENTER_CENTER,
                icons::CLOSE.text(false),
                icons::font(12.0),
                mix(p.text_muted, p.text_primary, hover),
            );
        }
        if resp.has_focus() {
            focus_ring(painter, rect, corner, &theme);
        }
    }
    ChipResponse {
        response: resp,
        removed,
    }
}
