//! The shield badge: a four-state glyph stating a game's protection (UI_DESIGN_SYSTEM §1,
//! §7.3). Neutral in colour; red only when something needs attention.

use easyplay_core::model::Preset;
use egui::accesskit::Role;
use egui::{
    Align2, Color32, CornerRadius, Painter, Pos2, Response, Sense, Stroke, StrokeKind, Ui, vec2,
};

use crate::a11y;
use crate::strings;
use crate::theme::palette::black;
use crate::theme::tokens::{alpha, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};

/// The glyph for a preset (or the alert shield).
pub fn glyph(preset: Preset, attention: bool) -> Icon {
    if attention {
        return icons::SHIELD_ALERT;
    }
    match preset {
        Preset::Offline => icons::SHIELD_OFFLINE,
        Preset::Online => icons::SHIELD_ONLINE,
        Preset::Standard => icons::SHIELD_STANDARD,
        Preset::Custom { .. } => icons::SHIELD_CUSTOM,
    }
}

/// Paint the glyph centred at `center` with `size` px and `color` (used on cards and rows).
pub fn paint(
    painter: &Painter,
    center: Pos2,
    size: f32,
    preset: Preset,
    attention: bool,
    color: Color32,
) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        glyph(preset, attention).text(false),
        icons::font(size),
        color,
    );
}

/// Tooltip text: the preset's name and one-line description.
pub fn tooltip_text(preset: Preset, attention: bool) -> String {
    if attention {
        format!(
            "{}. {}",
            strings::shield::ATTENTION,
            strings::shield::accessible_name(preset)
        )
    } else {
        strings::shield::accessible_name(preset)
    }
}

/// A standalone badge (16/20/24 px) with tooltip and accessible name "Protection: Offline…".
pub fn badge(ui: &mut Ui, preset: Preset, attention: bool, size: f32) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let color = if attention {
        p.danger
    } else {
        p.text_secondary
    };
    paint(ui.painter(), rect.center(), size, preset, attention, color);
    let text = tooltip_text(preset, attention);
    a11y::labelled(ui.ctx(), resp.id, Role::Image, &text);
    resp.on_hover_text(text)
}

/// The access summary card of the game page (§7.3): preset name, up to five access
/// statements as icon + sentence rows, then a link to everything. Returns the link's response.
pub fn summary_card(
    ui: &mut Ui,
    preset: Preset,
    statements: &[(Icon, &str)],
    see_all: &str,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    egui::Frame::NONE
        .fill(p.bg_surface)
        .stroke(Stroke::new(stroke::BORDER, p.border_subtle))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(space::S4)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.horizontal(|ui| {
                badge(ui, preset, false, icon_size::NAV);
                ui.label(
                    Type::Heading
                        .rich(strings::shield::preset_name(preset))
                        .color(p.text_primary),
                );
            });
            ui.label(
                Type::Caption
                    .rich(strings::shield::description(preset))
                    .color(p.text_secondary),
            );
            for (icon, sentence) in statements.iter().take(5) {
                ui.horizontal(|ui| {
                    ui.label(icons::icon(*icon, icon_size::INLINE).color(p.text_secondary));
                    ui.label(Type::Body.rich(*sentence).color(p.text_primary));
                });
            }
            ui.add(super::button::Button::new(see_all).variant(super::button::Variant::Link))
        })
        .inner
}

/// Paint the shield on a dark scrim disc (cards over artwork).
pub fn paint_on_scrim(
    painter: &Painter,
    center: Pos2,
    preset: Preset,
    attention: bool,
    theme: &theme::Theme,
) {
    let p = &theme.palette;
    painter.circle_filled(center, layout::SCRIM_DISC / 2.0, black(alpha::SCRIM));
    let color = if attention { p.danger } else { p.on_art };
    paint(
        painter,
        center,
        icon_size::BUTTON - 2.0,
        preset,
        attention,
        color,
    );
    if !p.allows_translucency() {
        painter.circle_stroke(
            center,
            layout::SCRIM_DISC / 2.0,
            Stroke::new(stroke::BORDER, p.border_strong),
        );
    }
    let _ = StrokeKind::Inside;
}
