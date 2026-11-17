//! Buttons and Play (the design's PARTS: Button, Play).
//!
//! A button is 30 high (26 and 22 for small ones), radius 8, 10 padding, 7 between its icon
//! and its label, 13 / 500. The default has no fill until pointed at; a pressed button
//! shrinks to 97 % for 80 ms. Play is the only pill: 44 high on the game page, 26 in the
//! pinned tab bar, a 30 (or 40) disc on a cover and a quiet icon button in a list row.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontId, Galley, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget,
    pos2, vec2,
};

use super::{
    add_scaled, contrast_outline, focus_ring, glyph_shape, label_galley, pointer_t, press_scale,
    press_t, progress, tooltip,
};
use crate::a11y;
use crate::strings;
use crate::theme::palette::{Shade, fade, mix};
use crate::theme::tokens::{alpha, control, icon as icon_size, radius, travel};
use crate::theme::{self, Icon, Theme, Type, icons};

/// Button emphasis. The CSS classes of the design: `.b` is [`Variant::Ghost`], `.b--box`
/// [`Variant::Secondary`], `.b--primary` [`Variant::Primary`], `.b--danger`
/// [`Variant::Danger`], `.b--warn` [`Variant::Warn`], `.link` [`Variant::Link`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variant {
    /// The accent fill: the one main action of a view. 600 weight.
    Primary,
    /// A faint fill (`o1`) with primary text: dialog and row actions.
    #[default]
    Secondary,
    /// No longer part of the design: drawn as [`Variant::Secondary`].
    Tonal,
    /// No fill until pointed at; secondary text.
    Ghost,
    /// Destructive and final: the coral fill. 600 weight.
    Danger,
    /// A quiet button whose action destroys something: coral text, a coral tint under the
    /// pointer ("Reset…", "Remove…").
    Warn,
    /// Accent text, underlined under the pointer.
    Link,
}

/// Button height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Size {
    /// 22 high, 11 pt text: the smallest text button ("Show as table").
    Xs,
    /// 26 high, with a 13 pt icon: a button inside a row ("Other programs").
    Sm,
    /// 30 high: every other button.
    #[default]
    Md,
    /// Play's size (`.b--primary.play`): a pill 44 high, radius 22, padded 19 · 24, at
    /// least 112 wide, its label 15 pt (600 on the accent fill) and its icon 7 from it. The
    /// first game's "Add game" and the welcome steps' buttons.
    Lg,
}

impl Size {
    /// Height in points.
    pub const fn height(self) -> f32 {
        match self {
            Size::Xs => control::BUTTON_XS,
            Size::Sm => control::BUTTON_SM,
            Size::Md => control::BUTTON,
            Size::Lg => control::PLAY,
        }
    }

    /// The icon's size.
    const fn glyph(self) -> f32 {
        match self {
            Size::Xs | Size::Sm => icon_size::SMALL,
            Size::Md | Size::Lg => icon_size::INLINE,
        }
    }
}

/// What follows the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trailing {
    None,
    /// A caret: the button opens a menu.
    Caret,
    /// A count: how many filters are on.
    Badge(usize),
}

/// Colours of one state.
struct Look {
    fill: Color32,
    text: Color32,
}

/// `c` with its channels multiplied by `k` (CSS `brightness()`).
fn brighten(c: Color32, k: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let f = |v: u8| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(r), f(g), f(b), a)
}

/// The colours of `variant` pointed at by `hover`, pressed by `pressed` (both 0–1).
fn look(variant: Variant, theme: &Theme, hover: f32, pressed: f32, enabled: bool) -> Look {
    let p = &theme.palette;
    if !enabled {
        return Look {
            fill: if variant == Variant::Primary {
                p.o1
            } else {
                Color32::TRANSPARENT
            },
            text: p.t4,
        };
    }
    match variant {
        Variant::Primary => Look {
            fill: mix(mix(p.fill, p.fill_hover, hover), p.fill_pressed, pressed),
            text: p.on_fill,
        },
        Variant::Secondary | Variant::Tonal => Look {
            fill: mix(mix(p.o1, p.o2, hover), p.o3, pressed),
            text: p.t1,
        },
        Variant::Ghost => Look {
            fill: mix(fade(p.o2, hover), p.o3, pressed),
            text: mix(p.t2, p.t1, hover),
        },
        Variant::Danger => Look {
            fill: brighten(
                p.coral,
                1.0 + (alpha::DANGER_HOVER_BRIGHTNESS - 1.0) * hover,
            ),
            text: p.on_coral,
        },
        Variant::Warn => Look {
            fill: fade(p.coral_tint, hover),
            text: p.coral,
        },
        Variant::Link => Look {
            fill: Color32::TRANSPARENT,
            text: p.accent_text,
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
    /// A boxed ([`Variant::Secondary`]) button, 30 high.
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

    /// Ghost variant (the design's plain `.b`).
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

    /// Busy: a spinner replaces the leading icon and the button is drawn at 80 %; the label
    /// and width stay ("Starting…").
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Disable with a reason shown as the tooltip.
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

    /// A trailing count in quiet figures (nothing for zero).
    pub fn badge(mut self, count: usize) -> Self {
        if count > 0 {
            self.trailing = Trailing::Badge(count);
        }
        self
    }

    /// Hide the label and show it as the tooltip instead: a square button. Needs an icon;
    /// the accessible name is unchanged.
    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only && self.icon.is_some();
        self
    }

    /// The button's menu is open: it keeps its pointed-at look.
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
    galley: Option<Arc<Galley>>,
    count: Option<Arc<Galley>>,
    lead: f32,
    trail: f32,
    width: f32,
}

impl Button<'_> {
    fn font(&self) -> (FontId, f32) {
        let t = match (self.size, self.variant) {
            (Size::Xs, _) => Type::MicroStrong,
            (Size::Lg, Variant::Primary | Variant::Danger) => Type::Heading,
            (Size::Lg, _) => Type::HeadingQuiet,
            (_, Variant::Primary | Variant::Danger) => Type::BodyBold,
            _ => Type::BodyStrong,
        };
        (t.font_id(), t.line_height())
    }

    /// Padding left and right of the content.
    fn pads(&self) -> [f32; 2] {
        match (self.variant, self.size) {
            (Variant::Link, _) => [0.0, 0.0],
            (_, Size::Lg) => control::PLAY_PAD,
            _ => [control::BUTTON_PAD; 2],
        }
    }

    /// The gap between the icon, the label and what follows it.
    fn gap(&self) -> f32 {
        if self.size == Size::Lg {
            control::PLAY_GAP
        } else {
            control::BUTTON_GAP
        }
    }

    fn measure(&self, ui: &Ui) -> Measured {
        let h = self.size.height();
        let [pad_l, pad_r] = self.pads();
        let gap = self.gap();
        let galley = (!self.icon_only).then(|| {
            let (font, line) = self.font();
            label_galley(ui.painter(), self.label, font, line)
        });
        let label_w = galley.as_ref().map_or(0.0, |g| g.size().x);
        let has_lead = self.icon.is_some() || self.busy;
        let lead = match (has_lead, galley.is_some()) {
            (false, _) => 0.0,
            (true, true) => self.size.glyph() + gap,
            (true, false) => self.size.glyph(),
        };
        let count = match self.trailing {
            Trailing::Badge(n) if !self.icon_only => {
                Some(Type::Micro.numeric_galley(ui, &n.to_string(), Color32::PLACEHOLDER))
            }
            _ => None,
        };
        let trail = match (self.trailing, &count) {
            (Trailing::Caret, _) if !self.icon_only => gap + icon_size::SMALL,
            (_, Some(c)) => gap + c.size().x,
            _ => 0.0,
        };
        let least = if self.size == Size::Lg {
            control::PLAY_MIN_W
        } else {
            0.0
        };
        let width = if self.icon_only {
            h.max(self.min_width)
        } else {
            (pad_l + lead + label_w + trail + pad_r)
                .max(self.min_width)
                .max(least)
        };
        Measured {
            galley,
            count,
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
        let m = self.measure(ui);
        // Allocated in the caller's own layout (no scope of its own), so a row of buttons
        // in a wrapping layout wraps; a disabled one is allocated disabled.
        let resp = ui.add_enabled(enabled, |ui: &mut Ui| {
            ui.allocate_exact_size(vec2(m.width, h), Sense::click()).1
        });
        let rect = resp.rect;
        let name = self
            .accessible_name
            .clone()
            .unwrap_or_else(|| self.label.to_owned());
        a11y::button(&resp, &name);

        if ui.is_rect_visible(rect) {
            // Inside a disabled row egui already fades everything: only a reason of the
            // button's own draws the disabled look.
            let looks_enabled = self.disabled_reason.is_none();
            let live = enabled && !self.busy;
            let hover = if live {
                pointer_t(&resp, &theme).max(f32::from(u8::from(self.open)))
            } else {
                0.0
            };
            let pressed = if live { press_t(&resp, &theme) } else { 0.0 };
            let l = look(self.variant, &theme, hover, pressed, looks_enabled);
            let opacity = if self.busy { alpha::BUSY } else { 1.0 };
            let f = |c: Color32| fade(c, opacity);
            let corner = CornerRadius::same(match (self.variant, self.size) {
                (Variant::Link, _) => radius::XS,
                (_, Size::Lg) => radius::PILL,
                _ => radius::MD,
            });
            let painter = ui.painter();
            let mut shapes = vec![Shape::rect_filled(rect, corner, f(l.fill))];
            if self.variant != Variant::Link
                && let Some(o) = contrast_outline(rect, corner, &theme)
            {
                shapes.push(o);
            }
            let label_w = m.galley.as_ref().map_or(0.0, |g| g.size().x);
            let content_w = m.lead + label_w + m.trail;
            // Centred between the paddings (Play's are uneven).
            let [pad_l, pad_r] = self.pads();
            let mut x = if self.icon_only {
                rect.center().x - content_w / 2.0
            } else {
                rect.left() + pad_l + (rect.width() - pad_l - pad_r - content_w) / 2.0
            };
            let cy = rect.center().y;
            let glyph = self.size.glyph();
            if self.busy {
                shapes.push(progress::spinner_shape(
                    ui.ctx(),
                    pos2(x + glyph / 2.0, cy),
                    control::SPINNER / 2.0,
                    f(l.text),
                ));
                x += m.lead;
            } else if let Some(icon) = self.icon {
                // Play and Stop are solid in the design; other glyphs are outlines.
                let solid = icon == icons::PLAY || icon == icons::STOP;
                shapes.push(glyph_shape(
                    painter,
                    pos2(x + glyph / 2.0, cy),
                    icon,
                    solid,
                    glyph,
                    f(l.text),
                ));
                x += m.lead;
            }
            if let Some(galley) = m.galley {
                let w = galley.size().x;
                let at = pos2(x, cy - galley.size().y / 2.0);
                if self.variant == Variant::Link && hover > 0.5 {
                    let y = at.y + galley.size().y - 1.0;
                    shapes.push(Shape::hline(
                        x..=x + w,
                        y,
                        Stroke::new(theme::tokens::stroke::BORDER, f(l.text)),
                    ));
                }
                shapes.push(Shape::galley(at, galley, f(l.text)));
                x += w;
            }
            match (self.trailing, m.count) {
                (Trailing::Caret, _) if !self.icon_only => {
                    shapes.push(glyph_shape(
                        painter,
                        pos2(x + self.gap() + icon_size::SMALL / 2.0, cy),
                        icons::CARET_DOWN,
                        false,
                        icon_size::SMALL,
                        f(l.text),
                    ));
                }
                (_, Some(c)) => {
                    let at = pos2(x + self.gap(), cy - c.size().y / 2.0);
                    let quiet = if self.variant == Variant::Primary {
                        l.text
                    } else {
                        p.t3
                    };
                    shapes.push(Shape::galley(at, c, f(quiet)));
                }
                _ => {}
            }
            let scale = if self.variant == Variant::Link {
                1.0
            } else {
                press_scale(pressed, &theme)
            };
            add_scaled(painter, rect.center(), scale, shapes);
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        match self.disabled_reason {
            Some(reason) => tooltip::disabled(resp, reason),
            None if self.icon_only => tooltip::tooltip(resp, &name, self.shortcut),
            None => resp,
        }
    }
}

/// The state of Play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// Ready to start.
    Ready,
    /// Starting; the phase text ("Starting…", "Preparing…").
    Launching(&'static str),
    /// Running: Play becomes Stop.
    Running,
    /// A stop was requested.
    Stopping,
    /// The program is missing: Play becomes "Locate…".
    Unavailable,
    /// A compatibility layer must be downloaded first.
    NeedsRuntime,
    /// The entry is an installer.
    Installer,
}

impl PlayState {
    /// Whether Play starts something in this state (the accent fill).
    fn starts(&self) -> bool {
        matches!(
            self,
            PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer
        )
    }

    fn label(&self) -> &'static str {
        match self {
            PlayState::Ready => strings::play::PLAY(),
            PlayState::Launching(phase) => phase,
            PlayState::Running => strings::play::STOP(),
            PlayState::Stopping => strings::play::STOPPING(),
            PlayState::Unavailable => strings::play::LOCATE(),
            PlayState::NeedsRuntime => strings::play::DOWNLOAD_AND_PLAY(),
            PlayState::Installer => strings::play::RUN_INSTALLER(),
        }
    }

    fn mark(&self) -> Mark {
        match self {
            PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer => {
                Mark::Glyph(icons::PLAY)
            }
            PlayState::Running => Mark::Glyph(icons::STOP),
            PlayState::Launching(_) | PlayState::Stopping => Mark::Spinner,
            PlayState::Unavailable => Mark::None,
        }
    }

    fn draw(&self) -> PlayDraw {
        PlayDraw {
            tone: match self {
                s if s.starts() => Tone::Primary,
                PlayState::Launching(_) => Tone::Starting,
                _ => Tone::Box,
            },
            mark: self.mark(),
            label: self.label(),
            disc: match self {
                s if s.starts() => Disc::Fill,
                PlayState::Unavailable => Disc::Off,
                _ => Disc::Art,
            },
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

/// Where Play sits, which fixes its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillSize {
    /// A disc on a cover (see [`play_disc`]).
    Card,
    /// A list row: a quiet 30 square icon button.
    Row,
    /// No longer part of the design: drawn as [`PillSize::Page`].
    Feature,
    /// The game page: 44 high, at least 112 wide.
    Page,
    /// The pinned tab bar of the game page: 26 high.
    Compact,
}

/// What stands before Play's label.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    None,
    Glyph(Icon),
    Spinner,
}

/// The pill's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    /// Starts the game: the accent fill.
    Primary,
    /// Starting: the accent fill at 80 %, not pressable.
    Starting,
    /// Stop, Locate…: the `o2` pill.
    Box,
    /// Another game is running: the `o2` pill with disabled text.
    Blocked,
}

/// The disc's look on a cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disc {
    /// The accent fill.
    Fill,
    /// Running or starting: a light disc with a dark glyph.
    Art,
    /// Cannot start now.
    Off,
}

#[derive(Clone, Copy, Debug)]
struct PlayDraw {
    tone: Tone,
    mark: Mark,
    label: &'static str,
    disc: Disc,
}

impl PlayDraw {
    fn blocked() -> Self {
        Self {
            tone: Tone::Blocked,
            mark: Mark::Glyph(icons::PLAY),
            label: strings::play::PLAY(),
            disc: Disc::Off,
        }
    }

    fn pressable(&self) -> bool {
        !matches!(self.tone, Tone::Starting | Tone::Blocked)
    }
}

/// Measures of a pill: height, padding left and right, gap, glyph size, least width.
struct PillMetrics {
    h: f32,
    pad: [f32; 2],
    gap: f32,
    glyph: f32,
    min_w: f32,
}

fn pill_metrics(size: PillSize) -> PillMetrics {
    match size {
        PillSize::Card => PillMetrics {
            h: control::DISC,
            pad: [0.0, 0.0],
            gap: 0.0,
            glyph: icon_size::INLINE,
            min_w: control::DISC,
        },
        PillSize::Row => PillMetrics {
            h: control::BUTTON,
            pad: [0.0, 0.0],
            gap: 0.0,
            glyph: icon_size::INLINE,
            min_w: control::BUTTON,
        },
        PillSize::Page | PillSize::Feature => PillMetrics {
            h: control::PLAY,
            pad: control::PLAY_PAD,
            gap: control::PLAY_GAP,
            glyph: icon_size::INLINE,
            min_w: control::PLAY_MIN_W,
        },
        PillSize::Compact => PillMetrics {
            h: control::PLAY_SM,
            pad: control::PLAY_SM_PAD,
            gap: control::PLAY_SM_GAP,
            glyph: icon_size::SMALL,
            min_w: 0.0,
        },
    }
}

/// The label's font: 15 pt on the page, 13 in the tab bar; semibold on the accent fill.
fn pill_font(size: PillSize, tone: Tone) -> FontId {
    let strong = matches!(tone, Tone::Primary | Tone::Starting);
    match size {
        PillSize::Page | PillSize::Feature => {
            if strong {
                Type::Heading.font_id()
            } else {
                Type::HeadingQuiet.font_id()
            }
        }
        _ => {
            if strong {
                Type::BodyBold.font_id()
            } else {
                Type::BodyStrong.font_id()
            }
        }
    }
}

fn pill_label(ui: &Ui, draw: &PlayDraw, size: PillSize) -> Option<Arc<Galley>> {
    match size {
        PillSize::Card | PillSize::Row => None,
        PillSize::Page | PillSize::Feature => Some(label_galley(
            ui.painter(),
            draw.label,
            pill_font(size, draw.tone),
            Type::Heading.line_height(),
        )),
        PillSize::Compact => Some(label_galley(
            ui.painter(),
            draw.label,
            pill_font(size, draw.tone),
            Type::Body.line_height(),
        )),
    }
}

fn pill_width(size: PillSize, draw: &PlayDraw, galley: Option<&Arc<Galley>>) -> f32 {
    let m = pill_metrics(size);
    match galley {
        Some(g) => {
            let lead = if draw.mark == Mark::None {
                0.0
            } else {
                m.glyph + m.gap
            };
            (m.pad[0] + lead + g.size().x + m.pad[1]).max(m.min_w)
        }
        None => m.h,
    }
}

/// Paint a pill or a list row's icon button for `draw` in `rect`.
#[allow(clippy::too_many_arguments)]
fn paint_pill(
    ui: &Ui,
    rect: Rect,
    resp: &Response,
    draw: &PlayDraw,
    size: PillSize,
    galley: Option<Arc<Galley>>,
    opacity: f32,
) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let m = pill_metrics(size);
    let live = draw.pressable() && ui.is_enabled();
    let hover = if live { pointer_t(resp, &theme) } else { 0.0 };
    let pressed = if live { press_t(resp, &theme) } else { 0.0 };
    let (fill, fg, busy) = if size == PillSize::Row {
        // The list row's `.b.ib`: no fill until pointed at; disabled text while blocked, as
        // the other sizes have it.
        let l = look(
            Variant::Ghost,
            &theme,
            hover,
            pressed,
            draw.tone != Tone::Blocked,
        );
        (l.fill, l.text, false)
    } else {
        match draw.tone {
            Tone::Primary => {
                let l = look(Variant::Primary, &theme, hover, pressed, true);
                (l.fill, l.text, false)
            }
            Tone::Starting => (p.fill, p.on_fill, true),
            Tone::Box => (p.o2, p.t1, false),
            Tone::Blocked => (p.o2, p.t4, false),
        }
    };
    let f = |c: Color32| fade(c, opacity * if busy { alpha::BUSY } else { 1.0 });
    let corner = if size == PillSize::Row {
        CornerRadius::same(radius::MD)
    } else {
        CornerRadius::same(radius::PILL)
    };
    let painter = ui.painter();
    let mut shapes = vec![Shape::rect_filled(rect, corner, f(fill))];
    if let Some(o) = contrast_outline(rect, corner, &theme) {
        shapes.push(o);
    }
    let cy = rect.center().y;
    let lead = if draw.mark == Mark::None {
        0.0
    } else {
        m.glyph + if galley.is_some() { m.gap } else { 0.0 }
    };
    let content_w = lead + galley.as_ref().map_or(0.0, |g| g.size().x);
    // Centred between the paddings (the pill may be wider than its content).
    let room = rect.width() - m.pad[0] - m.pad[1];
    let mut x = rect.left() + m.pad[0] + (room - content_w) / 2.0;
    let mark_c = pos2(x + m.glyph / 2.0, cy);
    match draw.mark {
        Mark::None => {}
        Mark::Glyph(icon) => shapes.push(glyph_shape(painter, mark_c, icon, true, m.glyph, f(fg))),
        Mark::Spinner => shapes.push(progress::spinner_shape(
            ui.ctx(),
            mark_c,
            control::SPINNER / 2.0,
            f(fg),
        )),
    }
    x += lead;
    if let Some(g) = galley {
        shapes.push(Shape::galley(pos2(x, cy - g.size().y / 2.0), g, f(fg)));
    }
    add_scaled(painter, rect.center(), press_scale(pressed, &theme), shapes);
    if resp.has_focus() {
        focus_ring(painter, rect, corner, &theme);
    }
}

fn play_pill_draw(
    ui: &mut Ui,
    draw: PlayDraw,
    name: &str,
    tip: &str,
    size: PillSize,
    opacity: f32,
) -> Response {
    if size == PillSize::Card {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(control::DISC), Sense::hover());
        let id = ui.next_auto_id();
        return disc(ui, rect, id, draw, name, tip, 1.0);
    }
    let galley = pill_label(ui, &draw, size);
    let w = pill_width(size, &draw, galley.as_ref());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, pill_metrics(size).h), Sense::click());
    a11y::button(&resp, name);
    if draw.tone == Tone::Blocked {
        a11y::describe(&resp, tip);
    }
    let resp = tooltip::attach(resp, tip);
    if opacity > 0.0 && ui.is_rect_visible(rect) {
        paint_pill(ui, rect, &resp, &draw, size, galley, opacity);
    }
    resp
}

/// Play: the same place in every state; its label, glyph and fill change. `opacity` lets a
/// list row keep it out of sight until the row is pointed at.
pub fn play_pill(
    ui: &mut Ui,
    state: &PlayState,
    game_name: &str,
    size: PillSize,
    opacity: f32,
) -> Response {
    let name = state.accessible_name(game_name);
    play_pill_draw(ui, state.draw(), &name, &name, size, opacity)
}

/// Play while another game is running: the quiet pill with disabled text. It stays
/// focusable and says `reason` ("Stop Lantern Keep first") as its tooltip; a click does
/// nothing the caller should act on.
pub fn play_pill_blocked(
    ui: &mut Ui,
    game_name: &str,
    reason: &str,
    size: PillSize,
    opacity: f32,
) -> Response {
    let name = strings::play::play_name(game_name);
    play_pill_draw(ui, PlayDraw::blocked(), &name, reason, size, opacity)
}

fn disc(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    draw: PlayDraw,
    name: &str,
    tip: &str,
    reveal: f32,
) -> Response {
    // Not a separate tab stop in the grid (Space / Ctrl+Enter play instead).
    let resp = ui.interact(rect, id, Sense::CLICK);
    a11y::button(&resp, name);
    if draw.tone == Tone::Blocked {
        a11y::describe(&resp, tip);
    }
    let resp = tooltip::attach(resp, tip);
    if reveal <= 0.0 || !ui.is_rect_visible(rect) {
        return resp;
    }
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let live = draw.disc == Disc::Fill;
    let hover = if live { pointer_t(&resp, &theme) } else { 0.0 };
    let pressed = if live { press_t(&resp, &theme) } else { 0.0 };
    let (fill, fg) = match draw.disc {
        Disc::Fill => (
            mix(mix(p.fill, p.fill_hover, hover), p.fill_pressed, pressed),
            p.on_fill,
        ),
        Disc::Art => (p.on_art, p.on_art_ink),
        Disc::Off => (p.disc_off, p.on_disc_off),
    };
    let f = |c: Color32| fade(c, reveal);
    let r = rect.width().min(rect.height()) / 2.0;
    let c = rect.center();
    let ring = control::DISC_RING;
    let [y, blur] = control::DISC_SHADE;
    let shade = Shade {
        y: y as i8,
        blur,
        color: p.disc_ring,
    };
    let mut shadow = shade.shadow();
    shadow.color = f(shadow.color);
    let corner = CornerRadius::same(radius::PILL);
    let painter = ui.painter();
    let mut shapes = vec![
        shadow.as_shape(rect, corner).into(),
        Shape::circle_filled(c, r + ring, f(p.disc_ring)),
        Shape::circle_filled(c, r, f(fill)),
    ];
    match draw.mark {
        Mark::Spinner => shapes.push(progress::spinner_shape(
            ui.ctx(),
            c,
            control::SPINNER / 2.0,
            f(fg),
        )),
        Mark::Glyph(icon) => {
            shapes.push(glyph_shape(
                painter,
                c,
                icon,
                true,
                icon_size::INLINE,
                f(fg),
            ));
        }
        // "Locate…" has no glyph of its own: the disc keeps the play triangle.
        Mark::None => shapes.push(glyph_shape(
            painter,
            c,
            icons::PLAY,
            true,
            icon_size::INLINE,
            f(fg),
        )),
    }
    // Grows from 86 % as it is revealed (not under reduced motion).
    let scale = if theme.motion.reduced {
        1.0
    } else {
        travel::REVEAL_FROM + (1.0 - travel::REVEAL_FROM) * reveal.clamp(0.0, 1.0)
    };
    add_scaled(painter, c, scale, shapes);
    if resp.has_focus() {
        focus_ring(painter, rect, corner, &theme);
    }
    resp
}

/// Play on a cover: a disc in `rect` (30 across, 40 in a small library) that fades and
/// grows from 86 % with `reveal` (0–1) when the cover is pointed at or focused. The accent
/// fill when it would start the game; a light disc while the game runs or starts; a dark
/// one when it cannot start.
pub fn play_disc(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    state: &PlayState,
    game_name: &str,
    reveal: f32,
) -> Response {
    let name = state.accessible_name(game_name);
    disc(ui, rect, id, state.draw(), &name, &name, reveal)
}

/// [`play_disc`] while another game is running: the dark disc, with `reason` as its tooltip.
pub fn play_disc_blocked(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    game_name: &str,
    reason: &str,
    reveal: f32,
) -> Response {
    let name = strings::play::play_name(game_name);
    disc(ui, rect, id, PlayDraw::blocked(), &name, reason, reveal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Appearance;

    fn rect_of(f: impl FnOnce(&mut Ui) -> Response) -> Rect {
        crate::widgets::test_frame(&Appearance::default(), |ui| f(ui).rect)
    }

    #[test]
    fn a_row_of_buttons_wraps_and_a_disabled_one_wraps_with_it() {
        let tops = crate::widgets::test_frame(&Appearance::default(), |ui| {
            ui.set_max_width(200.0);
            ui.horizontal_wrapped(|ui| {
                let mut tops = Vec::new();
                for i in 0..6 {
                    let b = Button::new("Choose a file instead…");
                    let b = if i == 3 {
                        b.disabled(Some("Not now"))
                    } else {
                        b
                    };
                    let r = ui.add(b);
                    assert!(r.rect.right() <= 200.0 + 0.5, "button {i} overflows");
                    assert_eq!(r.enabled(), i != 3);
                    tops.push(r.rect.top());
                }
                tops
            })
            .inner
        });
        assert!(tops.windows(2).all(|w| w[1] > w[0]), "{tops:?}");
    }

    #[test]
    fn buttons_are_30_26_and_22_high() {
        for (size, h) in [(Size::Md, 30.0), (Size::Sm, 26.0), (Size::Xs, 22.0)] {
            let r = rect_of(|ui| ui.add(Button::new("Copy").size(size)));
            assert_eq!(r.height(), h);
        }
    }

    #[test]
    fn a_play_sized_button_is_a_44_pill_at_least_112_wide() {
        let r = rect_of(|ui| ui.add(Button::primary("Add").size(Size::Lg).icon(icons::PLUS)));
        assert_eq!(r.size(), vec2(control::PLAY_MIN_W, control::PLAY));
    }

    #[test]
    fn an_icon_only_button_is_square() {
        let r = rect_of(|ui| ui.add(Button::ghost("More").icon(icons::DOTS).icon_only(true)));
        assert_eq!(r.size(), Vec2::splat(control::BUTTON));
    }

    #[test]
    fn play_on_the_page_is_112_by_44() {
        let r = rect_of(|ui| play_pill(ui, &PlayState::Ready, "Lantern Keep", PillSize::Page, 1.0));
        assert_eq!(r.size(), vec2(control::PLAY_MIN_W, control::PLAY));
        let r = rect_of(|ui| {
            play_pill(
                ui,
                &PlayState::Ready,
                "Lantern Keep",
                PillSize::Compact,
                1.0,
            )
        });
        assert_eq!(r.height(), control::PLAY_SM);
    }

    #[test]
    fn every_state_has_its_own_name() {
        let names: Vec<String> = [
            PlayState::Ready,
            PlayState::Running,
            PlayState::Unavailable,
            PlayState::Launching("Starting…"),
        ]
        .iter()
        .map(|s| s.accessible_name("Lantern Keep"))
        .collect();
        assert_eq!(
            names,
            [
                "Play Lantern Keep",
                "Stop Lantern Keep",
                "Locate Lantern Keep",
                "Starting Lantern Keep, starting"
            ]
        );
    }

    #[test]
    fn danger_brightens_under_the_pointer() {
        let c = Color32::from_rgb(200, 100, 50);
        assert_eq!(brighten(c, 1.08), Color32::from_rgb(216, 108, 54));
    }
}
