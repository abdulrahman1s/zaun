//! The game card (UI_DESIGN_SYSTEM §7.2, SCREEN_SPECS §3, GAME_LIBRARY §2): painter-level
//! rendering into a rect the grid computed. Galleys go through egui's layout cache, keyed by
//! text, font and wrap width, so a steady grid lays out no text.

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, Availability, EntryKind, GameSummary};
use egui::accesskit::Role;
use egui::{
    Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, TextureId, Ui,
    WidgetText, pos2, vec2,
};

use super::button::{PillSize, PlayState, play_pill};
use super::cover::{CoverSource, paint_cover, paint_vertical_gradient};
use super::{focus_ring, progress, shield};
use crate::strings;
use crate::theme::palette::{black, fade};
use crate::theme::tokens::{alpha, control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Density, Dur, Type, icons};

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
    /// Controller mode: the pill is always visible.
    pub controller: bool,
    /// Show playtime and last played (`appearance.show_playtime_on_cards`).
    pub show_playtime: bool,
    /// Grid density.
    pub density: Density,
}

/// What the user did with a card this frame.
#[derive(Debug)]
pub struct CardResponse {
    /// The card's own response (click opens the game page).
    pub response: Response,
    /// Play pill clicked, or double-click on the card.
    pub play: bool,
    /// Favourite star toggled.
    pub toggle_favorite: bool,
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

/// Paint and interact with one card in `rect`. `id` must be stable per game.
pub fn card(ui: &mut Ui, rect: Rect, id: Id, g: &GameSummary, ctx: &CardCtx) -> CardResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = ui.interact(rect, id, Sense::click());
    super::own_arrow_keys(&resp);
    let state_text = strings::card::state(g.activity, g.availability, ctx.now);
    let state_text = state_text
        .or_else(|| (g.kind == EntryKind::Installer).then(|| strings::card::INSTALLER.to_owned()));
    let attention = g.flags.needs_attention || g.flags.suspected_tamper;
    let name = strings::card::accessible_name(
        &g.name,
        g.preset,
        state_text
            .as_deref()
            .or(attention.then_some(strings::card::NEEDS_ATTENTION)),
        g.total_playtime_ms,
        g.last_played_at,
        ctx.now,
        ctx.utc_offset_s,
    );
    let selected = ctx.selected;
    resp.widget_info(move || egui::WidgetInfo::selected(Role::Button, true, selected, &name));

    let mut play = resp.double_clicked();
    let mut toggle_favorite = false;
    if !ui.is_rect_visible(rect) {
        return CardResponse {
            response: resp,
            play,
            toggle_favorite,
        };
    }

    let hot = resp.hovered() || resp.has_focus() || ctx.controller;
    let hover_t =
        ui.ctx()
            .animate_bool_with_time(id.with("hot"), hot, theme.motion.fade(Dur::Fast));
    let corner = CornerRadius::same(radius::MD);
    let painter = ui.painter_at(rect.expand(stroke::FOCUS_GAP + stroke::FOCUS_HC + 1.0));

    // 1. Cover.
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
    paint_cover(&painter, rect, corner, source, &g.name, &theme);
    let dimmed = unavailable(g);
    if dimmed {
        painter.rect_filled(rect, corner, black(alpha::UNAVAILABLE_DIM));
        let glyph = if g.availability == Availability::DriveDisconnected {
            icons::USB
        } else {
            icons::LINK_BREAK
        };
        let c = pos2(rect.center().x, rect.top() + rect.height() * 0.40);
        painter.circle_filled(c, icon_size::EMPTY / 2.0 + space::S1, black(alpha::SCRIM));
        painter.text(
            c,
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::NAV + 4.0),
            p.on_art,
        );
    }

    // 2. Bottom gradient for legible text over any artwork.
    let grad_top = rect.bottom() - rect.height() * alpha::CARD_GRADIENT_SPAN;
    let r = f32::from(radius::MD);
    let solid_from = rect.bottom() - r;
    paint_vertical_gradient(
        &painter,
        Rect::from_min_max(pos2(rect.left(), grad_top), pos2(rect.right(), solid_from)),
        Color32::TRANSPARENT,
        black(alpha::CARD_GRADIENT),
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), solid_from), rect.right_bottom()),
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius::MD,
            se: radius::MD,
        },
        black(alpha::CARD_GRADIENT),
    );

    // 3. Border.
    let border = if hover_t > 0.0 {
        p.border_subtle.lerp_to_gamma(p.border_strong, hover_t)
    } else {
        p.border_subtle
    };
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, border),
        StrokeKind::Inside,
    );

    // 4. Shield (or the multi-select check) on a scrim disc, top-left.
    let pad = space::S2;
    let badge_c = pos2(
        rect.left() + pad + layout::SCRIM_DISC / 2.0,
        rect.top() + pad + layout::SCRIM_DISC / 2.0,
    );
    if ctx.multi_selected {
        painter.circle_filled(badge_c, layout::SCRIM_DISC / 2.0, p.accent);
        painter.text(
            badge_c,
            Align2::CENTER_CENTER,
            icons::CHECK.text(true),
            icons::font(icon_size::INLINE),
            p.text_on_accent,
        );
    } else {
        shield::paint_on_scrim(&painter, badge_c, g.preset, attention, &theme);
    }
    let shield_rect = Rect::from_center_size(badge_c, vec2(layout::SCRIM_DISC, layout::SCRIM_DISC));
    let shield_resp = ui.interact(shield_rect, id.with("shield"), Sense::hover());
    shield_resp.on_hover_text(shield::tooltip_text(g.preset, attention));

    // 5. Favourite star, top-right: always when set, else on hover/focus.
    let star_c = pos2(rect.right() - pad - layout::SCRIM_DISC / 2.0, badge_c.y);
    let star_rect = Rect::from_center_size(star_c, vec2(layout::SCRIM_DISC, layout::SCRIM_DISC));
    if g.favorite || hover_t > 0.0 {
        let star = ui.interact(star_rect, id.with("star"), Sense::CLICK);
        if star.clicked() {
            toggle_favorite = true;
        }
        let a = if g.favorite { 1.0 } else { hover_t };
        painter.circle_filled(
            star_c,
            layout::SCRIM_DISC / 2.0,
            fade(black(alpha::SCRIM), a),
        );
        let (glyph, color) = if g.favorite {
            (icons::STAR_FILL.text(true), p.warning)
        } else {
            (icons::STAR.text(false), p.on_art)
        };
        let color = if star.hovered() {
            color
        } else {
            fade(color, if g.favorite { 1.0 } else { 0.85 })
        };
        painter.text(
            star_c,
            Align2::CENTER_CENTER,
            glyph,
            icons::font(icon_size::INLINE + 1.0),
            fade(color, a),
        );
        star.on_hover_text(strings::card::FAVORITE);
    }

    // 6. Title and status line.
    let inner_l = rect.left() + space::S3;
    let pill_w = control::LG;
    let title_w = rect.width() - space::S3 * 2.0 - pill_w - space::S2;
    // Two lines at most: wrap, then truncate the job to two rows.
    let title = {
        let mut job = egui::text::LayoutJob::simple(
            g.name.to_string(),
            Type::BodyStrong.font_id(),
            p.on_art,
            title_w.max(1.0),
        );
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = false;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let show_status = ctx.density != Density::Compact;
    let running = matches!(g.activity, Activity::Running { .. } | Activity::Lingering);
    let status = if show_status {
        let text = state_text.clone().unwrap_or_else(|| {
            if ctx.show_playtime {
                strings::card::played(
                    g.total_playtime_ms,
                    g.last_played_at,
                    ctx.now,
                    ctx.utc_offset_s,
                )
            } else {
                String::new()
            }
        });
        (!text.is_empty()).then(|| {
            // The status line uses the full width; the pill, when shown, covers its end.
            let status_w = rect.width() - space::S3 * 2.0 - if running { space::S3 } else { 0.0 };
            WidgetText::from(text).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                status_w,
                Type::Caption.font_id(),
            )
        })
    } else {
        None
    };
    let mut y = rect.bottom() - space::S3;
    if let Some(s) = &status {
        y -= s.size().y;
        let mut x = inner_l;
        if running {
            painter.circle_filled(pos2(x + 3.5, y + s.size().y / 2.0), 3.5, p.success);
            x += space::S3;
        }
        let color = if attention && state_text.is_none() {
            p.danger
        } else {
            p.on_art_secondary
        };
        painter.galley(pos2(x, y), s.clone(), color);
        y -= space::S0;
    }
    y -= title.size().y;
    painter.galley(pos2(inner_l, y), title, p.on_art);

    // 7. Launch progress: thin indeterminate bar along the bottom edge.
    if matches!(g.activity, Activity::Launching(_) | Activity::Stopping) {
        let bar = Rect::from_min_max(
            pos2(rect.left() + r, rect.bottom() - control::PROGRESS - 1.0),
            pos2(rect.right() - r, rect.bottom() - 1.0),
        );
        progress::paint_linear(ui, bar, None, p.accent, fade(p.on_art, 0.15));
    }

    // 8. Play pill, bottom-right: fades in on hover/focus/controller.
    if hover_t > 0.0 {
        let pill = Rect::from_min_size(
            pos2(
                rect.right() - space::S2 - pill_w,
                rect.bottom() - space::S2 - pill_w,
            ),
            vec2(pill_w, pill_w),
        );
        let state = play_state(g);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(pill)
                .id_salt(id.with("pill")),
        );
        // Not a separate tab stop in the grid (Space / Ctrl+Enter play instead).
        let pr = play_pill(&mut child, &state, &g.name, PillSize::Card, hover_t);
        if pr.clicked() {
            play = true;
        }
    }

    // 9. Selection / focus ring outside the card.
    if resp.has_focus() || ctx.selected {
        focus_ring(&painter, rect, corner, &theme);
    }
    CardResponse {
        response: resp,
        play,
        toggle_favorite,
    }
}
