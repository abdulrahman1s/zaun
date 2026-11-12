//! Banner: full width of the content area, 44 high, icon + text + one action + dismiss;
//! colours by severity at 12 % fill with a 2 px leading bar (§7.5).

use egui::{Align, CornerRadius, Layout, Rect, Sense, Ui, pos2, vec2};

use super::button::{Button, Size, Variant};
use super::icon_button::IconButton;
use crate::a11y;
use crate::strings;
use crate::theme::palette::fade;
use crate::theme::tokens::{alpha, icon as icon_size, layout, radius, space};
use crate::theme::{self, Type, icons};

/// Severity, which picks colour and icon (colour is never the only signal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Information.
    Info,
    /// Warning.
    Warning,
    /// Error / blocking.
    Danger,
}

/// What the user did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BannerResponse {
    /// The action button.
    pub action: bool,
    /// The dismiss button.
    pub dismissed: bool,
}

/// Show a banner. `dismissible` is false when the condition blocks correctness.
pub fn banner(
    ui: &mut Ui,
    severity: Severity,
    text: &str,
    action: Option<&str>,
    dismissible: bool,
) -> BannerResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let (color, icon) = match severity {
        Severity::Info => (p.info, icons::INFO),
        Severity::Warning => (p.warning, icons::WARNING),
        Severity::Danger => (p.danger, icons::ERROR),
    };
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), layout::BANNER_H), Sense::hover());
    a11y::labelled(ui.ctx(), resp.id, egui::Role::Status, text);
    let corner = CornerRadius::same(radius::SM);
    ui.painter().rect_filled(rect, corner, p.bg_base);
    ui.painter()
        .rect_filled(rect, corner, fade(color, alpha::BANNER_FILL));
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(2.0, rect.height())),
        CornerRadius {
            nw: radius::SM,
            sw: radius::SM,
            ne: 0,
            se: 0,
        },
        color,
    );
    let mut out = BannerResponse::default();
    let inner = rect.shrink2(vec2(space::S4, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::S3;
            ui.label(icons::icon(icon, icon_size::BUTTON).color(color));
            ui.label(Type::Body.rich(text).color(p.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if dismissible
                    && ui
                        .add(
                            IconButton::new(icons::CLOSE, strings::feedback::DISMISS)
                                .size(icon_size::NAV + 4.0)
                                .icon_size(icon_size::INLINE),
                        )
                        .clicked()
                {
                    out.dismissed = true;
                }
                if let Some(a) = action
                    && ui
                        .add(Button::new(a).variant(Variant::Ghost).size(Size::Sm))
                        .clicked()
                {
                    out.action = true;
                }
            });
        },
    );
    let _ = pos2;
    out
}
