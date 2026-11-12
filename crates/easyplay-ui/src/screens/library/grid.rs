//! The virtualized cover grid (PERFORMANCE §5, SCREEN_SPECS §2–3). Rows (card rows and group
//! headers) have precomputed offsets; only rows intersecting the viewport (plus one row of
//! overscan) are laid out, and covers of the next screenful are requested ahead. Arrow keys
//! move the cursor spatially (ACCESSIBILITY §2.1, `input::focus`); `Enter` opens, `Space`
//! plays; `Shift` extends the selection; `Ctrl+click` toggles; right-click opens the context
//! menu. Only one card is in the `Tab` order (roving focus), so `Tab` leaves the grid.

use std::sync::Arc;

use easyplay_core::library::GroupSpan;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Rect, ScrollArea, Ui, pos2, vec2};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::focus;
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{layout, radius, space};
use crate::theme::{self, Density, Type};
use crate::widgets::card::{CardCtx, card};
use crate::widgets::skeleton;

/// Grid geometry for a width (PERFORMANCE §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    /// Columns.
    pub cols: usize,
    /// Card width.
    pub card_w: f32,
    /// Card height (2:3 cover).
    pub card_h: f32,
    /// Gap between cards.
    pub gap: f32,
}

impl GridLayout {
    /// Compute for `avail_w` at `density`; `compact` uses the smaller gap.
    pub fn new(avail_w: f32, density: Density, compact: bool) -> Self {
        let gap = if compact {
            layout::CARD_GAP_COMPACT
        } else {
            layout::CARD_GAP
        };
        let min = density.card_min_width();
        let cols = (((avail_w + gap) / (min + gap)).floor() as usize).max(1);
        let card_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(1.0);
        Self {
            cols,
            card_w,
            card_h: card_w * layout::COVER_ASPECT,
            gap,
        }
    }

    /// Row pitch.
    pub fn row_h(&self) -> f32 {
        self.card_h + self.gap
    }

    /// Rows for `n` items.
    pub fn rows(&self, n: usize) -> usize {
        n.div_ceil(self.cols)
    }
}

/// Height of a group header row.
const HEADER_H: f32 = space::S10;

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
    (rows, (y - g.gap).max(0.0))
}

/// Twelve card skeletons, before the first snapshot.
pub fn skeleton(ui: &mut Ui, density: Density) {
    let compact = ui.ctx().content_rect().width() < layout::COMPACT_BELOW;
    let g = GridLayout::new(ui.available_width(), density, compact);
    let origin = ui.cursor().min;
    for i in 0..12 {
        let (r, c) = (i / g.cols, i % g.cols);
        let rect = Rect::from_min_size(
            pos2(
                origin.x + c as f32 * (g.card_w + g.gap),
                origin.y + r as f32 * g.row_h(),
            ),
            vec2(g.card_w, g.card_h),
        );
        skeleton::block(ui, rect, Some(egui::CornerRadius::same(radius::MD)));
    }
}

/// The stable id of a game's card.
pub fn card_id(game: easyplay_core::ids::GameId) -> egui::Id {
    egui::Id::unique(("card", game))
}

/// Show the grid; returns what the user asked for.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
    groups: &[GroupSpan],
) -> Option<(easyplay_core::ids::GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let compact = ui.ctx().content_rect().width() < layout::COMPACT_BELOW;
    let avail_w = ui.available_width();
    let g = GridLayout::new(avail_w, theme.density, compact);
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
    let thumb = ThumbSize::cover_for(g.card_w * ppp);
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
        let page = ((ui.clip_rect().height() / g.row_h()).floor() as usize).max(1);
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

    let grid_name = strings::library::grid_name(visible.len(), g.cols);
    let selection_active = !state.library.selection.is_empty();
    ScrollArea::vertical()
        .id_salt(("library.grid", state.library.collection))
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            ui.set_height(total_h + space::S6);
            a11y::labelled(ui.ctx(), ui.scope_id(), Role::Grid, &grid_name);
            let origin = ui.min_rect().min;
            let cell = |row_y: f32, col: usize| {
                Rect::from_min_size(
                    pos2(origin.x + col as f32 * (g.card_w + g.gap), origin.y + row_y),
                    vec2(g.card_w, g.card_h),
                )
            };
            if let Some(t) = scroll_to
                && let Some((Row::Cards { start, .. }, y)) = rows.iter().find(|(r, _)| {
                    matches!(r, Row::Cards { start, len } if t >= *start && t < start + len)
                })
            {
                ui.scroll_to_rect(cell(*y, t - start).expand(g.row_h() * 0.25), None);
            }
            // First row whose bottom is below the viewport top, minus one row of overscan.
            let first = rows
                .partition_point(|(_, y)| *y + g.row_h() < viewport.min.y)
                .saturating_sub(1);
            // The cells laid out this frame, and the one in the `Tab` order.
            let mut laid_out: Option<(usize, usize)> = None;
            for (row, y) in rows.iter().skip(first) {
                if *y > viewport.max.y + g.row_h() {
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
            let prefetch_to = viewport.max.y + viewport.height();
            for (row, y) in rows.iter().skip(first) {
                if *y > prefetch_to {
                    break;
                }
                let on_screen = *y <= viewport.max.y + g.row_h();
                match row {
                    Row::Header(gi) => {
                        if !on_screen {
                            continue;
                        }
                        let span = &groups[*gi];
                        let r = Rect::from_min_size(
                            pos2(origin.x, origin.y + y),
                            vec2(avail_w, HEADER_H),
                        );
                        ui.painter().text(
                            pos2(r.left(), r.center().y),
                            egui::Align2::LEFT_CENTER,
                            strings::library::group_header(&span.label, span.len as usize),
                            Type::Heading.font_id(),
                            p.text_secondary,
                        );
                    }
                    Row::Cards { start, len } => {
                        for k in 0..*len {
                            let pos = start + k;
                            let game = &lib.games[visible[pos] as usize];
                            if !on_screen {
                                // Prefetch: ask for the cover without drawing.
                                if let Some(art) = game.cover {
                                    let _ = env.textures.get(
                                        TexKey { art, size: thumb },
                                        false,
                                        env.art,
                                    );
                                }
                                continue;
                            }
                            let rect = cell(*y, k);
                            let (texture, placeholder) = match game.cover {
                                Some(art) => {
                                    match env.textures.get(
                                        TexKey { art, size: thumb },
                                        true,
                                        env.art,
                                    ) {
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
                            };
                            let id = card_id(game.id);
                            let r = card(ui, rect, id, game, &ctx);
                            state
                                .dnd
                                .target(rect, crate::input::dnd::DropTarget::Cover(game.id));
                            if rover != Some(pos) && focus::accesskit_focus_requested(ui.ctx(), id)
                            {
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

    #[test]
    fn layout_maths() {
        let g = GridLayout::new(1000.0, Density::Comfortable, false);
        // floor((1000 + 16) / (168 + 16)) = 5
        assert_eq!(g.cols, 5);
        assert!((g.card_w * 5.0 + 16.0 * 4.0 - 1000.0).abs() < 0.01);
        assert!((g.card_h - g.card_w * 1.5).abs() < 0.01);
        assert_eq!(g.rows(11), 3);
        let narrow = GridLayout::new(100.0, Density::Large, true);
        assert_eq!(narrow.cols, 1);
    }

    #[test]
    fn rows_with_group_headers() {
        let g = GridLayout::new(1000.0, Density::Comfortable, false);
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
