//! Translate tokens into `egui::Style` / `Visuals` (UI_DESIGN_SYSTEM §8).

/// Points one wheel notch scrolls (a browser's 100 px).
pub const LINE_SCROLL: f32 = 100.0;
/// The fastest a smooth scroll moves, in points a second.
pub const SCROLL_SPEED: f32 = 4000.0;

use egui::style::{ScrollStyle, Selection, WidgetVisuals, Widgets};
use egui::{CornerRadius, Margin, Shadow, Stroke, Style, Vec2, Visuals};

use super::Theme;
use super::palette::Palette;
use super::tokens::{control, layout, radius, space, stroke, timing};
use super::typography;

fn widget(fill: egui::Color32, border: egui::Color32, fg: egui::Color32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(stroke::BORDER, border),
        corner_radius: CornerRadius::same(radius::MD),
        fg_stroke: Stroke::new(1.5, fg),
        expansion: 0.0,
    }
}

/// The shadow under menus, popovers and toasts.
pub fn popup_shadow(p: &Palette) -> Shadow {
    p.shade_pop.shadow()
}

/// The shadow under dialogs.
pub fn overlay_shadow(p: &Palette) -> Shadow {
    p.shade_dialog.shadow()
}

/// egui visuals for a palette.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.kind.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let outline = if p.outlined() {
        p.line
    } else {
        egui::Color32::TRANSPARENT
    };
    v.override_text_color = None;
    v.weak_text_color = Some(p.t3);
    v.widgets = Widgets {
        noninteractive: widget(egui::Color32::TRANSPARENT, p.line, p.t1),
        inactive: widget(p.o1, outline, p.t1),
        hovered: widget(p.o2, outline, p.t1),
        active: widget(p.o3, outline, p.t1),
        open: widget(p.o2, outline, p.t1),
    };
    v.selection = Selection {
        bg_fill: p.tint,
        stroke: Stroke::new(stroke::BORDER, p.accent_mark),
    };
    v.hyperlink_color = p.accent_text;
    v.faint_bg_color = p.o1;
    v.extreme_bg_color = p.well;
    v.text_edit_bg_color = Some(p.well);
    v.code_bg_color = p.o1;
    v.warn_fg_color = p.amber;
    v.error_fg_color = p.coral;
    v.window_corner_radius = CornerRadius::same(radius::LG);
    v.window_fill = p.raised;
    // Menus, popovers and dialogs take their outline from here.
    v.window_stroke = Stroke::new(stroke::BORDER, p.line);
    v.window_shadow = overlay_shadow(p);
    v.popup_shadow = popup_shadow(p);
    v.menu_corner_radius = CornerRadius::same(radius::POP);
    v.panel_fill = p.bg;
    v.window_highlight_topmost = false;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.image_loading_spinners = false;
    v.disabled_alpha = super::tokens::alpha::DISABLED;
    v.text_cursor.stroke = Stroke::new(1.0, p.t1);
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
    s.spacing.button_padding = Vec2::new(control::BUTTON_PAD, space::S1 + 2.0);
    s.spacing.interact_size.y = control::BUTTON;
    s.spacing.menu_margin = Margin::same(layout::MENU_PAD as i8);
    s.spacing.window_margin = Margin::same(layout::SHEET_PAD as i8);
    s.spacing.icon_spacing = control::BUTTON_GAP;
    s.spacing.tooltip_width = layout::TIP_MAX_W;
    // Thin bars that take no room from the page (so a page never narrows when one appears),
    // always visible while there is more to scroll: a quiet thumb in the text colour on no
    // track, like the design's `scrollbar-width: thin; scrollbar-color: var(--o3) transparent`.
    s.spacing.scroll = ScrollStyle {
        floating: true,
        bar_width: 8.0,
        floating_width: 4.0,
        floating_allocated_width: 0.0,
        bar_outer_margin: 2.0,
        handle_min_length: 32.0,
        foreground_color: true,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.0,
        interact_background_opacity: 0.0,
        dormant_handle_opacity: 0.32,
        active_handle_opacity: 0.5,
        interact_handle_opacity: 0.75,
        ..ScrollStyle::thin()
    };
    // A page ends at the edge it scrolls under: nothing fades there.
    s.spacing.scroll.fade.strength = 0.0;
    // A spun wheel keeps up: egui's 1000 points a second lagged behind quick notches.
    s.scroll_animation =
        egui::style::ScrollAnimation::new(SCROLL_SPEED, egui::Rangef::new(0.06, 0.18));
    s.interaction.selectable_labels = false;
    s.interaction.tooltip_delay = timing::TOOLTIP_DELAY;
    s.interaction.show_tooltips_only_when_still = false;
    s.url_in_tooltip = false;
    s
}
