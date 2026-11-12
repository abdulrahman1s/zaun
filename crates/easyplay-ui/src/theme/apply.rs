//! Translate tokens into `egui::Style` / `Visuals` (UI_DESIGN_SYSTEM §8).

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{CornerRadius, Margin, Shadow, Stroke, Style, Vec2, Visuals};

use super::Theme;
use super::palette::{Palette, ThemeKind, mix};
use super::tokens::{control, radius, space, stroke, timing};
use super::typography;

fn widget(fill: egui::Color32, border: egui::Color32, fg: egui::Color32, bw: f32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(bw, border),
        corner_radius: CornerRadius::same(radius::MD),
        fg_stroke: Stroke::new(1.5, fg),
        expansion: 0.0,
    }
}

/// egui visuals for a palette.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.kind.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let hc = p.kind == ThemeKind::HighContrast;
    let bw = stroke::BORDER;
    let hover_fill = if p.kind.is_dark() {
        mix(p.bg_raised, p.text_primary, 0.06)
    } else {
        mix(p.bg_raised, p.text_primary, 0.04)
    };
    let pressed_fill = if p.kind.is_dark() {
        mix(p.bg_raised, p.text_primary, 0.10)
    } else {
        mix(p.bg_raised, p.text_primary, 0.08)
    };

    v.override_text_color = None;
    v.weak_text_color = Some(p.text_muted);
    v.widgets = Widgets {
        noninteractive: widget(p.bg_surface, p.border_subtle, p.text_primary, bw),
        inactive: widget(p.bg_raised, p.border_strong, p.text_primary, bw),
        hovered: widget(
            hover_fill,
            if hc { p.focus } else { p.border_strong },
            p.text_primary,
            bw,
        ),
        active: widget(pressed_fill, p.accent, p.text_primary, bw),
        open: widget(p.bg_raised, p.border_strong, p.text_primary, bw),
    };
    v.selection = Selection {
        bg_fill: if hc { p.accent } else { p.accent_subtle },
        stroke: Stroke::new(bw, if hc { p.text_on_accent } else { p.accent }),
    };
    v.hyperlink_color = p.accent;
    v.faint_bg_color = p.bg_surface;
    v.extreme_bg_color = p.bg_sunken;
    v.text_edit_bg_color = Some(p.bg_raised);
    v.code_bg_color = p.bg_sunken;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.window_corner_radius = CornerRadius::same(radius::LG);
    v.window_fill = p.bg_overlay;
    v.window_stroke = Stroke::new(bw, if hc { p.border_strong } else { p.border_subtle });
    v.window_shadow = if p.allows_translucency() {
        Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: p.shadow,
        }
    } else {
        Shadow::NONE
    };
    v.popup_shadow = if p.allows_translucency() {
        Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: p.shadow,
        }
    } else {
        Shadow::NONE
    };
    v.menu_corner_radius = CornerRadius::same(radius::SM);
    v.panel_fill = p.bg_base;
    v.window_highlight_topmost = false;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.image_loading_spinners = false;
    v.disabled_alpha = super::tokens::alpha::DISABLED;
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v
}

/// The full egui style for a theme.
pub fn style(theme: &Theme) -> Style {
    let mut s = Style {
        visuals: visuals(&theme.palette),
        text_styles: typography::text_styles(),
        animation_time: theme.motion.animation_time(),
        ..Style::default()
    };
    s.spacing.item_spacing = Vec2::splat(space::S2);
    s.spacing.button_padding = Vec2::new(space::S3, space::S1 + 2.0);
    s.spacing.interact_size.y = control::MD;
    s.spacing.menu_margin = Margin::same(space::S1 as i8);
    s.spacing.window_margin = Margin::same(space::S6 as i8);
    s.spacing.icon_spacing = space::S2;
    s.interaction.selectable_labels = false;
    s.interaction.tooltip_delay = timing::TOOLTIP_DELAY;
    s.interaction.show_tooltips_only_when_still = false;
    s.url_in_tooltip = false;
    s
}
