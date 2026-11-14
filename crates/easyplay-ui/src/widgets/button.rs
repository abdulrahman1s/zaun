//! Buttons and the Play pill (UI_DESIGN_SYSTEM §7.1).

use egui::{
    Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget,
    WidgetText, pos2, vec2,
};

use super::{focus_ring, hover_t, press_t, progress, tooltip};
use crate::a11y;
use crate::strings;
use crate::theme::motion::scaled;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{alpha, control, icon as icon_size, radius, space, stroke, travel};
use crate::theme::{self, Icon, Theme, Type, icons};

/// Button emphasis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variant {
    /// Accent fill: the one main action of a view.
    Primary,
    /// A quiet fill with a hairline: other actions.
    #[default]
    Secondary,
    /// Accent-tinted fill with accent text: a state that is on (active filters).
    Tonal,
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

/// What follows the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trailing {
    None,
    /// A caret: the button opens a menu.
    Caret,
    /// A count on an accent disc: how many filters are on.
    Badge(usize),
}

/// Colours for one visual state.
struct Look {
    fill: Color32,
    border: Option<Color32>,
    text: Color32,
}

fn look(variant: Variant, theme: &Theme, hover: f32, pressed: f32, open: bool) -> Look {
    let p = &theme.palette;
    // Quiet fills deepen from rest to hover to press and work over any plane.
    let held = hover.max(f32::from(u8::from(open)));
    let lift = |amount: f32| mix(p.fill_selected, p.text_primary, amount);
    match variant {
        Variant::Primary => Look {
            fill: mix(
                mix(p.accent, p.accent_hover, hover),
                p.accent_pressed,
                pressed,
            ),
            border: (!p.allows_translucency()).then_some(p.border_strong),
            text: p.text_on_accent,
        },
        Variant::Secondary => Look {
            fill: mix(
                mix(p.fill_selected, lift(alpha::HOVER_LIGHTEN), held),
                lift(alpha::HOVER_LIGHTEN * 2.0),
                pressed,
            ),
            border: Some(p.hairline),
            text: p.text_primary,
        },
        Variant::Tonal => Look {
            fill: mix(p.accent_subtle, fade(p.accent, 0.22), held.max(pressed)),
            border: (!p.allows_translucency()).then_some(p.accent),
            text: if p.allows_translucency() {
                p.accent
            } else {
                p.text_primary
            },
        },
        Variant::Ghost => Look {
            fill: mix(
                fade(p.fill_selected, held),
                lift(alpha::HOVER_LIGHTEN),
                pressed,
            ),
            border: (!p.allows_translucency()).then_some(p.border_strong),
            text: mix(p.text_secondary, p.text_primary, held),
        },
        Variant::Danger => Look {
            fill: fade(p.danger, 0.14 * hover + 0.10 * pressed),
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
    trailing: Trailing,
    icon_only: bool,
    open: bool,
    shortcut: Option<&'a str>,
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
            trailing: Trailing::None,
            icon_only: false,
            open: false,
            shortcut: None,
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

    /// A trailing caret: the button opens a menu.
    pub fn caret(mut self) -> Self {
        self.trailing = Trailing::Caret;
        self
    }

    /// A trailing count on an accent disc (nothing for zero).
    pub fn badge(mut self, count: usize) -> Self {
        if count > 0 {
            self.trailing = Trailing::Badge(count);
        }
        self
    }

    /// Hide the label and show it as the tooltip instead (narrow toolbars). The button
    /// needs an icon; the accessible name is unchanged.
    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only && self.icon.is_some();
        self
    }

    /// The button's menu is open: it keeps its hovered look.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Shortcut shown in the tooltip of an icon-only button.
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
}

/// What a button is made of, measured.
struct Measured {
    galley: Option<std::sync::Arc<egui::Galley>>,
    lead: f32,
    trail: f32,
    width: f32,
}

impl Button<'_> {
    fn measure(&self, ui: &Ui) -> Measured {
        let h = self.size.height();
        let pad = if self.variant == Variant::Link {
            0.0
        } else if self.size == Size::Sm || self.icon_only {
            space::S3 - 2.0
        } else {
            space::S3 + 2.0
        };
        let galley = (!self.icon_only).then(|| {
            WidgetText::from(self.label).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                Type::BodyStrong.font_id(),
            )
        });
        let label_w = galley.as_ref().map_or(0.0, |g| g.size().x);
        let gap = if galley.is_some() { space::S2 } else { 0.0 };
        let lead = if self.icon.is_some() || self.busy {
            icon_size::INLINE + gap
        } else {
            0.0
        };
        let trail = match self.trailing {
            Trailing::Caret if !self.icon_only => space::S1 + space::S3,
            Trailing::Badge(_) => space::S2 + control::BADGE,
            _ => 0.0,
        };
        let width = (pad * 2.0 + lead + label_w + trail)
            .max(self.min_width)
            .max(if self.icon_only { h } else { 0.0 });
        Measured {
            galley,
            lead,
            trail,
            width,
        }
    }

    /// The width the button will take (toolbars size the space around their buttons).
    pub fn width(&self, ui: &Ui) -> f32 {
        self.measure(ui).width
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = theme::theme(ui.ctx());
        let p = &theme.palette;
        let enabled = self.disabled_reason.is_none() && ui.is_enabled();
        let h = self.size.height();
        let Measured {
            galley,
            lead,
            trail,
            width: w,
        } = self.measure(ui);
        let label_w = galley.as_ref().map_or(0.0, |g| g.size().x);
        let badge_text = match self.trailing {
            Trailing::Badge(n) => Some(n.to_string()),
            _ => None,
        };
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
            let pressed = if enabled { press_t(&resp, &theme) } else { 0.0 };
            let mut l = look(self.variant, &theme, hover, pressed, self.open);
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
            let content_w = lead + label_w + trail;
            let mut x = rect.center().x - content_w / 2.0;
            let cy = rect.center().y;
            if self.busy {
                let c = pos2(x + icon_size::INLINE / 2.0, cy);
                progress::spinner(ui, c, icon_size::INLINE / 2.0 - 1.0, l.text);
                x += lead;
            } else if let Some(icon) = self.icon {
                painter.text(
                    pos2(x + icon_size::INLINE / 2.0, cy),
                    Align2::CENTER_CENTER,
                    icon.text(false),
                    icons::font(icon_size::INLINE),
                    l.text,
                );
                x += lead;
            }
            if let Some(galley) = galley {
                let text_pos = pos2(x, cy - galley.size().y / 2.0);
                if self.variant == Variant::Link && hover > 0.5 {
                    let y = text_pos.y + galley.size().y;
                    painter.hline(x..=x + galley.size().x, y, Stroke::new(1.0, l.text));
                }
                x += galley.size().x;
                painter.galley(text_pos, galley, l.text);
            }
            match self.trailing {
                Trailing::Caret if !self.icon_only => {
                    painter.text(
                        pos2(x + space::S1 + space::S3 / 2.0, cy),
                        Align2::CENTER_CENTER,
                        icons::CARET_DOWN.text(false),
                        icons::font(space::S3),
                        fade(l.text, 0.7),
                    );
                }
                Trailing::Badge(_) => {
                    let c = pos2(x + space::S2 + control::BADGE / 2.0, cy);
                    painter.circle_filled(c, control::BADGE / 2.0, p.accent);
                    painter.text(
                        c,
                        Align2::CENTER_CENTER,
                        badge_text.unwrap_or_default(),
                        Type::Label.numeric_font_id(),
                        p.text_on_accent,
                    );
                }
                _ => {}
            }
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        match self.disabled_reason {
            Some(reason) => {
                let tip = tooltip::rich(ui.ctx(), reason);
                resp.on_disabled_hover_text(tip)
            }
            None if self.icon_only => tooltip::tooltip(resp, &name, self.shortcut),
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
    /// Icon-only 44 px disc on a cover.
    Card,
    /// `md` with label in list rows; quiet until pointed at or focused.
    Row,
    /// `lg` with label: the spotlight.
    Feature,
    /// `xl`, minimum 120 wide, on the game page.
    Page,
}

/// How the pill is drawn this frame.
#[derive(Clone, Copy, Debug)]
struct PillPaint {
    opacity: f32,
    scale: f32,
    /// Row pills are quiet (no accent fill) until pointed at or focused.
    quiet: bool,
}

fn pill_metrics(size: PillSize) -> (f32, f32) {
    match size {
        PillSize::Card => (control::LG, icon_size::BUTTON),
        // Rows are 48 px: a 36 px pill leaves air above and below (UX-004).
        PillSize::Row => (control::MD, icon_size::INLINE),
        PillSize::Feature => (control::LG, icon_size::BUTTON - 2.0),
        PillSize::Page => (control::XL, icon_size::BUTTON),
    }
}

fn pill_label(ui: &Ui, state: &PlayState, size: PillSize) -> Option<std::sync::Arc<egui::Galley>> {
    let font = match size {
        PillSize::Card => return None,
        PillSize::Page => Type::Heading.font_id(),
        PillSize::Row | PillSize::Feature => Type::BodyStrong.font_id(),
    };
    Some(WidgetText::from(state.label()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        font,
    ))
}

fn paint_pill(
    ui: &Ui,
    rect: Rect,
    resp: &Response,
    state: &PlayState,
    size: PillSize,
    galley: Option<std::sync::Arc<egui::Galley>>,
    how: PillPaint,
) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (_, lead) = pill_metrics(size);
    let hover = hover_t(resp, &theme);
    let pressed = press_t(resp, &theme);
    let rect = scaled(rect, how.scale);
    let corner = CornerRadius::same(radius::PILL);
    let (fill, fg, border) = if state.accent_filled() && !how.quiet {
        let fill = mix(
            mix(p.accent, p.accent_hover, hover),
            p.accent_pressed,
            pressed,
        );
        (fill, p.text_on_accent, None)
    } else if size == PillSize::Card {
        // On artwork: an opaque plane so the glyph never depends on the cover.
        let border = if *state == PlayState::Running && hover > 0.0 {
            mix(p.border_strong, p.danger, hover)
        } else {
            p.border_strong
        };
        (
            mix(p.bg_overlay, p.text_primary, 0.06 * hover),
            p.text_primary,
            Some(border),
        )
    } else {
        let fill = mix(
            p.fill_selected,
            mix(p.fill_selected, p.text_primary, alpha::HOVER_LIGHTEN),
            hover,
        );
        let border = if *state == PlayState::Running && hover > 0.0 {
            mix(p.hairline, p.danger, hover)
        } else {
            p.hairline
        };
        (fill, p.text_primary, Some(border))
    };
    let painter = ui.painter().clone().with_layer_id(ui.layer_id());
    let f = |c: Color32| fade(c, how.opacity);
    painter.rect_filled(rect, corner, f(fill));
    if let Some(b) = border {
        painter.rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, f(b)),
            StrokeKind::Inside,
        );
    }
    let gap = space::S2;
    let content_w = galley.as_ref().map_or(lead, |g| lead + gap + g.size().x);
    let mut x = rect.center().x - content_w / 2.0;
    let cy = rect.center().y;
    let show_label = galley.is_some();
    if state.busy() {
        let spinner = if state.accent_filled() { fg } else { p.accent };
        progress::spinner(ui, pos2(x + lead / 2.0, cy), lead / 2.0 - 1.5, f(spinner));
    } else if *state == PlayState::Running && show_label {
        // Running is dot *and* word: the success dot takes the glyph's place.
        painter.circle_filled(pos2(x + lead / 2.0, cy), 4.0, f(p.success));
    } else if let Some(glyph) = state.glyph() {
        // The play triangle's optical centre sits right of its box centre.
        let nudge = if glyph == icons::PLAY && !show_label {
            1.5
        } else {
            0.0
        };
        let glyph_c = if how.quiet && state.accent_filled() {
            p.accent
        } else {
            fg
        };
        painter.text(
            pos2(x + lead / 2.0 + nudge, cy),
            Align2::CENTER_CENTER,
            glyph.text(true),
            icons::font(lead),
            f(glyph_c),
        );
    }
    x += lead + gap;
    if let Some(g) = galley {
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, f(fg));
    }
    if resp.has_focus() {
        focus_ring(&painter, rect, corner, &theme);
    }
}

/// The Play pill: same place in every state, label and colour change (§7.1).
/// `opacity` lets a list row keep it out of sight until the row is pointed at.
pub fn play_pill(
    ui: &mut Ui,
    state: &PlayState,
    game_name: &str,
    size: PillSize,
    opacity: f32,
) -> Response {
    let (h, lead) = pill_metrics(size);
    let galley = pill_label(ui, state, size);
    let pad = match size {
        PillSize::Page => space::S6,
        PillSize::Feature => space::S5,
        _ => space::S4,
    };
    let w = match &galley {
        Some(g) => {
            let w = pad * 2.0 + lead + space::S2 + g.size().x;
            match size {
                PillSize::Page => w.max(control::PILL_MIN_W),
                PillSize::Feature => w.max(control::PILL_MIN_W - space::S2),
                _ => w,
            }
        }
        None => h,
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let name = state.accessible_name(game_name);
    let resp = tooltip::attach(resp, &name);
    a11y::button(&resp, &name);
    if !ui.is_rect_visible(rect) || opacity <= 0.0 {
        return resp;
    }
    // In list rows the pill is quiet until it is pointed at or focused: one accent fill per
    // view (UI_DESIGN_SYSTEM §2.5 rule 1), not one per row.
    let theme = theme::theme(ui.ctx());
    let quiet = size == PillSize::Row && hover_t(&resp, &theme) <= 0.0 && !resp.has_focus();
    paint_pill(
        ui,
        rect,
        &resp,
        state,
        size,
        galley,
        PillPaint {
            opacity,
            scale: 1.0,
            quiet,
        },
    );
    resp
}

/// The Play control on a cover: a 44 px disc in `rect` that fades and grows in with
/// `reveal` (0–1) when the card is pointed at or focused.
pub fn play_disc(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    state: &PlayState,
    game_name: &str,
    reveal: f32,
) -> Response {
    // Not a separate tab stop in the grid (Space / Ctrl+Enter play instead).
    let resp = ui.interact(rect, id, Sense::CLICK);
    let name = state.accessible_name(game_name);
    let resp = tooltip::attach(resp, &name);
    a11y::button(&resp, &name);
    if reveal > 0.0 && ui.is_rect_visible(rect) {
        let theme = theme::theme(ui.ctx());
        let grow = if theme.motion.reduced {
            1.0
        } else {
            travel::REVEAL_FROM + (1.0 - travel::REVEAL_FROM) * reveal
        };
        paint_pill(
            ui,
            rect,
            &resp,
            state,
            PillSize::Card,
            None,
            PillPaint {
                opacity: reveal,
                scale: grow,
                quiet: false,
            },
        );
    }
    resp
}

/// Size of the pill for a given placement (for layout before painting).
pub fn pill_height(size: PillSize) -> f32 {
    pill_metrics(size).0
}

/// Minimum hit target helper for small controls (ACCESSIBILITY §3).
pub fn min_target(v: Vec2) -> Vec2 {
    v.max(Vec2::splat(control::MIN_TARGET))
}
