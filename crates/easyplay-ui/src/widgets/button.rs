//! Buttons and the Play pill (UI_DESIGN_SYSTEM §7.1).

use egui::{
    Align2, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget,
    WidgetText, pos2, vec2,
};

use super::{focus_ring, hover_t, progress};
use crate::a11y;
use crate::strings;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{alpha, control, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Icon, Theme, Type, icons};

/// Button emphasis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variant {
    /// Accent fill: the one main action of a view.
    Primary,
    /// Raised fill with a strong border.
    #[default]
    Secondary,
    /// No fill until hovered.
    Ghost,
    /// Destructive: danger outline and text.
    Danger,
    /// Accent text.
    Link,
}

/// Button height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Size {
    /// 28 px.
    Sm,
    /// 36 px.
    #[default]
    Md,
    /// 44 px.
    Lg,
}

impl Size {
    /// Height in points.
    pub const fn height(self) -> f32 {
        match self {
            Size::Sm => control::SM,
            Size::Md => control::MD,
            Size::Lg => control::LG,
        }
    }
}

/// Colours for one visual state.
struct Look {
    fill: Color32,
    border: Option<Color32>,
    text: Color32,
}

fn look(variant: Variant, theme: &Theme, hover: f32, pressed: bool) -> Look {
    let p = &theme.palette;
    let towards = |c: Color32| {
        if p.kind.is_dark() {
            mix(c, Color32::WHITE, alpha::HOVER_LIGHTEN * hover)
        } else {
            mix(c, Color32::BLACK, alpha::HOVER_LIGHTEN * hover)
        }
    };
    match variant {
        Variant::Primary => Look {
            fill: if pressed {
                p.accent_pressed
            } else {
                mix(p.accent, p.accent_hover, hover)
            },
            border: (!p.allows_translucency()).then_some(p.border_strong),
            text: p.text_on_accent,
        },
        Variant::Secondary => Look {
            fill: if pressed {
                mix(p.bg_raised, p.text_primary, 0.10)
            } else {
                towards(p.bg_raised)
            },
            border: Some(p.border_strong),
            text: p.text_primary,
        },
        Variant::Ghost => Look {
            fill: fade(
                mix(
                    p.bg_raised,
                    p.text_primary,
                    if pressed { 0.08 } else { 0.0 },
                ),
                hover.max(f32::from(u8::from(pressed))),
            ),
            border: (!p.allows_translucency()).then_some(p.border_strong),
            text: mix(p.text_secondary, p.text_primary, hover),
        },
        Variant::Danger => Look {
            fill: fade(p.danger, if pressed { 0.24 } else { 0.14 * hover }),
            border: Some(p.danger),
            text: p.danger,
        },
        Variant::Link => Look {
            fill: Color32::TRANSPARENT,
            border: None,
            text: if hover > 0.5 {
                p.accent_hover
            } else {
                p.accent
            },
        },
    }
}

/// A text button with an optional leading icon.
#[must_use = "add it with `ui.add(button)`"]
pub struct Button<'a> {
    label: &'a str,
    icon: Option<Icon>,
    variant: Variant,
    size: Size,
    busy: bool,
    disabled_reason: Option<&'a str>,
    min_width: f32,
    accessible_name: Option<String>,
}

impl<'a> Button<'a> {
    /// A secondary, medium button.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            variant: Variant::Secondary,
            size: Size::Md,
            busy: false,
            disabled_reason: None,
            min_width: 0.0,
            accessible_name: None,
        }
    }

    /// Primary variant.
    pub fn primary(label: &'a str) -> Self {
        Self::new(label).variant(Variant::Primary)
    }

    /// Ghost variant.
    pub fn ghost(label: &'a str) -> Self {
        Self::new(label).variant(Variant::Ghost)
    }

    /// Set the variant.
    pub fn variant(mut self, v: Variant) -> Self {
        self.variant = v;
        self
    }

    /// Set the size.
    pub fn size(mut self, s: Size) -> Self {
        self.size = s;
        self
    }

    /// Leading icon.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Busy: a spinner replaces the leading icon; the label and width stay.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Disable with a reason shown as the tooltip (USER_EXPERIENCE §4).
    pub fn disabled(mut self, reason: Option<&'a str>) -> Self {
        self.disabled_reason = reason;
        self
    }

    /// Minimum width.
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    /// Accessible name when it should differ from the label ("Play Hollow Knight").
    pub fn accessible_name(mut self, name: impl Into<String>) -> Self {
        self.accessible_name = Some(name.into());
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = theme::theme(ui.ctx());
        let enabled = self.disabled_reason.is_none() && ui.is_enabled();
        let h = self.size.height();
        let pad = if self.variant == Variant::Link {
            0.0
        } else if self.size == Size::Sm {
            space::S3
        } else {
            space::S4
        };
        let font = Type::BodyStrong.font_id();
        let galley = WidgetText::from(self.label).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            font,
        );
        let has_lead = self.icon.is_some() || self.busy;
        let lead = if has_lead {
            icon_size::INLINE + space::S2
        } else {
            0.0
        };
        let w = (pad * 2.0 + lead + galley.size().x).max(self.min_width);
        let (rect, resp) = ui
            .add_enabled_ui(enabled, |ui| {
                ui.allocate_exact_size(vec2(w, h), Sense::click())
            })
            .inner;
        let name = self
            .accessible_name
            .clone()
            .unwrap_or_else(|| self.label.to_owned());
        a11y::button(&resp, &name);

        if ui.is_rect_visible(rect) {
            let hover = if enabled { hover_t(&resp, &theme) } else { 0.0 };
            let pressed = enabled && resp.is_pointer_button_down_on();
            let mut l = look(self.variant, &theme, hover, pressed);
            if !enabled {
                l.fill = fade(l.fill, alpha::DISABLED);
                l.text = fade(l.text, alpha::DISABLED);
                l.border = l.border.map(|b| fade(b, alpha::DISABLED));
            }
            let corner = CornerRadius::same(radius::MD);
            let painter = ui.painter();
            painter.rect_filled(rect, corner, l.fill);
            if let Some(b) = l.border {
                painter.rect_stroke(
                    rect,
                    corner,
                    Stroke::new(stroke::BORDER, b),
                    StrokeKind::Inside,
                );
            }
            let content_w = lead + galley.size().x;
            let mut x = rect.center().x - content_w / 2.0;
            if self.busy {
                let c = pos2(x + icon_size::INLINE / 2.0, rect.center().y);
                progress::spinner(ui, c, icon_size::INLINE / 2.0 - 1.0, l.text);
                x += lead;
            } else if let Some(icon) = self.icon {
                painter.text(
                    pos2(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    icon.text(false),
                    icons::font(icon_size::INLINE),
                    l.text,
                );
                x += lead;
            }
            let text_pos = pos2(x, rect.center().y - galley.size().y / 2.0);
            if self.variant == Variant::Link && hover > 0.5 {
                let y = text_pos.y + galley.size().y;
                painter.hline(x..=x + galley.size().x, y, Stroke::new(1.0, l.text));
            }
            painter.galley(text_pos, galley, l.text);
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        match self.disabled_reason {
            Some(reason) => resp.on_disabled_hover_text(reason),
            None => resp,
        }
    }
}

/// The state of the Play pill (§7.1 table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// Ready to start.
    Ready,
    /// Launch in progress; the phase text ("Preparing…").
    Launching(&'static str),
    /// Running.
    Running,
    /// A stop was requested.
    Stopping,
    /// The executable is missing.
    Unavailable,
    /// A compatibility layer must be downloaded first.
    NeedsRuntime,
    /// The entry is an installer.
    Installer,
}

impl PlayState {
    fn accent_filled(&self) -> bool {
        matches!(
            self,
            PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer
        )
    }

    fn busy(&self) -> bool {
        matches!(self, PlayState::Launching(_) | PlayState::Stopping)
    }

    fn label(&self) -> &'static str {
        match self {
            PlayState::Ready => strings::play::PLAY,
            PlayState::Launching(phase) => phase,
            PlayState::Running => strings::play::STOP,
            PlayState::Stopping => strings::play::STOPPING,
            PlayState::Unavailable => strings::play::LOCATE,
            PlayState::NeedsRuntime => strings::play::DOWNLOAD_AND_PLAY,
            PlayState::Installer => strings::play::RUN_INSTALLER,
        }
    }

    fn glyph(&self) -> Option<Icon> {
        match self {
            PlayState::Ready | PlayState::Installer => Some(icons::PLAY),
            PlayState::Running => Some(icons::STOP),
            PlayState::Unavailable => Some(icons::LINK_BREAK),
            PlayState::NeedsRuntime => Some(icons::DOWNLOAD),
            PlayState::Launching(_) | PlayState::Stopping => None,
        }
    }

    /// The accessible name for a game called `name`.
    pub fn accessible_name(&self, name: &str) -> String {
        match self {
            PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer => {
                strings::play::play_name(name)
            }
            PlayState::Running => strings::play::stop_name(name),
            PlayState::Stopping => strings::play::stop_name(name),
            PlayState::Launching(phase) => {
                strings::play::busy_name(name, &phase.trim_end_matches('…').to_lowercase())
            }
            PlayState::Unavailable => strings::play::locate_name(name),
        }
    }
}

/// Where the pill sits, which fixes its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillSize {
    /// Icon-only 44 px disc on cards.
    Card,
    /// `lg` with label in list rows.
    Row,
    /// `xl`, minimum 120 wide, on the game page.
    Page,
}

/// The Play pill: same place in every state, label and colour change (§7.1).
/// `opacity` lets the card fade it in on hover.
pub fn play_pill(
    ui: &mut Ui,
    state: &PlayState,
    game_name: &str,
    size: PillSize,
    opacity: f32,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (h, icon_px) = match size {
        PillSize::Card => (control::LG, icon_size::BUTTON),
        PillSize::Row => (control::LG, icon_size::INLINE),
        PillSize::Page => (control::XL, icon_size::BUTTON),
    };
    let label_font = match size {
        PillSize::Page => Type::Heading.font_id(),
        _ => Type::BodyStrong.font_id(),
    };
    let show_label = size != PillSize::Card;
    let galley = show_label.then(|| {
        WidgetText::from(state.label()).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            label_font,
        )
    });
    let gap = space::S2;
    let pad = if size == PillSize::Page {
        space::S6
    } else {
        space::S4
    };
    let lead = icon_px;
    let w = match &galley {
        Some(g) => {
            let w = pad * 2.0 + lead + gap + g.size().x;
            if size == PillSize::Page {
                w.max(control::PILL_MIN_W)
            } else {
                w
            }
        }
        None => h,
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let resp = resp.on_hover_text(state.accessible_name(game_name));
    a11y::button(&resp, &state.accessible_name(game_name));
    if !ui.is_rect_visible(rect) || opacity <= 0.0 {
        return resp;
    }
    let hover = hover_t(&resp, &theme);
    let pressed = resp.is_pointer_button_down_on();
    let corner = CornerRadius::same(radius::PILL);
    let (fill, fg, border) = if state.accent_filled() {
        let fill = if pressed {
            p.accent_pressed
        } else {
            mix(p.accent, p.accent_hover, hover)
        };
        (fill, p.text_on_accent, None)
    } else {
        let fill = mix(p.bg_raised, p.text_primary, 0.06 * hover);
        let border = if *state == PlayState::Running && hover > 0.0 {
            Some(mix(p.border_strong, p.danger, hover))
        } else {
            Some(p.border_strong)
        };
        (fill, p.text_primary, border)
    };
    let painter = ui.painter().clone().with_layer_id(ui.layer_id());
    let f = |c: Color32| fade(c, opacity);
    painter.rect_filled(rect, corner, f(fill));
    if let Some(b) = border {
        painter.rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, f(b)),
            StrokeKind::Inside,
        );
    }
    let content_w = galley.as_ref().map_or(lead, |g| lead + gap + g.size().x);
    let mut x = rect.center().x - content_w / 2.0;
    let cy = rect.center().y;
    if state.busy() {
        progress::spinner(ui, pos2(x + lead / 2.0, cy), lead / 2.0 - 1.5, f(p.accent));
    } else if let Some(glyph) = state.glyph() {
        // The play triangle's optical centre sits right of its box centre.
        let nudge = if glyph == icons::PLAY && !show_label {
            1.5
        } else {
            0.0
        };
        painter.text(
            pos2(x + lead / 2.0 + nudge, cy),
            Align2::CENTER_CENTER,
            glyph.text(true),
            icons::font(lead),
            f(fg),
        );
    }
    if *state == PlayState::Running {
        // Success dot before the label: running is dot *and* word.
        let dot_c = pos2(rect.left() + pad * 0.6, cy);
        if show_label {
            painter.circle_filled(dot_c, 3.5, f(p.success));
        }
    }
    x += lead + gap;
    if let Some(g) = galley {
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, f(fg));
    }
    if resp.has_focus() {
        focus_ring(&painter, rect, corner, &theme);
    }
    resp
}

/// Size of the pill for a given placement (for layout before painting).
pub fn pill_height(size: PillSize) -> f32 {
    match size {
        PillSize::Card => control::LG,
        PillSize::Row => control::MD,
        PillSize::Page => control::XL,
    }
}

/// Minimum hit target helper for small controls (ACCESSIBILITY §3).
pub fn min_target(v: Vec2) -> Vec2 {
    v.max(Vec2::splat(control::MIN_TARGET))
}
