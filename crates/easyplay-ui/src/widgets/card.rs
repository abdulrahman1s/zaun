//! The card: a cover on the wall (UI_DESIGN_SYSTEM §7.2; the design's `.card`). The cover is
//! the whole card: nothing is written under it until it is pointed at or focused. Then it
//! rises 3 with the lift shadow, Play appears in its top right corner and a one-line
//! caption fades in under it ("Lantern Keep  12 h, yesterday"). A small library always
//! shows its captions, on two lines.
//!
//! A favourite carries a star badge where Play appears (Play takes that corner, the caption
//! then starts with the star); a selected cover a check in the other corner and the accent
//! ring. The card paints into the rect the wall computed and answers the pointer there;
//! galleys go through egui's layout cache, so a steady wall lays out no text.

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, Availability, EntryKind, GameSummary};
use egui::accesskit::Role;
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, Id, Painter, Rect, Response, Sense, Stroke, StrokeKind,
    TextureId, Ui, pos2, vec2,
};

use super::button::{PlayState, play_disc, play_disc_blocked};
use super::cover::{self, CoverLook, CoverSource};
use super::{focus_visible, press_t};
use crate::strings;
use crate::theme::motion::{self, scaled};
use crate::theme::tokens::{control, icon as icon_size, layout, stroke, travel};
use crate::theme::{self, Density, Dur, Theme, Type, icons};

/// Inputs the card needs besides the summary.
#[derive(Clone, Copy, Debug)]
pub struct CardCtx {
    /// Loaded cover texture, if any.
    pub texture: Option<TextureId>,
    /// Dominant colour while the texture loads.
    pub placeholder: Option<Color32>,
    /// Wall-clock now (for the session clock and relative days).
    pub now: Timestamp,
    /// Local UTC offset in seconds.
    pub utc_offset_s: i32,
    /// The wall's cursor is on this card (its accessible "selected" state).
    pub selected: bool,
    /// Part of a multi-selection: the accent ring and the check.
    pub multi_selected: bool,
    /// Controller mode: Play is always visible.
    pub controller: bool,
    /// In the `Tab` order (the wall is one tab stop, ACCESSIBILITY §2.1).
    pub focusable: bool,
    /// The caption says how long the game was played (`appearance.show_playtime_on_cards`).
    pub show_playtime: bool,
    /// Cover size preference.
    pub density: Density,
    /// Opacity of the whole card (a cover newly shown fades in).
    pub opacity: f32,
}

/// How the wall shows a card, besides the game itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallLook {
    /// A library of up to three games: Play always shown, 40 across, 10 in.
    pub big: bool,
    /// A library of up to six games: the caption always shown, title then meta.
    pub sparse: bool,
    /// The card's menu is open: it keeps its pointed-at look.
    pub menu_open: bool,
    /// The page's right edge less 20: a caption that would cross it hangs from the cover's
    /// right side instead.
    pub caption_edge: f32,
    /// The cover is in flight to or from its page: the card answers but paints nothing.
    pub hidden: bool,
}

impl Default for WallLook {
    fn default() -> Self {
        Self {
            big: false,
            sparse: false,
            menu_open: false,
            caption_edge: f32::INFINITY,
            hidden: false,
        }
    }
}

/// What the user did with a card this frame.
#[derive(Debug)]
pub struct CardResponse {
    /// The cover's own response (a click opens the game page).
    pub response: Response,
    /// Play was clicked.
    pub play: bool,
}

/// The state of Play for a summary (GAME_LIBRARY §2 card state rules). A Steam game is
/// always handed to Steam, which says itself what is wrong with it (ADR-0031): it is never
/// "Locate…" and never an installer. Draw Play with [`play_view`], which also knows that a
/// Steam game Steam runs has no Stop.
pub fn play_state(g: &GameSummary) -> PlayState {
    if g.is_steam() && g.activity == Activity::Idle {
        return PlayState::Ready;
    }
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

/// Play as it is drawn for a summary, everywhere it stands (the cover, a list row, the game
/// page, the pinned tab bar, the game menu).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayView {
    /// Play in this state.
    State(PlayState),
    /// The quiet Play that does nothing, with why: Steam runs this game, and EasyPlay has no
    /// Stop for it (ADR-0031).
    Blocked(&'static str),
}

/// Play for `g`: a Steam game that Steam runs shows the quiet, unpressable Play.
pub fn play_view(g: &GameSummary) -> PlayView {
    if steam_runs(g) {
        PlayView::Blocked(strings::steam::action::NO_STOP())
    } else {
        PlayView::State(play_state(g))
    }
}

/// A Steam game that Steam runs now (EasyPlay sees it run; it does not own it).
pub fn steam_runs(g: &GameSummary) -> bool {
    g.is_steam()
        && matches!(
            g.activity,
            Activity::Running { .. } | Activity::Lingering | Activity::Stopping
        )
}

/// The word that says how a game is protected: its level's name, or "Run by Steam" for a
/// game Steam starts, which has no level (ADR-0031, SECURITY_MODEL §10).
pub fn protection_word(g: &GameSummary) -> &'static str {
    if g.is_steam() {
        strings::steam::RUN_BY_STEAM()
    } else {
        strings::shield::preset_name(g.preset)
    }
}

fn unavailable(g: &GameSummary) -> bool {
    matches!(
        g.availability,
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
    )
}

/// A visible session clock changes every second: ask for the next frame at the next whole
/// second of the session (PERFORMANCE §8, 1 Hz while a game runs, nothing otherwise). Call
/// wherever elapsed time is shown.
pub fn tick_elapsed(ctx: &egui::Context, activity: Activity, now: Timestamp) {
    if let Activity::Running { since } = activity {
        let ms = now.since(since).map_or(0, |d| d.as_millis() as u64);
        ctx.request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
    }
}

/// The caption's meta for `g` ("12 h, yesterday", "Running 0:42", …).
pub fn caption_meta(g: &GameSummary, show_playtime: bool, now: Timestamp, offset_s: i32) -> String {
    strings::card::caption_meta(
        g.activity,
        g.availability,
        g.kind == EntryKind::Installer,
        g.total_playtime_ms,
        g.last_played_at,
        show_playtime,
        now,
        offset_s,
    )
}

/// Interaction and the accessible name.
fn interact(ui: &Ui, rect: Rect, id: Id, g: &GameSummary, ctx: &CardCtx) -> Response {
    let sense = if ctx.focusable {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let resp = ui.interact(rect, id, sense);
    super::own_arrow_keys(&resp);
    tick_elapsed(ui.ctx(), g.activity, ctx.now);
    let state_text = strings::card::state(g.activity, g.availability, ctx.now).or_else(|| {
        (g.kind == EntryKind::Installer).then(|| strings::card::INSTALLER().to_owned())
    });
    let attention = g.flags.needs_attention || g.flags.suspected_tamper;
    let name = strings::card::accessible_name(
        &g.name,
        protection_word(g),
        state_text
            .as_deref()
            .or(attention.then_some(strings::card::NEEDS_ATTENTION())),
        g.total_playtime_ms,
        g.last_played_at,
        ctx.now,
        ctx.utc_offset_s,
    );
    let selected = ctx.selected || ctx.multi_selected;
    resp.widget_info(move || egui::WidgetInfo::selected(Role::Button, true, selected, &name));
    resp
}

/// The accent ring of a selected or focused cover: 3 outside (2 pt; 3 pt in high contrast).
fn ring(painter: &Painter, cover: Rect, theme: &Theme) {
    let w = theme.focus_width();
    let grow = stroke::COVER_RING_GAP + w / 2.0;
    let r = cover::corner(cover).nw.saturating_add(grow.round() as u8);
    painter.rect_stroke(
        cover.expand(grow),
        CornerRadius::same(r),
        Stroke::new(w, theme.palette.accent_mark),
        StrokeKind::Middle,
    );
}

/// A 20 round badge in a corner of `cover`: `fill` with `glyph` in `ink`.
fn badge(painter: &Painter, cover: Rect, left: bool, fill: Color32, glyph: String, ink: Color32) {
    let inset = control::DISC_INSET;
    let half = control::BADGE / 2.0;
    let x = if left {
        cover.left() + inset + half
    } else {
        cover.right() - inset - half
    };
    let c = pos2(x, cover.top() + inset + half);
    painter.circle_filled(c, half, fill);
    painter.text(
        c,
        Align2::CENTER_CENTER,
        glyph,
        icons::font(icon_size::TINY),
        ink,
    );
}

/// A caption line laid out at 13 pt in a 16 line box.
fn caption_job(title: &str, meta: &str, star: bool, theme: &Theme, wrap: Option<f32>) -> LayoutJob {
    let p = &theme.palette;
    let mut job = LayoutJob::default();
    let mut strong = Type::BodyStrong.format(p.t1, false);
    strong.line_height = Some(layout::CAPTION_LINE);
    let lead = if star {
        icon_size::TINY + layout::CAPTION_STAR_GAP
    } else {
        0.0
    };
    job.append(&theme::typography::shaped(title), lead, strong);
    let mut quiet = Type::Body.format(p.t3, true);
    quiet.line_height = Some(layout::CAPTION_LINE);
    match wrap {
        // The small library's caption: the title on its own lines, the meta under it.
        Some(w) => {
            job.append("\n", 0.0, quiet.clone());
            job.append(meta, 0.0, quiet);
            job.wrap.max_width = w.max(1.0);
        }
        None => {
            job.append(meta, layout::CAPTION_META_GAP, quiet);
            job.wrap.max_width = f32::INFINITY;
        }
    }
    job
}

/// Paint and answer one card whose cover occupies `cover` (2:3). `id` must be stable per
/// game. The caption hangs under the cover, outside `cover`.
pub fn wall_card(
    ui: &mut Ui,
    cover: Rect,
    id: Id,
    g: &GameSummary,
    ctx: &CardCtx,
    look: &WallLook,
) -> CardResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = interact(ui, cover, id, g, ctx);
    let mut out = CardResponse {
        play: false,
        response: resp.clone(),
    };
    let reach = Rect::from_min_max(
        cover.min,
        pos2(
            cover.right(),
            cover.bottom() + layout::CAPTION_GAP + layout::CAPTION_LINE * 4.0,
        ),
    );
    if !ui.is_rect_visible(reach.expand(control::DISC)) {
        return out;
    }
    let keyboard = resp.has_focus() && focus_visible(ui.ctx());
    let hot = resp.contains_pointer() || keyboard || look.menu_open;
    let shade_t =
        ui.ctx()
            .animate_bool_with_time(id.with("shade"), hot, theme.motion.fade(Dur::Fast));
    let reduced = theme.motion.reduced;
    let lift = motion::glide(
        ui.ctx(),
        id.with("lift"),
        if hot && !reduced { 1.0 } else { 0.0 },
        theme.motion.movement(Dur::Fast),
    );
    let pressed = if reduced { 0.0 } else { press_t(&resp, &theme) };
    let rise =
        layout::COVER_LIFT * lift - (layout::COVER_LIFT - travel::COVER_PRESS_LIFT) * pressed;
    let drawn = scaled(
        cover.translate(vec2(0.0, -rise)),
        1.0 - (1.0 - travel::COVER_PRESS) * pressed,
    );
    let opacity = ctx.opacity;
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);

    let running = matches!(
        g.activity,
        Activity::Running { .. }
            | Activity::Lingering
            | Activity::Launching(_)
            | Activity::Stopping
    );
    let selected = ctx.multi_selected;
    if !look.hidden {
        cover::paint_shadow(&painter, drawn, p.shade_lift, shade_t);
        let source = match (ctx.texture, ctx.placeholder) {
            (Some(t), _) => {
                let t_in = ui.ctx().animate_bool_with_time(
                    id.with("tex"),
                    true,
                    theme.motion.fade(Dur::Base),
                );
                CoverSource::Texture(t, t_in)
            }
            (None, Some(c)) => CoverSource::Pending(c),
            (None, None) => CoverSource::Placeholder,
        };
        cover::paint(
            &painter,
            drawn,
            source,
            &g.name,
            CoverLook {
                missing: unavailable(g),
                // EasyPlay's mark (a shield) is not put on a game Steam runs (ADR-0031).
                mark: !selected && !g.is_steam(),
                opacity: 1.0,
            },
            &theme,
        );
        // The star yields to Play: Play takes that corner whenever it shows.
        let play_corner = hot || look.big || running || ctx.controller;
        if g.favorite && !play_corner {
            badge(
                &painter,
                drawn,
                false,
                p.badge,
                icons::STAR.text(true),
                p.on_art,
            );
        }
        if selected {
            badge(
                &painter,
                drawn,
                true,
                p.fill,
                icons::CHECK.text(false),
                p.on_fill,
            );
        }
        if selected || keyboard {
            ring(&painter, drawn, &theme);
        }
    }

    // Play: top right, on hover or focus; always on a small library's covers, while the game
    // runs or starts, and in controller mode.
    let shown = hot || look.big || running || ctx.controller;
    let reveal =
        ui.ctx()
            .animate_bool_with_time(id.with("play"), shown, theme.motion.fade(Dur::Fast));
    let (size, inset) = if look.big {
        (control::DISC_BIG, control::DISC_BIG_INSET)
    } else {
        (control::DISC, control::DISC_INSET)
    };
    let disc = Rect::from_min_size(
        pos2(drawn.right() - inset - size, drawn.top() + inset),
        vec2(size, size),
    );
    let reveal = if look.hidden { 0.0 } else { reveal * opacity };
    match play_view(g) {
        PlayView::State(st) => {
            if play_disc(ui, disc, id.with("play"), &st, &g.name, reveal).clicked() {
                out.play = true;
            }
        }
        // Steam runs it: the quiet disc, which says so and does nothing.
        PlayView::Blocked(why) => {
            let _ = play_disc_blocked(ui, disc, id.with("play"), &g.name, why, reveal);
        }
    }

    // The caption: under the cover (which does not rise with it).
    let cap_t = if look.sparse {
        1.0
    } else {
        ui.ctx()
            .animate_bool_with_time(id.with("caption"), hot, theme.motion.fade(Dur::Fast))
    };
    if cap_t > 0.0 && !look.hidden {
        let meta = caption_meta(g, ctx.show_playtime, ctx.now, ctx.utc_offset_s);
        let wrap = look.sparse.then_some(cover.width());
        let job = caption_job(&g.name, &meta, g.favorite, &theme, wrap);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let w = galley.size().x;
        let top = cover.bottom() + layout::CAPTION_GAP;
        let x = if !look.sparse && cover.left() + w > look.caption_edge {
            cover.right() - w
        } else {
            cover.left()
        };
        let a = cap_t * opacity;
        // The caption is not clipped to the cover: it may run wider.
        let mut cap_painter = ui.painter().clone();
        cap_painter.multiply_opacity(a);
        if g.favorite {
            cap_painter.text(
                pos2(x + icon_size::TINY / 2.0, top + layout::CAPTION_LINE / 2.0),
                Align2::CENTER_CENTER,
                icons::STAR.text(true),
                icons::font(icon_size::TINY),
                p.t1,
            );
        }
        cap_painter.galley(pos2(x, top), galley, p.t1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::theme::Appearance;

    #[test]
    fn play_follows_the_run_state() {
        let lib = demo::library(demo::baseline_now());
        let state = |name: &str| {
            play_state(
                lib.games
                    .iter()
                    .find(|g| &*g.name == name)
                    .unwrap_or(&lib.games[0]),
            )
        };
        assert_eq!(state("Starfall Tactics"), PlayState::Running);
        assert!(matches!(state("Harbor Lights"), PlayState::Launching(_)));
        assert_eq!(state("Paper Kingdoms"), PlayState::Unavailable);
        assert_eq!(state("Setup Glassworks"), PlayState::Installer);
        assert_eq!(state("Lantern Keep"), PlayState::Ready);
    }

    #[test]
    fn captions_say_what_the_design_says() {
        let now = demo::baseline_now();
        let lib = demo::library(now);
        let meta = |name: &str, playtime: bool| {
            let g = lib.games.iter().find(|g| &*g.name == name);
            g.map(|g| caption_meta(g, playtime, now, 0))
                .unwrap_or_default()
        };
        assert_eq!(meta("Lantern Keep", true), "12 h, yesterday");
        assert_eq!(meta("Lantern Keep", false), "Yesterday");
        assert_eq!(meta("Vale of Cinders", true), "Not played yet");
        assert_eq!(meta("Paper Kingdoms", true), "Not found");
        assert_eq!(meta("Setup Glassworks", true), "Installer");
        assert_eq!(meta("Starfall Tactics", true), "Running 42:10");
        assert_eq!(meta("Harbor Lights", true), "Starting…");
        assert_eq!(meta("Low Tide Motel", true), "3 h 30 min, 2 days ago");
    }

    #[test]
    fn a_steam_game_steam_runs_has_no_stop() {
        let now = demo::baseline_now();
        let games = demo::steam_samples(now);
        let running = games
            .iter()
            .find(|g| g.activity != Activity::Idle)
            .expect("a running sample");
        assert_eq!(
            play_view(running),
            PlayView::Blocked(strings::steam::action::NO_STOP())
        );
        let idle = games.iter().find(|g| g.activity == Activity::Idle);
        assert!(idle.is_some_and(|g| play_view(g) == PlayView::State(PlayState::Ready)));
        // A Steam game is never "Locate…": Steam says what is wrong.
        let mut gone = games[0].clone();
        gone.availability = Availability::Missing;
        assert_eq!(play_state(&gone), PlayState::Ready);
        assert_eq!(protection_word(&gone), strings::steam::RUN_BY_STEAM());
        let name = strings::card::accessible_name(
            &gone.name,
            protection_word(&gone),
            None,
            gone.total_playtime_ms,
            gone.last_played_at,
            now,
            0,
        );
        assert!(
            name.contains("Run by Steam") && !name.contains("Standard"),
            "{name}"
        );
    }

    #[test]
    fn a_steam_game_with_time_but_no_date_is_not_unplayed() {
        let now = demo::baseline_now();
        let mut g = demo::steam_samples(now)[0].clone();
        g.last_played_at = None;
        assert_eq!(caption_meta(&g, true, now, 0), "74 h");
        assert_eq!(caption_meta(&g, false, now, 0), strings::card::PLAYED());
        let name = strings::card::accessible_name(
            &g.name,
            protection_word(&g),
            None,
            g.total_playtime_ms,
            None,
            now,
            0,
        );
        assert!(
            name.contains("Played 74 hours") && !name.contains("Not played"),
            "{name}"
        );
        g.total_playtime_ms = 0;
        assert_eq!(caption_meta(&g, true, now, 0), strings::card::NOT_PLAYED());
    }

    #[test]
    fn a_card_answers_on_its_cover() {
        let lib = demo::library(demo::baseline_now());
        let g = lib.games[0].clone();
        let rect = crate::widgets::test_frame(&Appearance::default(), |ui| {
            let cover = Rect::from_min_size(pos2(10.0, 10.0), vec2(164.0, 246.0));
            let ctx = CardCtx {
                texture: None,
                placeholder: None,
                now: demo::baseline_now(),
                utc_offset_s: 0,
                selected: false,
                multi_selected: true,
                controller: false,
                focusable: true,
                show_playtime: true,
                density: Density::Comfortable,
                opacity: 1.0,
            };
            wall_card(ui, cover, Id::unique("c"), &g, &ctx, &WallLook::default())
                .response
                .rect
        });
        assert_eq!(rect.size(), vec2(164.0, 246.0));
    }
}
