//! The virtualized cover grid (PERFORMANCE §5, SCREEN_SPECS §2–3). Only rows intersecting
//! the viewport (plus one row of overscan) are laid out and painted. Arrow keys move the
//! selection spatially (ACCESSIBILITY §2.1); `Enter` opens, `Space`/`Ctrl+Enter` play, `F`
//! favourites.
//!
//! Foundation version for UI-008; UI-009 adds prefetch, grouping and the allocation bench.

use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Key, Modifiers, Rect, ScrollArea, Ui, pos2, vec2};

use super::ItemAction;
use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::screens::Env;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::tokens::{layout, radius, space};
use crate::theme::{self, Density};
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

/// Show the grid; returns what the user asked for.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
) -> Option<ItemAction> {
    let theme = theme::theme(ui.ctx());
    let compact = ui.ctx().content_rect().width() < layout::COMPACT_BELOW;
    let avail_w = ui.available_width();
    let g = GridLayout::new(avail_w, theme.density, compact);
    let rows = g.rows(visible.len());
    let mut action = None;
    let ppp = ui.ctx().pixels_per_point();
    let thumb = ThumbSize::cover_for(g.card_w * ppp);

    // Keyboard: arrow navigation over the visible order.
    let sel_pos = state
        .library
        .selected
        .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
    let grid_has_focus = ui
        .ctx()
        .memory(|m| m.focused())
        .is_some_and(|f| sel_pos.is_some_and(|p| f == card_id(lib, visible, p)));
    let mut scroll_to: Option<usize> = None;
    if grid_has_focus && let Some(pos) = sel_pos {
        let (l, r, u, d, home, end, enter, play, fav) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Home),
                i.consume_key(Modifiers::NONE, Key::End),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Enter)
                    || i.consume_key(Modifiers::NONE, Key::Space),
                i.consume_key(Modifiers::NONE, Key::F),
            )
        });
        let col = pos % g.cols;
        let target = if l && col > 0 {
            Some(pos - 1)
        } else if r && col + 1 < g.cols && pos + 1 < visible.len() {
            Some(pos + 1)
        } else if u && pos >= g.cols {
            Some(pos - g.cols)
        } else if d {
            (pos + g.cols < visible.len()).then_some(pos + g.cols)
        } else if home {
            Some(pos - col)
        } else if end {
            Some((pos - col + g.cols - 1).min(visible.len() - 1))
        } else {
            None
        };
        let id = lib.games[visible[pos] as usize].id;
        if let Some(t) = target {
            state.library.selected = Some(lib.games[visible[t] as usize].id);
            state.pending_focus = state.library.selected;
            scroll_to = Some(t);
        } else if enter {
            action = Some(ItemAction::Open(id));
        } else if play {
            action = Some(ItemAction::Play(id));
        } else if fav {
            action = Some(ItemAction::Favorite(id));
        }
    }

    let total_h = (rows as f32 * g.row_h() - g.gap).max(0.0);
    let grid_name = strings::library::grid_name(visible.len(), g.cols);
    let scroll = ScrollArea::vertical()
        .id_salt(("library.grid", state.library.collection))
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            ui.set_height(total_h + space::S6);
            a11y::labelled(ui.ctx(), ui.scope_id(), Role::Grid, &grid_name);
            let origin = ui.min_rect().min;
            if let Some(t) = scroll_to {
                let (r, c) = (t / g.cols, t % g.cols);
                let rect = Rect::from_min_size(
                    pos2(
                        origin.x + c as f32 * (g.card_w + g.gap),
                        origin.y + r as f32 * g.row_h(),
                    ),
                    vec2(g.card_w, g.card_h),
                )
                .expand(g.row_h() * 0.25);
                ui.scroll_to_rect(rect, None);
            }
            let first = ((viewport.min.y / g.row_h()).floor() as isize - 1).max(0) as usize;
            let last = ((viewport.max.y / g.row_h()).ceil() as usize + 1).min(rows);
            for row in first..last {
                for col in 0..g.cols {
                    let pos = row * g.cols + col;
                    let Some(&gi) = visible.get(pos) else { break };
                    let game = &lib.games[gi as usize];
                    let rect = Rect::from_min_size(
                        pos2(
                            origin.x + col as f32 * (g.card_w + g.gap),
                            origin.y + row as f32 * g.row_h(),
                        ),
                        vec2(g.card_w, g.card_h),
                    );
                    let (texture, placeholder) = match game.cover {
                        Some(art) => {
                            match env.textures.get(TexKey { art, size: thumb }, true, env.art) {
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
                        multi_selected: false,
                        controller: false,
                        show_playtime: true,
                        density: theme.density,
                    };
                    let id = card_id(lib, visible, pos);
                    let r = card(ui, rect, id, game, &ctx);
                    if state.pending_focus == Some(game.id) {
                        r.response.request_focus();
                        state.pending_focus = None;
                    }
                    if r.response.has_focus() {
                        state.library.selected = Some(game.id);
                    }
                    if r.toggle_favorite {
                        action = Some(ItemAction::Favorite(game.id));
                    } else if r.play {
                        action = Some(ItemAction::Play(game.id));
                    } else if r.response.clicked() {
                        action = Some(ItemAction::Open(game.id));
                    }
                }
            }
        });
    let _ = scroll;
    action
}

/// The stable id of the card at `pos`.
fn card_id(lib: &LibraryView, visible: &[u32], pos: usize) -> egui::Id {
    egui::Id::unique(("card", lib.games[visible[pos] as usize].id))
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
}
