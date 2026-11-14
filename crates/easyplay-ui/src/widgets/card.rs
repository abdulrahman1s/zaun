//! The game card and the spotlight (UI_DESIGN_SYSTEM §7.2, SCREEN_SPECS §3, GAME_LIBRARY §2):
//! painter-level rendering into a rect the grid computed. Galleys go through egui's layout
//! cache, keyed by text, font and wrap width, so a steady grid lays out no text.
//!
//! A card is a cover with its caption beneath: the artwork is never written on. Controls
//! (favourite, more, play) arrive on the cover when the card is pointed at or focused.
//! The spotlight is the same game at the size of a collection that holds nothing else: the
//! cover beside its title, one sentence each about playtime and protection, and Play.

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, Availability, EntryKind, GameSummary};
use egui::accesskit::Role;
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Painter, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, TextureId, Ui, UiBuilder, pos2, vec2,
};

use super::button::{PillSize, PlayState, play_disc, play_pill};
use super::chip::{ChipKind, chip};
use super::cover::{CoverSource, paint_cover, paint_edge};
use super::icon_button::IconButton;
use super::{focus_ring, press_t, progress, shield, tooltip};
use crate::a11y;
use crate::strings;
use crate::theme::motion::{self, scaled};
use crate::theme::palette::{black, fade};
use crate::theme::tokens::{
    alpha, control, icon as icon_size, layout, radius, space, stroke, travel,
};
use crate::theme::{self, Density, Dur, Theme, Type, icons};

/// Inputs the card needs besides the summary.
#[derive(Clone, Copy, Debug)]
pub struct CardCtx {
    /// Loaded cover texture, if any.
    pub texture: Option<TextureId>,
    /// Dominant colour while the texture loads.
    pub placeholder: Option<Color32>,
    /// Wall-clock now (for elapsed time and relative dates).
    pub now: Timestamp,
    /// Local UTC offset in seconds.
    pub utc_offset_s: i32,
    /// Selected (keyboard cursor in the grid).
    pub selected: bool,
    /// Part of a multi-selection.
    pub multi_selected: bool,
    /// Controller mode: Play is always visible.
    pub controller: bool,
    /// In the `Tab` order (the grid's roving cell, ACCESSIBILITY §2.1).
    pub focusable: bool,
    /// Show playtime and last played (`appearance.show_playtime_on_cards`).
    pub show_playtime: bool,
    /// Grid density.
    pub density: Density,
    /// Opacity of a card that is entering (0–1): the grid fades new arrivals in.
    pub opacity: f32,
}

/// What the user did with a card this frame.
#[derive(Debug)]
pub struct CardResponse {
    /// The card's own response (click opens the game page).
    pub response: Response,
    /// Play control clicked, or double-click on the card.
    pub play: bool,
    /// Favourite star toggled.
    pub toggle_favorite: bool,
    /// The more control was clicked: open the game's menu here.
    pub menu: Option<Pos2>,
}

/// The pill state for a summary (GAME_LIBRARY §2 card state rules).
pub fn play_state(g: &GameSummary) -> PlayState {
    match g.activity {
        Activity::Running { .. } | Activity::Lingering => PlayState::Running,
        Activity::Launching(tag) => PlayState::Launching(strings::card::phase(tag)),
        Activity::Stopping => PlayState::Stopping,
        Activity::Idle => match g.availability {
            Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable => {
                PlayState::Unavailable
            }
            _ if g.kind == EntryKind::Installer => PlayState::Installer,
            _ => PlayState::Ready,
        },
    }
}

fn unavailable(g: &GameSummary) -> bool {
    matches!(
        g.availability,
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
    )
}

fn attention(g: &GameSummary) -> bool {
    g.flags.needs_attention || g.flags.suspected_tamper
}

/// A visible "Running · m:ss" label changes every second: ask for the next frame at the
/// next whole second of the session (PERFORMANCE §8, 1 Hz while a game runs, nothing
/// otherwise). Call wherever elapsed time is shown.
pub fn tick_elapsed(ctx: &egui::Context, activity: Activity, now: Timestamp) {
    if let Activity::Running { since } = activity {
        let ms = now.since(since).map_or(0, |d| d.as_millis() as u64);
        ctx.request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
    }
}

/// Height of the caption beneath a cover at `density`: the title (two lines, one when
/// compact) and the status line (none when compact). Fixed, so rows of covers stay aligned.
pub fn caption_height(density: Density) -> f32 {
    let title = Type::BodyStrong.line_height();
    match density {
        Density::Compact => layout::CAPTION_GAP + title,
        _ => {
            layout::CAPTION_GAP
                + title * 2.0
                + layout::CAPTION_LINE_GAP
                + Type::Caption.line_height()
        }
    }
}

/// The cover of a card occupying `rect` (the cover on top, the caption beneath).
pub fn cover_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        rect.min,
        vec2(rect.width(), rect.width() * layout::COVER_ASPECT),
    )
}

/// Interaction and the accessible name shared by the card and the spotlight.
fn interact(ui: &Ui, rect: Rect, id: Id, g: &GameSummary, ctx: &CardCtx) -> Response {
    let sense = if ctx.focusable {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let resp = ui.interact(rect, id, sense);
    super::own_arrow_keys(&resp);
    let state_text = strings::card::state(g.activity, g.availability, ctx.now);
    tick_elapsed(ui.ctx(), g.activity, ctx.now);
    let state_text = state_text
        .or_else(|| (g.kind == EntryKind::Installer).then(|| strings::card::INSTALLER.to_owned()));
    let name = strings::card::accessible_name(
        &g.name,
        g.preset,
        state_text
            .as_deref()
            .or(attention(g).then_some(strings::card::NEEDS_ATTENTION)),
        g.total_playtime_ms,
        g.last_played_at,
        ctx.now,
        ctx.utc_offset_s,
    );
    let selected = ctx.selected;
    resp.widget_info(move || egui::WidgetInfo::selected(Role::Button, true, selected, &name));
    resp
}

/// The cover with everything that belongs to the artwork itself: the image or its stand-in,
/// the dim and glyph of a game that cannot start, the edge, the launch bar.
#[allow(clippy::too_many_arguments)]
fn paint_art(
    ui: &Ui,
    painter: &Painter,
    cover: Rect,
    corner: CornerRadius,
    id: Id,
    g: &GameSummary,
    ctx: &CardCtx,
    hot: f32,
    theme: &Theme,
) {
    let p = &theme.palette;
    let source = match (ctx.texture, ctx.placeholder) {
        (Some(t), _) => {
            let fade_in =
                ui.ctx()
                    .animate_bool_with_time(id.with("tex"), true, theme.motion.fade(Dur::Base));
            CoverSource::Texture(t, fade_in)
        }
        (None, Some(c)) => CoverSource::Pending(c),
        (None, None) => CoverSource::Placeholder,
    };
    paint_cover(painter, cover, corner, source, &g.name, theme);
    if unavailable(g) {
        painter.rect_filled(cover, corner, black(alpha::UNAVAILABLE_DIM));
        let glyph = if g.availability == Availability::DriveDisconnected {
            icons::USB
        } else {
            icons::LINK_BREAK
        };
        let c = cover.center();
        scrim_disc(painter, c, icon_size::EMPTY / 2.0, 1.0, theme);
        painter.text(
            c,
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::NAV),
            p.on_art,
        );
    }
    paint_edge(painter, cover, corner, hot, theme);
    // Launch progress: a thin indeterminate bar along the bottom edge.
    if matches!(g.activity, Activity::Launching(_) | Activity::Stopping) {
        let r = f32::from(corner.sw);
        let bar = Rect::from_min_max(
            pos2(
                cover.left() + r,
                cover.bottom() - control::PROGRESS - space::S2,
            ),
            pos2(cover.right() - r, cover.bottom() - space::S2),
        );
        progress::paint_linear(ui, bar, None, p.accent, black(alpha::SCRIM));
    }
}

/// A disc behind a control on artwork.
fn scrim_disc(painter: &Painter, center: Pos2, r: f32, opacity: f32, theme: &Theme) {
    let p = &theme.palette;
    if p.allows_translucency() {
        painter.circle_filled(center, r, fade(p.scrim, alpha::SCRIM * opacity));
    } else {
        painter.circle_filled(center, r, p.scrim);
        painter.circle_stroke(center, r, Stroke::new(stroke::BORDER, p.border_strong));
    }
}

/// A round control on a cover: scrim disc and glyph, revealed with `reveal` (0–1). `label`
/// is its tooltip; the caller names it for assistive technology.
#[allow(clippy::too_many_arguments)]
fn cover_control(
    ui: &Ui,
    painter: &Painter,
    center: Pos2,
    id: Id,
    glyph: String,
    label: &str,
    reveal: f32,
    pop: f32,
    theme: &Theme,
) -> Response {
    let p = &theme.palette;
    let size = layout::SCRIM_DISC;
    let hit = Rect::from_center_size(center, vec2(control::MIN_TARGET, control::MIN_TARGET));
    let resp = ui.interact(hit, id, Sense::CLICK);
    let over = resp.hovered();
    let grow = if theme.motion.reduced {
        1.0
    } else {
        travel::REVEAL_FROM + (1.0 - travel::REVEAL_FROM) * reveal + travel::POP_PEAK * pop
    };
    scrim_disc(
        painter,
        center,
        size / 2.0 * grow,
        reveal * if over { 1.25 } else { 1.0 },
        theme,
    );
    painter.text(
        center,
        Align2::CENTER_CENTER,
        glyph,
        icons::font(((icon_size::INLINE + 1.0) * (1.0 + travel::POP_PEAK * pop)).round()),
        fade(p.on_art, reveal * if over { 1.0 } else { 0.9 }),
    );
    tooltip::attach(resp, label)
}

/// Favourite (top right), more (bottom left) and Play (bottom right) on `cover`.
#[allow(clippy::too_many_arguments)]
fn cover_controls(
    ui: &mut Ui,
    painter: &Painter,
    cover: Rect,
    id: Id,
    g: &GameSummary,
    reveal: f32,
    out: &mut CardResponse,
    theme: &Theme,
) {
    let inset = layout::COVER_INSET;
    let half = layout::SCRIM_DISC / 2.0;
    // The star: always when set, else with the other controls.
    if g.favorite || reveal > 0.0 {
        let c = pos2(cover.right() - inset - half, cover.top() + inset + half);
        let pop = motion::pulse(
            ui.ctx(),
            id.with("star"),
            g.favorite,
            theme.motion.movement(Dur::Slow),
        );
        let glyph = if g.favorite {
            icons::STAR_FILL.text(true)
        } else {
            icons::STAR.text(false)
        };
        let shown = if g.favorite { 1.0 } else { reveal };
        let star = cover_control(
            ui,
            painter,
            c,
            id.with("star"),
            glyph,
            strings::card::FAVORITE,
            shown,
            pop,
            theme,
        );
        a11y::toggle(&star, &strings::card::favorite_name(&g.name), g.favorite);
        if star.clicked() {
            out.toggle_favorite = true;
        }
    }
    if reveal <= 0.0 {
        return;
    }
    let c = pos2(cover.left() + inset + half, cover.bottom() - inset - half);
    let more = cover_control(
        ui,
        painter,
        c,
        id.with("more"),
        icons::DOTS.text(false),
        strings::card::MORE,
        reveal,
        0.0,
        theme,
    );
    a11y::button(&more, &strings::card::more_name(&g.name));
    if more.clicked() {
        out.menu = Some(more.rect.left_bottom() + vec2(0.0, space::S1));
    }
    let size = control::LG;
    let pill = Rect::from_min_size(
        pos2(cover.right() - inset - size, cover.bottom() - inset - size),
        vec2(size, size),
    );
    if play_disc(ui, pill, id.with("play"), &play_state(g), &g.name, reveal).clicked() {
        out.play = true;
    }
}

/// The ring of a card in a multi-selection, and its check.
fn paint_selection(painter: &Painter, cover: Rect, corner: CornerRadius, theme: &Theme) {
    let p = &theme.palette;
    let grow = stroke::FOCUS_GAP + stroke::FOCUS / 2.0;
    painter.rect_stroke(
        cover.expand(grow),
        CornerRadius::same(corner.nw.saturating_add(grow as u8)),
        Stroke::new(stroke::FOCUS, p.accent),
        StrokeKind::Middle,
    );
    let half = layout::SCRIM_DISC / 2.0;
    let c = pos2(
        cover.left() + layout::COVER_INSET + half,
        cover.top() + layout::COVER_INSET + half,
    );
    painter.circle_filled(c, half, p.accent);
    painter.text(
        c,
        Align2::CENTER_CENTER,
        icons::CHECK.text(false),
        icons::font(icon_size::INLINE),
        p.text_on_accent,
    );
}

/// What leads a status line.
enum StatusLead {
    /// The protection glyph (the alert shield when something needs attention).
    Shield,
    /// A session is running.
    Dot,
    /// A launch or stop is in progress.
    Spinner,
}

/// Paint and interact with one card in `rect` (see [`cover_rect`] and [`caption_height`]).
/// `id` must be stable per game.
pub fn card(ui: &mut Ui, rect: Rect, id: Id, g: &GameSummary, ctx: &CardCtx) -> CardResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = interact(ui, rect, id, g, ctx);
    let mut out = CardResponse {
        play: resp.double_clicked(),
        response: resp,
        toggle_favorite: false,
        menu: None,
    };
    if !ui.is_rect_visible(rect) {
        return out;
    }
    let resp = out.response.clone();

    let hot = resp.contains_pointer() || resp.has_focus() || ctx.controller;
    let reveal =
        ui.ctx()
            .animate_bool_with_time(id.with("hot"), hot, theme.motion.fade(Dur::Quick));
    let pressed = if theme.motion.reduced {
        0.0
    } else {
        press_t(&resp, &theme)
    };
    let cover = cover_rect(rect);
    let drawn = scaled(cover, 1.0 - (1.0 - travel::PRESS) * pressed);
    let corner = CornerRadius::same(radius::POP);
    let mut painter = ui.painter_at(rect.expand(stroke::FOCUS_GAP + stroke::FOCUS_HC + 1.0));
    painter.multiply_opacity(ctx.opacity);

    paint_art(ui, &painter, drawn, corner, id, g, ctx, reveal, &theme);
    if ctx.multi_selected {
        paint_selection(&painter, drawn, corner, &theme);
    }
    cover_controls(ui, &painter, cover, id, g, reveal, &mut out, &theme);

    // Caption: the title, then the status line.
    let compact = ctx.density == Density::Compact;
    let mut y = cover.bottom() + layout::CAPTION_GAP;
    let title = Type::BodyStrong.galley(
        ui,
        &g.name,
        p.text_primary,
        rect.width(),
        if compact { 1 } else { 2 },
    );
    let title_rect = Rect::from_min_size(pos2(rect.left(), y), title.size());
    let elided = title.elided;
    painter.galley(title_rect.min, title, p.text_primary);
    y += title_rect.height() + layout::CAPTION_LINE_GAP;
    if !compact {
        status_line(ui, &painter, rect, y, id, g, ctx, &theme);
    }

    if resp.has_focus() || ctx.selected {
        focus_ring(&painter, drawn, corner, &theme);
    }
    if elided {
        full_title(ui, &resp, title_rect, id, &g.name);
    }
    out
}

/// A title that had to be cut is whole one hover away: pointing at the title shows it, and
/// so does moving keyboard focus to the card. (It is always in the accessible name.)
/// Only the title's own rectangle answers the pointer, so covers passing under a resting
/// pointer while the shelf scrolls raise nothing.
fn full_title(ui: &Ui, card: &Response, title_rect: Rect, id: Id, name: &str) {
    let over = ui.interact(title_rect, id.with("title"), Sense::hover());
    tooltip::attach(over, name);
    if card.has_focus() && super::focus_visible(ui.ctx()) {
        card.show_tooltip_text(tooltip::rich(ui.ctx(), name));
    }
}

/// The status line of a card: the protection glyph, what is true now, and when.
#[allow(clippy::too_many_arguments)]
fn status_line(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    y: f32,
    id: Id,
    g: &GameSummary,
    ctx: &CardCtx,
    theme: &Theme,
) {
    let p = &theme.palette;
    let attention = attention(g);
    let active = g.activity != Activity::Idle;
    let (mut left, mut right) = if ctx.show_playtime || active || unavailable(g) {
        strings::card::status_parts(
            g.activity,
            g.availability,
            g.total_playtime_ms,
            g.last_played_at,
            ctx.now,
            ctx.utc_offset_s,
        )
    } else {
        (strings::shield::preset_name(g.preset).to_owned(), None)
    };
    let idle = !active && !unavailable(g);
    if idle && attention {
        left = strings::card::NEEDS_ATTENTION.to_owned();
        right = None;
    } else if idle && g.kind == EntryKind::Installer {
        left = strings::card::INSTALLER.to_owned();
        right = None;
    }
    let lead = match g.activity {
        Activity::Running { .. } | Activity::Lingering => StatusLead::Dot,
        Activity::Launching(_) | Activity::Stopping => StatusLead::Spinner,
        Activity::Idle => StatusLead::Shield,
    };
    let line_h = Type::Caption.line_height();
    let cy = y + line_h / 2.0;
    let lead_w = icon_size::CAPTION;
    let lead_c = pos2(rect.left() + lead_w / 2.0, cy);
    match lead {
        StatusLead::Shield => {
            let color = if attention { p.danger } else { p.text_muted };
            shield::paint(painter, lead_c, lead_w, g.preset, attention, color);
        }
        StatusLead::Dot => {
            painter.circle_filled(lead_c, 3.5, p.success);
        }
        StatusLead::Spinner => {
            progress::spinner(ui, lead_c, lead_w / 2.0 - 2.0, p.accent);
        }
    }
    // The glyph explains itself on hover.
    let glyph_rect = Rect::from_center_size(lead_c, vec2(lead_w + 4.0, line_h));
    let glyph = ui.interact(glyph_rect, id.with("shield"), Sense::hover());
    tooltip::attach(glyph, &shield::tooltip_text(g.preset, attention));

    let x = rect.left() + lead_w + space::S1 + 2.0;
    let left_color = if idle && attention {
        p.danger
    } else if active {
        p.text_secondary
    } else {
        p.text_muted
    };
    let right_g = right.map(|r| Type::Caption.numeric_galley(ui, &r, p.text_muted));
    let right_w = right_g.as_ref().map_or(0.0, |g| g.size().x + space::S2);
    let avail = rect.right() - x;
    // When the two ends do not fit, the "when" gives way.
    let left_g = Type::Caption.galley(ui, &left, left_color, avail, 1);
    let fits = left_g.size().x + right_w <= avail;
    painter.galley(pos2(x, y), left_g, left_color);
    if let (Some(rg), true) = (right_g, fits) {
        painter.galley(pos2(rect.right() - rg.size().x, y), rg, p.text_muted);
    }
}

/// The spotlight: one game given the room of a collection that holds nothing else. `rect`
/// is as tall as the cover; the text column takes the width that is left. `tags` are the
/// game's tags as (name, hue). Interaction is that of [`card`].
pub fn spotlight(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    g: &GameSummary,
    ctx: &CardCtx,
    tags: &[(&str, u8)],
) -> CardResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = interact(ui, rect, id, g, ctx);
    let mut out = CardResponse {
        play: resp.double_clicked(),
        response: resp,
        toggle_favorite: false,
        menu: None,
    };
    if !ui.is_rect_visible(rect) {
        return out;
    }
    let resp = out.response.clone();
    let hot = resp.contains_pointer() || resp.has_focus();
    let reveal =
        ui.ctx()
            .animate_bool_with_time(id.with("hot"), hot, theme.motion.fade(Dur::Quick));
    let pressed = if theme.motion.reduced {
        0.0
    } else {
        press_t(&resp, &theme)
    };
    let cover = Rect::from_min_size(
        rect.min,
        vec2(rect.height() / layout::COVER_ASPECT, rect.height()),
    );
    let drawn = scaled(cover, 1.0 - (1.0 - travel::PRESS) * 0.5 * pressed);
    let corner = CornerRadius::same(radius::LG);
    let mut painter = ui.painter_at(rect.expand(stroke::FOCUS_GAP + stroke::FOCUS_HC + 1.0));
    painter.multiply_opacity(ctx.opacity);
    paint_art(ui, &painter, drawn, corner, id, g, ctx, reveal, &theme);
    if ctx.multi_selected {
        paint_selection(&painter, drawn, corner, &theme);
    }

    // The text column.
    let x = cover.right() + space::S8;
    let w = (rect.right() - x).max(1.0);
    let mut y = rect.top() + space::S1;
    // The title is set as large as shows it whole in the room above the sentences and the
    // actions; only a title too long for the smallest size is cut (and then has a tooltip).
    let below = space::S2
        + Type::Body.line_height()
        + space::S5
        + Type::Body.line_height() * 3.0
        + space::S4
        + control::LG;
    let room = (cover.height() - space::S1 - below).max(0.0);
    let title = [Type::Hero, Type::Display, Type::Title]
        .into_iter()
        .map(|t| {
            let rows = ((room / t.line_height()).floor() as usize).clamp(1, 3);
            t.galley(ui, &g.name, p.text_primary, w, rows)
        })
        .enumerate()
        .find(|(i, galley)| !galley.elided || *i == 2)
        .map(|(_, galley)| galley);
    let Some(title) = title else {
        return out;
    };
    let title_rect = Rect::from_min_size(pos2(x, y), title.size());
    let elided = title.elided;
    painter.galley(title_rect.min, title, p.text_primary);
    y += title_rect.height() + space::S2;

    // One sentence about play: the state while there is one, else playtime.
    let running = matches!(g.activity, Activity::Running { .. } | Activity::Lingering);
    let sentence = strings::card::state(g.activity, g.availability, ctx.now)
        .or_else(|| (g.kind == EntryKind::Installer).then(|| strings::card::INSTALLER.to_owned()))
        .unwrap_or_else(|| {
            strings::card::played_sentence(
                g.total_playtime_ms,
                g.last_played_at,
                ctx.now,
                ctx.utc_offset_s,
            )
        });
    let mut sx = x;
    if running {
        painter.circle_filled(
            pos2(sx + 4.0, y + Type::Body.line_height() / 2.0),
            4.0,
            p.success,
        );
        sx += space::S3 + 2.0;
    }
    let line = ui.fonts_mut(|f| {
        let mut job = egui::text::LayoutJob::single_section(
            sentence,
            Type::Body.format(p.text_secondary, true),
        );
        job.wrap.max_width = (rect.right() - sx).max(1.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        f.layout_job(job)
    });
    let line_h = line.size().y;
    painter.galley(pos2(sx, y), line, p.text_secondary);
    y += line_h + space::S5;

    // One sentence about protection, led by the shield.
    let attention = attention(g);
    let shield_px = icon_size::BUTTON;
    let shield_c = pos2(x + shield_px / 2.0, y + Type::Body.line_height() / 2.0);
    shield::paint(
        &painter,
        shield_c,
        shield_px,
        g.preset,
        attention,
        if attention {
            p.danger
        } else {
            p.text_secondary
        },
    );
    let tx = x + shield_px + space::S2 + 2.0;
    let name = if attention {
        strings::card::NEEDS_ATTENTION
    } else {
        strings::shield::preset_name(g.preset)
    };
    let name_g = Type::BodyStrong.galley(
        ui,
        name,
        if attention { p.danger } else { p.text_primary },
        rect.right() - tx,
        1,
    );
    let name_h = name_g.size().y;
    painter.galley(pos2(tx, y), name_g, p.text_primary);
    y += name_h;
    let desc = Type::Body.galley(
        ui,
        &strings::shield::description(g.preset),
        p.text_secondary,
        (rect.right() - tx).min(layout::TEXT_MAX_W),
        2,
    );
    let desc_h = desc.size().y;
    painter.galley(pos2(tx, y), desc, p.text_secondary);
    y += desc_h + space::S4;

    // Actions, on the cover's baseline: Play, favourite, more.
    let actions_h = control::LG;
    let actions = Rect::from_min_max(
        pos2(x, rect.bottom() - actions_h),
        pos2(rect.right(), rect.bottom()),
    );
    // Tags, when there is room between the sentences and the actions.
    if !tags.is_empty() && y + control::CHIP_SM + space::S3 <= actions.top() {
        let row = Rect::from_min_max(pos2(x, y), pos2(rect.right(), y + control::CHIP_SM));
        let mut tags_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center)),
        );
        tags_ui.multiply_opacity(ctx.opacity);
        tags_ui.spacing_mut().item_spacing.x = space::S1 + 2.0;
        for (name, color) in tags {
            if tags_ui.available_width() < 80.0 {
                break;
            }
            chip(
                &mut tags_ui,
                ChipKind::Tag {
                    color: *color,
                    removable: false,
                },
                name,
                None,
                true,
            );
        }
    }
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(actions)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.multiply_opacity(ctx.opacity);
    row.spacing_mut().item_spacing.x = space::S2;
    if play_pill(&mut row, &play_state(g), &g.name, PillSize::Feature, 1.0).clicked() {
        out.play = true;
    }
    row.add_space(space::S1);
    let star = row.add(
        IconButton::new(
            if g.favorite {
                icons::STAR_FILL
            } else {
                icons::STAR
            },
            strings::card::FAVORITE,
        )
        .toggle(g.favorite)
        .size(control::MD)
        .shortcut("F"),
    );
    a11y::toggle(&star, &strings::card::favorite_name(&g.name), g.favorite);
    if star.clicked() {
        out.toggle_favorite = true;
    }
    let more = row.add(IconButton::new(icons::DOTS, strings::card::MORE).size(control::MD));
    a11y::button(&more, &strings::card::more_name(&g.name));
    if more.clicked() {
        out.menu = Some(more.rect.left_bottom() + vec2(0.0, space::S1));
    }

    if resp.has_focus() || ctx.selected {
        focus_ring(&painter, drawn, corner, &theme);
    }
    if elided {
        full_title(ui, &resp, title_rect, id, &g.name);
    }
    out
}
