//! Visual identity: tokens, palette, typography, icons, motion, and their application to
//! `egui::Style`. Screens and widgets never use literal colours, sizes or durations. See MODULES
//! §8 and UI_DESIGN_SYSTEM.
//!
//! Custom widgets read tokens from [`Theme`], stored in egui's data map (§8), so that roles
//! egui does not model (tag hues, shield states, scrims) are available. Both the dark and the
//! light `Theme` are stored; [`theme`] picks the one matching the context's current theme, so
//! following the system preference needs no extra code.

pub mod apply;
pub mod icons;
pub mod motion;
pub mod palette;
pub mod tokens;
pub mod typography;

use std::sync::Arc;

use egui::{Context, Id, ThemePreference};

pub use icons::Icon;
pub use motion::{Dur, Motion};
pub use palette::{Accent, Palette, ThemeKind};
pub use typography::Type;

/// `appearance.theme` (CONFIGURATION).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    /// Follow the desktop.
    #[default]
    System,
    /// Dark.
    Dark,
    /// Light.
    Light,
    /// High contrast.
    HighContrast,
}

impl ThemeChoice {
    /// Every choice.
    pub const ALL: [ThemeChoice; 4] = [
        ThemeChoice::System,
        ThemeChoice::Dark,
        ThemeChoice::Light,
        ThemeChoice::HighContrast,
    ];

    /// The configuration value.
    pub const fn as_str(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Light => "light",
            ThemeChoice::HighContrast => "high-contrast",
        }
    }

    /// Parse [`ThemeChoice::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// Cover size on the wall (`appearance.grid_density`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Density {
    /// 132 px covers.
    Compact,
    /// 164 px covers (default).
    #[default]
    Comfortable,
    /// 200 px covers.
    Large,
}

impl Density {
    /// Narrowest a cover is drawn on the wall.
    pub const fn cover_width(self) -> f32 {
        match self {
            Density::Compact => tokens::layout::COVER_COMPACT,
            Density::Comfortable => tokens::layout::COVER_COMFORTABLE,
            Density::Large => tokens::layout::COVER_LARGE,
        }
    }
}

/// The appearance preferences the theme depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    /// Theme.
    pub theme: ThemeChoice,
    /// Accent.
    pub accent: Accent,
    /// Reduced motion in effect (already resolved against the desktop setting).
    pub reduce_motion: bool,
    /// Multiplies the system scale factor (0.75 ..= 2.0).
    pub ui_scale: f32,
    /// Cover size.
    pub density: Density,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            accent: Accent::Tide,
            reduce_motion: false,
            ui_scale: 1.0,
            density: Density::Comfortable,
        }
    }
}

/// The desktop's colour scheme preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemScheme {
    /// Prefers dark.
    Dark,
    /// Prefers light.
    Light,
}

/// Appearance preferences of the desktop (XDG portal `org.freedesktop.appearance` and the
/// desktop keys it exposes), read by the binary off the UI thread (task A11Y-004).
#[derive(Clone, Debug, PartialEq)]
pub struct SystemPrefs {
    /// Colour scheme, when the desktop states one.
    pub scheme: Option<SystemScheme>,
    /// The desktop asks for high contrast.
    pub high_contrast: bool,
    /// The desktop asks for reduced motion (`None`: unknown).
    pub reduced_motion: Option<bool>,
    /// Desktop text scale (GNOME `text-scaling-factor`), multiplied into the UI scale.
    pub text_scale: f32,
    /// The desktop's locale ("ar_EG.UTF-8", "en_GB"...), when known.
    pub locale: Option<String>,
}

impl Default for SystemPrefs {
    fn default() -> Self {
        Self {
            scheme: None,
            high_contrast: false,
            reduced_motion: None,
            text_scale: 1.0,
            locale: None,
        }
    }
}

fn system_id() -> Id {
    Id::unique("easyplay.system_prefs")
}

/// Remember the desktop's preferences and follow its colour scheme where egui's own
/// detection (winit) has none.
pub fn set_system(ctx: &Context, system: SystemPrefs) {
    ctx.data_mut(|d| d.insert_temp(system_id(), system.clone()));
    if let Some(s) = system.scheme {
        ctx.options_mut(|o| {
            o.fallback_theme = match s {
                SystemScheme::Dark => egui::Theme::Dark,
                SystemScheme::Light => egui::Theme::Light,
            };
        });
    }
    ctx.request_repaint();
}

/// The desktop's preferences last reported (defaults when unknown).
pub fn system(ctx: &Context) -> SystemPrefs {
    ctx.data(|d| d.get_temp::<SystemPrefs>(system_id()))
        .unwrap_or_default()
}

/// The zoom factor for `appearance`: the UI scale times the desktop text scale.
pub fn zoom(ctx: &Context, appearance: &Appearance) -> f32 {
    (appearance.ui_scale * system(ctx).text_scale).clamp(0.5, 3.0)
}

/// Everything a widget needs to paint itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Colour roles.
    pub palette: Palette,
    /// Motion switch.
    pub motion: Motion,
    /// Grid density.
    pub density: Density,
}

impl Theme {
    /// A theme of `kind` from appearance preferences.
    pub fn new(kind: ThemeKind, appearance: &Appearance) -> Self {
        Self {
            palette: Palette::new(kind, appearance.accent),
            motion: Motion {
                reduced: appearance.reduce_motion,
            },
            density: appearance.density,
        }
    }

    /// Focus ring width for this theme.
    pub fn focus_width(&self) -> f32 {
        if self.palette.kind == ThemeKind::HighContrast {
            tokens::stroke::FOCUS_HC
        } else {
            tokens::stroke::FOCUS
        }
    }
}

#[derive(Clone)]
struct ThemeSet {
    dark: Arc<Theme>,
    light: Arc<Theme>,
    appearance: Appearance,
}

fn set_id() -> Id {
    Id::unique("easyplay.theme")
}

/// Install fonts (once) and apply `appearance`. Calling it again restyles everything live.
pub fn install(ctx: &Context, appearance: &Appearance) {
    ctx.set_fonts(typography::font_definitions());
    apply(ctx, appearance);
}

/// Apply `appearance` without touching fonts: the live theme/accent/motion switch.
pub fn apply(ctx: &Context, appearance: &Appearance) {
    let (dark_kind, light_kind, pref) = match appearance.theme {
        ThemeChoice::System => (ThemeKind::Dark, ThemeKind::Light, ThemePreference::System),
        ThemeChoice::Dark => (ThemeKind::Dark, ThemeKind::Light, ThemePreference::Dark),
        ThemeChoice::Light => (ThemeKind::Dark, ThemeKind::Light, ThemePreference::Light),
        ThemeChoice::HighContrast => (
            ThemeKind::HighContrast,
            ThemeKind::HighContrast,
            ThemePreference::Dark,
        ),
    };
    let set = ThemeSet {
        dark: Arc::new(Theme::new(dark_kind, appearance)),
        light: Arc::new(Theme::new(light_kind, appearance)),
        appearance: *appearance,
    };
    ctx.set_style_of(egui::Theme::Dark, apply::style(&set.dark));
    ctx.set_style_of(egui::Theme::Light, apply::style(&set.light));
    ctx.set_theme(pref);
    ctx.data_mut(|d| d.insert_temp(set_id(), set));
    ctx.request_repaint();
}

/// The active theme. Falls back to the default dark theme if [`install`] has not run.
pub fn theme(ctx: &Context) -> Arc<Theme> {
    let current = ctx.theme();
    ctx.data(|d| d.get_temp::<ThemeSet>(set_id()))
        .map(|s| match current {
            egui::Theme::Dark => s.dark,
            egui::Theme::Light => s.light,
        })
        .unwrap_or_else(|| Arc::new(Theme::new(ThemeKind::Dark, &Appearance::default())))
}

fn controller_id() -> Id {
    Id::unique("easyplay.controller_mode")
}

/// Enter or leave controller mode (USER_EXPERIENCE §5.3): widgets read it to widen the focus
/// ring and keep Play visible.
pub fn set_controller_mode(ctx: &Context, on: bool) {
    if controller_mode(ctx) != on {
        ctx.data_mut(|d| d.insert_temp(controller_id(), on));
        ctx.request_repaint();
    }
}

/// Whether controller mode is on.
pub fn controller_mode(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(controller_id()))
        .unwrap_or(false)
}

/// The appearance last applied.
pub fn appearance(ctx: &Context) -> Appearance {
    ctx.data(|d| d.get_temp::<ThemeSet>(set_id()))
        .map(|s| s.appearance)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_theme_and_accent_restyles_live() {
        let ctx = Context::default();
        let mut a = Appearance {
            theme: ThemeChoice::Dark,
            ..Appearance::default()
        };
        install(&ctx, &a);
        let dark = theme(&ctx);
        assert_eq!(dark.palette.kind, ThemeKind::Dark);
        assert_eq!(ctx.global_style().visuals.panel_fill, dark.palette.bg);

        a.theme = ThemeChoice::Light;
        a.accent = Accent::Iris;
        apply(&ctx, &a);
        let light = theme(&ctx);
        assert_eq!(light.palette.kind, ThemeKind::Light);
        assert_eq!(
            light.palette.fill,
            Palette::new(ThemeKind::Light, Accent::Iris).fill
        );
        assert_eq!(ctx.global_style().visuals.panel_fill, light.palette.bg);
        assert_eq!(
            ctx.global_style().visuals.hyperlink_color,
            light.palette.accent_text
        );

        a.theme = ThemeChoice::HighContrast;
        apply(&ctx, &a);
        assert_eq!(theme(&ctx).palette.kind, ThemeKind::HighContrast);
    }

    #[test]
    fn reduced_motion_zeroes_animation_time() {
        let ctx = Context::default();
        let a = Appearance {
            theme: ThemeChoice::Dark,
            reduce_motion: true,
            ..Appearance::default()
        };
        install(&ctx, &a);
        assert_eq!(ctx.global_style().animation_time, 0.0);
        assert!(theme(&ctx).motion.reduced);
    }

    #[test]
    fn choices_parse() {
        for c in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::parse(c.as_str()), Some(c));
        }
    }
}
