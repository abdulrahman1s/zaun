//! The virtualized cover grid (PERFORMANCE §5, SCREEN_SPECS §2–3). Rows (card rows and group
//! headers) have precomputed offsets; only rows intersecting the viewport (plus one row of
//! overscan) are laid out, and covers of the next screenful are requested ahead. Arrow keys
//! move the cursor spatially (ACCESSIBILITY §2.1, `input::focus`); `Enter` opens, `Space`
//! plays; `Shift` extends the selection; `Ctrl+click` toggles; right-click opens the context
//! menu. Only one card is in the `Tab` order (roving focus), so `Tab` leaves the grid.
//!
//! The grid lives inside the library's scroll area, below the page heading. Its geometry
//! follows the size of the collection, not of the current search: a collection too small to
//! fill a row gets larger covers, and a collection of exactly one game becomes a spotlight.
//! When the order changes (search, filter, sort), the cards on screen glide to their new
//! places and new arrivals fade in.

use std::collections::HashMap;
use std::sync::Arc;

use easyplay_core::ids::GameId;
use easyplay_core::library::GroupSpan;
use egui::accesskit::Role;
use egui::mutex::Mutex;
use egui::{Id, Key, Modifiers, Pos2, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::focus;
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::motion::ease_standard;
use crate::theme::tokens::{layout, space, travel};
use crate::theme::{self, Density, Dur, Type};
use crate::vm::library::Collection;
use crate::widgets::card::{CardCtx, caption_height, card, spotlight};
use crate::widgets::skeleton;

/// Grid geometry for a width (PERFORMANCE §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    /// Columns.
    pub cols: usize,
    /// Card width (the spotlight's whole width when `spotlight`).
    pub card_w: f32,
    /// Cover height (2:3).
    pub cover_h: f32,
    /// Caption height beneath the cover.
    pub caption_h: f32,
    /// Gap between columns.
    pub gap: f32,
    /// Gap between a caption and the next row.
    pub row_gap: f32,
    /// The collection holds one game: a single spotlight cell.
    pub spotlight: bool,
}

impl GridLayout {
    /// Compute for `avail_w` at `density`; `compact` uses the smaller gap. `total` is the
    /// size of the collection being shown (before search and filters), `viewport_h` the
    /// height the spotlight may take.
    pub fn new(
        avail_w: f32,
        density: Density,
        compact: bool,
        total: usize,
        viewport_h: f32,
    ) -> Self {
        let gap = if compact {
            layout::CARD_GAP_COMPACT
        } else {
            layout::CARD_GAP
        };
        if total == 1 {
            // The cover takes about a third of the width, as tall as the viewport allows.
            let by_width = (avail_w * 0.34).min(layout::SPOTLIGHT_COVER_MAX);
            let by_height = (viewport_h - space::S8) / layout::COVER_ASPECT;
            let cover_w = by_width
                .min(by_height)
                .max(layout::SPOTLIGHT_COVER_MIN)
                .round();
            let text = (avail_w - cover_w - space::S8).clamp(0.0, layout::SPOTLIGHT_TEXT_MAX);
            return Self {
                cols: 1,
                card_w: (cover_w + space::S8 + text).min(avail_w),
                cover_h: cover_w * layout::COVER_ASPECT,
                caption_h: 0.0,
                gap,
                row_gap: 0.0,
                spotlight: true,
            };
        }
        let min = density.card_min_width();
        let fit = (((avail_w + gap) / (min + gap)).floor() as usize).max(1);
        let stretched = |cols: usize| (avail_w - gap * (cols as f32 - 1.0)) / cols as f32;
        let (cols, card_w) = if total > 0 && total < fit {
            // A sparse shelf: larger covers, never stretched across the page.
            let w = stretched(total).min((min * layout::CARD_SPARSE_SCALE).round());
            let cols = (((avail_w + gap) / (w + gap)).floor() as usize).max(1);
            (cols, w)
        } else {
            (fit, stretched(fit))
        };
        let card_w = card_w.max(1.0);
        Self {
            cols,
            card_w,
            cover_h: card_w * layout::COVER_ASPECT,
            caption_h: caption_height(density),
            gap,
            row_gap: layout::CARD_ROW_GAP,
            spotlight: false,
        }
    }

    /// Height of one card: cover and caption.
    pub fn card_h(&self) -> f32 {
        self.cover_h + self.caption_h
    }

    /// Row pitch.
    pub fn row_h(&self) -> f32 {
        self.card_h() + self.row_gap
    }

    /// Rows for `n` items.
    pub fn rows(&self, n: usize) -> usize {
        n.div_ceil(self.cols)
    }
}

/// Height of a group header row.
const HEADER_H: f32 = layout::GROUP_HEADER_H;

/// One laid-out row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A group header (index into the spans).
    Header(usize),
    /// Cards `start..start + len` of the visible list.
    Cards {
        /// First position.
        start: usize,
        /// Count.
        len: usize,
    },
}

/// Rows with their top offsets, and the total height.
pub fn layout_rows(g: &GridLayout, n: usize, groups: &[GroupSpan]) -> (Vec<(Row, f32)>, f32) {
    let mut rows = Vec::new();
    let mut y = 0.0;
    let push_cards = |rows: &mut Vec<(Row, f32)>, y: &mut f32, start: usize, len: usize| {
        let mut s = start;
        while s < start + len {
            let l = g.cols.min(start + len - s);
            rows.push((Row::Cards { start: s, len: l }, *y));
            *y += g.row_h();
            s += l;
        }
    };
    if groups.is_empty() {
        push_cards(&mut rows, &mut y, 0, n);
    } else {
        for (gi, span) in groups.iter().enumerate() {
            rows.push((Row::Header(gi), y));
            y += HEADER_H;
            push_cards(&mut rows, &mut y, span.start as usize, span.len as usize);
        }
    }
    (rows, (y - g.row_gap).max(0.0))
}

/// Twelve card skeletons, before the first snapshot.
pub fn skeleton(ui: &mut Ui, density: Density) {
    let compact = ui.ctx().content_rect().width() < layout::COMPACT_BELOW;
    let g = GridLayout::new(ui.available_width(), density, compact, 12, 0.0);
    let rows = g.rows(12);
    let (area, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), rows as f32 * g.row_h()),
        Sense::hover(),
    );
    for i in 0..12 {
        let (r, c) = (i / g.cols, i % g.cols);
        let rect = Rect::from_min_size(
            pos2(
                area.left() + c as f32 * (g.card_w + g.gap),
                area.top() + r as f32 * g.row_h(),
            ),
            vec2(g.card_w, g.card_h()),
        );
        if ui.is_rect_visible(rect) {
            skeleton::card(ui, rect, density);
        }
    }
}

/// The stable id of a game's card.
pub fn card_id(game: GameId) -> Id {
    Id::unique(("card", game))
}

/// What the grid remembers between frames to animate a change of order.
#[derive(Default)]
struct Flow {
    /// The order generation the cards were last laid out for.
    generation: u64,
    /// What a change of order must leave untouched to be a reflow rather than a new layout.
    shape: Option<(Collection, usize, u32)>,
    /// Where each card on screen was drawn last frame, relative to the grid's origin.
    drawn: HashMap<GameId, Pos2>,
    /// Where the cards were when the order last changed.
    from: HashMap<GameId, Pos2>,
    /// When the order last changed.
    started: f64,
    /// When the library first appeared (the rows arrive one after another, once).
    intro: Option<f64>,
}

fn flow(ctx: &egui::Context) -> Arc<Mutex<Flow>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<Flow>>>(Id::unique("library.grid.flow"))
            .clone()
    })
}

/// The library was first shown some other way (as a list): the grid's first appearance
/// then is a change of view, not an arrival, and its rows do not come in one by one.
pub fn skip_intro(ctx: &egui::Context) {
    flow(ctx).lock().intro.get_or_insert(f64::NEG_INFINITY);
}

/// Show the grid; returns what the user asked for. `total` is the size of the collection
/// (before search and filters), `room_h` the height of the viewport below the page heading
/// (what a spotlight may fill).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
    groups: &[GroupSpan],
    total: usize,
    room_h: f32,
) -> Option<(GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let compact = ui.ctx().content_rect().width() < layout::COMPACT_BELOW;
    let avail_w = ui.available_width();
    let clip = ui.clip_rect();
    let g = GridLayout::new(avail_w, theme.density, compact, total, room_h);
    let (rows, total_h) = layout_rows(&g, visible.len(), groups);
    let card_rows: Vec<(usize, usize)> = rows
        .iter()
        .filter_map(|(r, _)| match r {
            Row::Cards { start, len } => Some((*start, *len)),
            Row::Header(_) => None,
        })
        .collect();
    let mut action = None;
    let ppp = ui.ctx().pixels_per_point();
    let cover_w = if g.spotlight {
        g.cover_h / layout::COVER_ASPECT
    } else {
        g.card_w
    };
    let thumb = ThumbSize::cover_for(cover_w * ppp);
    let show_playtime = env.bridge.prefs().appearance.show_playtime_on_cards;
    let game_at = |pos: usize| lib.games[visible[pos] as usize].id;

    // Keyboard: arrow navigation over the visible order.
    let sel_pos = state
        .library
        .selected
        .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
    let focused = ui.ctx().memory(|m| m.focused());
    let grid_has_focus = sel_pos.is_some_and(|p| focused == Some(card_id(game_at(p))));
    state.library.focused = grid_has_focus;
    let mut scroll_to: Option<usize> = None;
    if grid_has_focus && let Some(pos) = sel_pos {
        let (mv, enter, play) = ui.input_mut(|i| {
            (
                focus::take_move(i),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Space),
            )
        });
        let page = ((clip.height() / g.row_h().max(1.0)).floor() as usize).max(1);
        if let Some((mv, shift)) = mv {
            if let Some(t) = focus::target(&card_rows, pos, mv, page) {
                let id = game_at(t);
                if shift {
                    if state.library.anchor.is_none() {
                        state.library.anchor = Some(game_at(pos));
                    }
                    state.library.select_range(lib, visible, id);
                }
                state.library.selected = Some(id);
                state.pending_focus = Some(id);
                scroll_to = Some(t);
            }
        } else if enter {
            action = Some((game_at(pos), GameAction::Open));
        } else if play {
            action = Some((game_at(pos), GameAction::Play));
        }
    }
    // A pending focus outside the viewport scrolls it into view first.
    if scroll_to.is_none()
        && let Some(pf) = state.pending_focus
        && let Some(t) = visible.iter().position(|i| lib.games[*i as usize].id == pf)
    {
        scroll_to = Some(t);
    }

    // Order changes: the cards on screen travel from where they were drawn.
    let now = ui.input(|i| i.time);
    let flow = flow(ui.ctx());
    let mut flow = flow.lock();
    let shape = (state.library.collection, g.cols, g.card_w.to_bits());
    let generation = state.library.generation();
    let travel_secs = theme.motion.movement(Dur::Glide);
    if flow.shape != Some(shape) {
        flow.from.clear();
    } else if flow.generation != generation && travel_secs > 0.0 {
        flow.from = std::mem::take(&mut flow.drawn);
        flow.started = now;
    }
    flow.shape = Some(shape);
    flow.generation = generation;
    let travel_t = if flow.from.is_empty() || travel_secs <= 0.0 {
        1.0
    } else {
        ((now - flow.started) as f32 / travel_secs).clamp(0.0, 1.0)
    };
    if travel_t >= 1.0 {
        flow.from.clear();
    } else {
        ui.ctx().request_repaint();
    }
    let travel_e = ease_standard(travel_t);
    // The first appearance: rows arrive one after another.
    let intro_secs = theme.motion.movement(Dur::Page);
    let intro_start = *flow.intro.get_or_insert(now);
    let intro_end = intro_secs + travel::STAGGER * travel::STAGGER_ROWS as f32;
    let intro_at = (now - intro_start) as f32;
    let intro_running = intro_secs > 0.0 && intro_at < intro_end;
    if intro_running {
        ui.ctx().request_repaint();
    }
    flow.drawn.clear();

    let grid_name = strings::library::grid_name(visible.len(), g.cols);
    let selection_active = !state.library.selection.is_empty();
    ui.scope_builder(UiBuilder::new().id_salt("library.grid"), |ui| {
        a11y::labelled(ui.ctx(), ui.scope_id(), Role::Grid, &grid_name);
        let (area, _) = ui.allocate_exact_size(vec2(avail_w, total_h), Sense::hover());
        let origin = area.min;
        let view_top = clip.top() - origin.y;
        let view_bottom = clip.bottom() - origin.y;
        let view_h = view_bottom - view_top;
        let cell = |row_y: f32, col: usize| {
            Rect::from_min_size(
                pos2(origin.x + col as f32 * (g.card_w + g.gap), origin.y + row_y),
                vec2(g.card_w, g.card_h()),
            )
        };
        if let Some(t) = scroll_to
            && let Some((Row::Cards { start, .. }, y)) = rows.iter().find(
                |(r, _)| matches!(r, Row::Cards { start, len } if t >= *start && t < start + len),
            )
        {
            ui.scroll_to_rect(cell(*y, t - start).expand(space::S6), None);
        }
        // First row whose bottom is below the viewport top, minus one row of overscan.
        let first = rows
            .partition_point(|(_, y)| *y + g.row_h() < view_top)
            .saturating_sub(1);
        // The cells laid out this frame, and the one in the `Tab` order.
        let mut laid_out: Option<(usize, usize)> = None;
        for (row, y) in rows.iter().skip(first) {
            if *y > view_bottom + g.row_h() {
                break;
            }
            if let Row::Cards { start, len } = row {
                let (a, b) = laid_out.unwrap_or((*start, start + len));
                laid_out = Some((a.min(*start), b.max(start + len)));
            }
        }
        let pending_pos = state
            .pending_focus
            .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
        let laid_out = laid_out.map_or(0..0, |(a, b)| a..b);
        let rover = focus::rover(pending_pos.or(sel_pos), laid_out);
        // A page in flight shows what is cached and asks for nothing.
        let jumped = super::page_jumped(ui, view_top, view_h);
        let prefetch_to = if jumped {
            view_bottom + g.row_h()
        } else {
            view_bottom + view_h.max(0.0)
        };
        // How far a card may glide when the order changes: to the next slot or the next row.
        let reach = (g.card_w + g.gap).max(g.row_h()) * 1.05;
        let mut card_row = 0_usize;
        for (row, y) in rows.iter().skip(first) {
            if *y > prefetch_to {
                break;
            }
            let on_screen = *y <= view_bottom + g.row_h();
            match row {
                Row::Header(gi) => {
                    if !on_screen {
                        continue;
                    }
                    let span = &groups[*gi];
                    let base = origin.y + y + HEADER_H - space::S3;
                    let label = Type::Heading.galley(ui, &span.label, p.text_primary, avail_w, 1);
                    let label_w = label.size().x;
                    ui.painter().galley(
                        pos2(origin.x, base - label.size().y),
                        label,
                        p.text_primary,
                    );
                    let count = Type::Body.numeric_galley(ui, &span.len.to_string(), p.text_muted);
                    ui.painter().galley(
                        pos2(origin.x + label_w + space::S2, base - count.size().y - 1.0),
                        count,
                        p.text_muted,
                    );
                }
                Row::Cards { start, len } => {
                    // Rows of the first appearance arrive in turn.
                    let arrive = if intro_running && card_row < travel::STAGGER_ROWS {
                        let local = (intro_at - travel::STAGGER * card_row as f32) / intro_secs;
                        ease_standard(local.clamp(0.0, 1.0))
                    } else {
                        1.0
                    };
                    if on_screen {
                        card_row += 1;
                    }
                    for k in 0..*len {
                        let pos = start + k;
                        let game = &lib.games[visible[pos] as usize];
                        if !on_screen {
                            // Prefetch: ask for the cover without drawing.
                            if let Some(art) = game.cover {
                                let _ =
                                    env.textures
                                        .get(TexKey { art, size: thumb }, false, env.art);
                            }
                            continue;
                        }
                        let target = cell(*y, k);
                        let rel = (target.min - origin).to_pos2();
                        // A card a slot or a row away glides there; one that would have
                        // to cross the shelf, or was not on screen, fades in at its place.
                        let (rel_now, entering) = if travel_t < 1.0 {
                            match flow.from.get(&game.id) {
                                Some(from) if (*from - rel).length() <= reach => {
                                    (from.lerp(rel, travel_e), 1.0)
                                }
                                _ => (rel, travel_e),
                            }
                        } else {
                            (rel, 1.0)
                        };
                        flow.drawn.insert(game.id, rel_now);
                        let rise = (1.0 - arrive) * travel::RISE;
                        let rect = Rect::from_min_size(
                            origin + rel_now.to_vec2() + vec2(0.0, rise),
                            target.size(),
                        );
                        let (texture, placeholder) = match game.cover {
                            Some(art) => {
                                let key = TexKey { art, size: thumb };
                                let state = if jumped {
                                    env.textures.peek(key, true)
                                } else {
                                    env.textures.get(key, true, env.art)
                                };
                                match state {
                                    TexState::Ready(t) => (Some(t), None),
                                    TexState::Pending(c) => (None, c),
                                }
                            }
                            None => (None, None),
                        };
                        let selected = state.library.selected == Some(game.id);
                        let ctx = CardCtx {
                            texture,
                            placeholder,
                            now: env.now,
                            utc_offset_s: env.utc_offset_s,
                            selected: selected && grid_has_focus,
                            multi_selected: state.library.is_selected(game.id),
                            controller: state.controller,
                            focusable: rover == Some(pos),
                            show_playtime,
                            density: theme.density,
                            opacity: entering * arrive,
                        };
                        let id = card_id(game.id);
                        let r = if g.spotlight {
                            let tags: Vec<(&str, u8)> = game
                                .tags
                                .iter()
                                .filter_map(|t| lib.tag(*t))
                                .map(|t| (&*t.name, t.color.index()))
                                .collect();
                            let rect = Rect::from_min_size(rect.min, vec2(g.card_w, g.cover_h));
                            spotlight(ui, rect, id, game, &ctx, &tags)
                        } else {
                            card(ui, rect, id, game, &ctx)
                        };
                        state
                            .dnd
                            .target(rect, crate::input::dnd::DropTarget::Cover(game.id));
                        if rover != Some(pos) && focus::accesskit_focus_requested(ui.ctx(), id) {
                            state.pending_focus = Some(game.id);
                            ui.ctx().request_repaint();
                        }
                        if state.pending_focus == Some(game.id) {
                            r.response.request_focus();
                            state.pending_focus = None;
                        }
                        if r.response.has_focus() {
                            state.library.selected = Some(game.id);
                            state.library.cursor_rect = Some(rect);
                        }
                        if r.response.secondary_clicked() {
                            let at = r.response.interact_pointer_pos().unwrap_or(rect.center());
                            super::open_menu(state, game.id, at);
                        } else if let Some(at) = r.menu {
                            super::open_menu(state, game.id, at);
                        } else if r.toggle_favorite {
                            action = Some((game.id, GameAction::Favorite));
                        } else if r.play {
                            action = Some((game.id, GameAction::Play));
                        } else if r.response.clicked() {
                            let m = ui.input(|i| i.modifiers);
                            if m.command {
                                state.library.toggle_select(game.id);
                            } else if m.shift {
                                state.library.select_range(lib, visible, game.id);
                            } else if selection_active {
                                state.library.toggle_select(game.id);
                            } else {
                                action = Some((game.id, GameAction::Open));
                            }
                        }
                    }
                }
            }
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regular(avail: f32, total: usize) -> GridLayout {
        GridLayout::new(avail, Density::Comfortable, false, total, 600.0)
    }

    #[test]
    fn layout_maths() {
        let g = regular(1000.0, 24);
        // floor((1000 + 20) / (168 + 20)) = 5
        assert_eq!(g.cols, 5);
        assert!((g.card_w * 5.0 + 20.0 * 4.0 - 1000.0).abs() < 0.01);
        assert!((g.cover_h - g.card_w * 1.5).abs() < 0.01);
        assert!(g.card_h() > g.cover_h);
        assert_eq!(g.rows(11), 3);
        let narrow = GridLayout::new(100.0, Density::Large, true, 24, 600.0);
        assert_eq!(narrow.cols, 1);
    }

    /// A collection too small to fill a row shows larger covers at their natural places;
    /// the size depends on the collection, so a search never resizes the cards.
    #[test]
    fn sparse_collections_get_larger_covers_without_stretching() {
        let full = regular(1000.0, 24);
        let three = regular(1000.0, 3);
        assert!(!three.spotlight);
        assert!(three.card_w > full.card_w);
        assert!(three.card_w <= (168.0_f32 * layout::CARD_SPARSE_SCALE).round());
        // The cards still sit on a grid that starts at the left edge.
        assert!(three.cols >= 3);
        // A row that the collection fills exactly is a normal row.
        assert_eq!(regular(1000.0, 5), full);
        // Four cards that fit the width keep it.
        let four = regular(1000.0, 4);
        assert_eq!(four.cols, 4);
        assert!(four.card_w <= (168.0_f32 * layout::CARD_SPARSE_SCALE).round());
    }

    #[test]
    fn one_game_is_a_spotlight_that_fits_the_viewport() {
        let s = regular(1000.0, 1);
        assert!(s.spotlight);
        assert_eq!(s.cols, 1);
        assert!(s.cover_h <= 600.0);
        assert!(s.card_w <= 1000.0);
        assert_eq!(s.caption_h, 0.0);
        // A low window: the cover shrinks, down to its minimum.
        let low = GridLayout::new(552.0, Density::Comfortable, true, 1, 300.0);
        assert!(low.cover_h < s.cover_h);
        assert!(low.cover_h >= layout::SPOTLIGHT_COVER_MIN * layout::COVER_ASPECT - 0.01);
        assert!(low.card_w <= 552.0);
    }

    #[test]
    fn rows_with_group_headers() {
        let g = regular(1000.0, 24);
        let span = |start, len| GroupSpan {
            key: easyplay_core::library::query::GroupKey::Letter('A'),
            label: Arc::from("A"),
            start,
            len,
        };
        let (rows, h) = layout_rows(&g, 7, &[span(0, 6), span(6, 1)]);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].0, Row::Header(0));
        assert_eq!(rows[1].0, Row::Cards { start: 0, len: 5 });
        assert_eq!(rows[2].0, Row::Cards { start: 5, len: 1 });
        assert_eq!(rows[3].0, Row::Header(1));
        assert!(h > 3.0 * g.row_h());
    }
}
