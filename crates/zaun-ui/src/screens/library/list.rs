//! The list (the design's `.list`): a 30 high header that stays put while the rows scroll,
//! then rows 44 high: the cover as a 24 × 36 thumb, the title, a star for a favourite and the
//! tags · Protection · Last played · Time played · Play. Name, Last played and Time played
//! sort on click. Under 860 wide the Last played column goes; under 620 Protection too.
//! Virtualised by row; `↑`/`↓` move, `Enter` opens, `Space` selects, `→`/`←` move between a
//! row and its Play.

use std::sync::Arc;

use egui::accesskit::Role;
use egui::{
    Align2, CornerRadius, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use zaun_core::ids::GameId;
use zaun_core::library::SortKey;
use zaun_core::model::{Activity, GameSummary};

use super::grid::Reflow;
use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::focus;
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke, travel};
use crate::theme::{self, Dur, Type, icons};
use crate::vm::library::{Shelf, ViewMode, is_missing};
use crate::widgets::button::{PillSize, play_pill, play_pill_blocked};
use crate::widgets::card::{PlayView, play_view, protection_word};
use crate::widgets::cover::{self, CoverLook, CoverSource};

/// The stable id of a game's row.
pub fn row_id(game: GameId) -> egui::Id {
    egui::Id::unique(("row", game))
}

/// Which of the optional columns fit a page `page_w` wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    /// Protection (from 620).
    pub protection: bool,
    /// Last played (from 860).
    pub last_played: bool,
}

impl Columns {
    /// The columns for a page `page_w` wide.
    pub fn for_width(page_w: f32) -> Self {
        Self {
            protection: page_w >= layout::NARROW_BELOW,
            last_played: page_w >= layout::MEDIUM_BELOW,
        }
    }

    /// The x ranges of the cells in a row `left..left + w` wide: name, protection,
    /// last played, time played, play.
    fn cells(self, left: f32, w: f32) -> [Option<(f32, f32)>; 5] {
        let [prot_w, last_w, time_w, play_w] = layout::LIST_COLS;
        let gap = layout::LIST_COL_GAP;
        let mut right = left + w;
        let mut take = |width: f32, on: bool| {
            on.then(|| {
                let r = (right - width, right);
                right -= width + gap;
                r
            })
        };
        let play = take(play_w, true);
        let time = take(time_w, true);
        let last = take(last_w, self.last_played);
        let prot = take(prot_w, self.protection);
        let name = Some((left, right.max(left)));
        [name, prot, last, time, play]
    }
}

/// A sortable column header: 12 / 500, quiet until pointed at; the current one is primary
/// with its arrow.
fn head(
    ui: &mut Ui,
    at: Pos2,
    label: &str,
    key: Option<SortKey>,
    end: bool,
    state: &UiState,
) -> Option<egui::Response> {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let active = key.is_some_and(|k| k == state.library.sort);
    let g = Type::Label.line(ui, label, p.t3);
    let [pad, gap] = layout::TH_PAD;
    let arrow = if active { gap + icon_size::HEAD } else { 0.0 };
    let w = pad * 2.0 + g.size().x + arrow;
    let h = layout::TH_H;
    let left = if end { at.x - w } else { at.x - pad };
    let rect = Rect::from_min_size(pos2(left, at.y - h / 2.0), vec2(w, h));
    let resp = key.map(|_| ui.interact(rect, ui.scope_id().with(("th", label)), Sense::click()));
    let hover = resp
        .as_ref()
        .map_or(0.0, |r| crate::widgets::hover_t(r, &theme));
    let ink = if active { p.t1 } else { mix(p.t3, p.t1, hover) };
    let painter = ui.painter();
    if hover > 0.0 {
        painter.rect_filled(rect, CornerRadius::same(radius::SM), fade(p.o1, hover));
    }
    painter.galley(
        pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0),
        g.clone(),
        ink,
    );
    if active {
        let glyph = if state.library.descending {
            icons::ARROW_DOWN
        } else {
            icons::ARROW_UP
        };
        painter.text(
            pos2(
                rect.left() + pad + g.size().x + gap + icon_size::HEAD / 2.0,
                rect.center().y,
            ),
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::HEAD),
            ink,
        );
    }
    if let Some(r) = &resp {
        let spoken = if active {
            strings::library::sorted_by(label, state.library.descending)
        } else {
            label.to_owned()
        };
        a11y::option(r, Role::ColumnHeader, &spoken, active);
        if r.has_focus() {
            crate::widgets::focus_ring(painter, rect, CornerRadius::same(radius::SM), &theme);
        }
    }
    resp
}

/// The header: Name · Protection · Last played · Time played.
fn header(ui: &mut Ui, state: &mut UiState, rect: Rect, cols: Columns, content: (f32, f32)) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    // It stays put: the page's background behind it, a hairline under it.
    let plate = Rect::from_min_max(
        pos2(rect.left() - layout::ROW_OVERHANG, rect.top()),
        pos2(rect.right() + layout::ROW_OVERHANG, rect.bottom()),
    );
    ui.painter().rect_filled(plate, CornerRadius::ZERO, p.bg);
    ui.painter().hline(
        plate.x_range(),
        plate.bottom() - stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line),
    );
    let cells = cols.cells(content.0, content.1);
    let cy = rect.center().y;
    let mut clicked = None;
    let l = strings::library::COL_NAME;
    if let Some((x, _)) = cells[0]
        && head(ui, pos2(x, cy), l(), Some(SortKey::Name), false, state)
            .is_some_and(|r| r.clicked())
    {
        clicked = Some((SortKey::Name, l()));
    }
    if let Some((x, _)) = cells[1] {
        head(
            ui,
            pos2(x, cy),
            strings::library::COL_PROTECTION(),
            None,
            false,
            state,
        );
    }
    let l = strings::library::COL_LAST_PLAYED;
    if let Some((x, _)) = cells[2]
        && head(
            ui,
            pos2(x, cy),
            l(),
            Some(SortKey::LastPlayed),
            false,
            state,
        )
        .is_some_and(|r| r.clicked())
    {
        clicked = Some((SortKey::LastPlayed, l()));
    }
    let l = strings::library::COL_PLAYTIME;
    if let Some((_, x)) = cells[3]
        && head(ui, pos2(x, cy), l(), Some(SortKey::Playtime), true, state)
            .is_some_and(|r| r.clicked())
    {
        clicked = Some((SortKey::Playtime, l()));
    }
    if let Some((k, label)) = clicked {
        state.library.set_sort(k, true);
        a11y::Announcer::announce(
            ui.ctx(),
            strings::library::sorted_by(label, state.library.descending),
            a11y::Politeness::Polite,
        );
    }
}

/// The thumb texture or poster of a row.
fn thumb_source(env: &mut Env<'_>, g: &GameSummary, jumped: bool) -> CoverSource {
    let Some(art) = g.cover else {
        return CoverSource::Placeholder;
    };
    let key = TexKey {
        art,
        size: ThumbSize::Cover240,
    };
    let s = if jumped {
        env.textures.peek(key, true)
    } else {
        env.textures.get(key, true, env.art)
    };
    // While the thumb's size loads, the wall's larger size stands in (a cover arriving from
    // the wall keeps its picture).
    let s = match s {
        TexState::Pending(c) => match env.textures.peek(
            TexKey {
                art,
                size: ThumbSize::Cover480,
            },
            false,
        ) {
            TexState::Ready(t) => TexState::Ready(t),
            TexState::Pending(_) => TexState::Pending(c),
        },
        s => s,
    };
    match s {
        TexState::Ready(t) => CoverSource::Texture(t, 1.0),
        TexState::Pending(Some(c)) => CoverSource::Pending(c),
        TexState::Pending(None) => CoverSource::Placeholder,
    }
}

/// Show the list; returns what the user asked for. `page_w` is the width of the page (the
/// columns follow it), `origin` the top left of the page's content.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    shelf: &Shelf,
    page_w: f32,
    origin: Pos2,
) -> Option<(GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let visible: &[u32] = &shelf.items;
    let mut action = None;
    let avail_w = ui.available_width();
    let cols = Columns::for_width(page_w);
    let row_h = layout::LIST_ROW_H;
    let head_h = layout::LIST_HEAD;
    let clip = ui.clip_rect();
    let prefs = env.bridge.prefs().clone();
    let single_click_plays = prefs.integration.single_click_play;

    // The header: at its place until the page scrolls under it, then pinned to the top.
    let (head_slot, _) = ui.allocate_exact_size(vec2(avail_w, head_h), Sense::hover());
    ui.add_space(layout::LIST_HEAD_GAP);
    let head_top = head_slot.top().max(clip.top());
    let head_rect = Rect::from_min_size(pos2(head_slot.left(), head_top), head_slot.size());
    let content = (head_slot.left(), avail_w);

    // Keyboard.
    let focused = ui.ctx().memory(|m| m.focused());
    let pos = state
        .library
        .selected
        .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
    let row_focus = pos.is_some_and(|p| focused == Some(row_id(lib.games[visible[p] as usize].id)));
    let ctx = ui.ctx().clone();
    let play_id =
        |g: GameId| -> Option<egui::Id> { ctx.data(|d| d.get_temp(row_id(g).with("play_id"))) };
    let play_focus = pos.is_some_and(|p| {
        let pid = play_id(lib.games[visible[p] as usize].id);
        pid.is_some() && focused == pid
    });
    state.library.focused = row_focus || play_focus;
    let mut scroll_to = None;
    if let Some(p) = pos
        && (row_focus || play_focus)
    {
        let id = lib.games[visible[p] as usize].id;
        let (left, right, enter, space_key) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                row_focus && i.consume_key(Modifiers::NONE, Key::Enter),
                row_focus && i.consume_key(Modifiers::NONE, Key::Space),
            )
        });
        let page = (((clip.height() - head_h) / row_h).floor() as usize).max(1);
        let mv = ui.input_mut(focus::take_move);
        let t = mv.and_then(|(mv, shift)| {
            let rows = focus::list_rows(visible.len());
            let t = focus::target(&rows, p, focus::list_move(mv), page)?;
            if shift {
                let tid = lib.games[visible[t] as usize].id;
                if state.library.anchor.is_none() {
                    state.library.anchor = Some(id);
                }
                state.library.select_range(lib, visible, tid);
            }
            Some(t)
        });
        if let Some(t) = t {
            let tid = lib.games[visible[t] as usize].id;
            state.library.selected = Some(tid);
            state.pending_focus = Some(tid);
            scroll_to = Some(t);
        } else if right && row_focus {
            if let Some(pid) = play_id(id) {
                ui.memory_mut(|m| m.request_focus(pid));
            }
        } else if left && play_focus {
            ui.memory_mut(|m| m.request_focus(row_id(id)));
        } else if enter {
            action = Some((id, GameAction::Open));
        } else if space_key {
            state.library.toggle_select(id);
        }
    }
    if scroll_to.is_none()
        && let Some(pf) = state.pending_focus
        && let Some(t) = visible.iter().position(|i| lib.games[*i as usize].id == pf)
    {
        scroll_to = Some(t);
    }

    let selection_active = !state.library.selection.is_empty();
    let (area, _) =
        ui.allocate_exact_size(vec2(avail_w, visible.len() as f32 * row_h), Sense::hover());
    let row_rect = |i: usize| {
        Rect::from_min_size(
            pos2(area.left(), area.top() + i as f32 * row_h),
            vec2(avail_w, row_h),
        )
    };
    if let Some(t) = scroll_to {
        // Clear of the pinned header above and of the window's edge below.
        let r = row_rect(t);
        ui.scroll_to_rect(
            Rect::from_min_max(
                pos2(r.left(), r.top() - head_h - layout::LIST_HEAD_GAP),
                pos2(r.right(), r.bottom() + layout::LIST_HEAD_GAP),
            ),
            None,
        );
    }
    let rows_clip = Rect::from_min_max(pos2(clip.left(), head_rect.bottom()), clip.max);
    let first =
        (((rows_clip.top() - area.top()) / row_h).floor().max(0.0) as usize).min(visible.len());
    let last =
        (((rows_clip.bottom() - area.top()) / row_h).ceil().max(0.0) as usize).min(visible.len());
    let jumped = super::page_jumped(ui, rows_clip.top() - area.top(), rows_clip.height());
    let reflow = Reflow::begin(
        ui.ctx(),
        state.library.generation(),
        ViewMode::List,
        state.library.typing,
    );
    let mut flow = reflow.lock();
    let near_view = clip.expand(travel::REFLOW_MARGIN);
    let cells = cols.cells(content.0, content.1);
    let menu_game = state.library.menu.map(|(id, _)| id);
    ui.scope_builder(UiBuilder::new().id_salt("library.list"), |ui| {
        ui.set_clip_rect(rows_clip.intersect(clip));
        a11y::labelled(
            ui.ctx(),
            ui.scope_id(),
            Role::Table,
            strings::library::LIST(),
        );
        let pending_pos = state
            .pending_focus
            .and_then(|g| visible.iter().position(|i| lib.games[*i as usize].id == g));
        let rover = focus::rover(pending_pos.or(pos), first..last);
        for pos in first..last {
            let g = &lib.games[visible[pos] as usize];
            let rect = row_rect(pos);
            let id = row_id(g.id);
            // The thumb: where the cover is, so it can travel between the wall and the list.
            let thumb_target = Rect::from_min_size(
                pos2(
                    rect.left(),
                    rect.center().y - layout::LIST_THUMB * layout::COVER_ASPECT / 2.0,
                ),
                vec2(
                    layout::LIST_THUMB,
                    layout::LIST_THUMB * layout::COVER_ASPECT,
                ),
            );
            let placed = flow.place(
                g.id,
                thumb_target.translate(-origin.to_vec2()),
                near_view.intersects(rect),
            );
            let thumb = placed.rect.translate(origin.to_vec2());
            // A row that moves with the order travels with its thumb.
            let shift = if placed.cross {
                vec2(0.0, 0.0)
            } else {
                thumb.min - thumb_target.min
            };
            let rect = rect.translate(shift);
            state.flight.note_wall(g.id, thumb);
            let sense = if rover == Some(pos) {
                Sense::click()
            } else {
                Sense::CLICK
            };
            let resp = ui.interact(rect, id, sense);
            crate::widgets::own_arrow_keys(&resp);
            let state_text = strings::card::state(g.activity, g.availability, env.now);
            crate::widgets::card::tick_elapsed(ui.ctx(), g.activity, env.now);
            let name = strings::card::accessible_name(
                &g.name,
                protection_word(g),
                state_text.as_deref(),
                g.total_playtime_ms,
                g.last_played_at,
                env.now,
                env.utc_offset_s,
            );
            let multi = state.library.is_selected(g.id);
            a11y::option(&resp, Role::Row, &name, multi);
            if rover != Some(pos) && focus::accesskit_focus_requested(ui.ctx(), id) {
                state.pending_focus = Some(g.id);
                ui.ctx().request_repaint();
            }
            if state.pending_focus == Some(g.id) {
                resp.request_focus();
                state.pending_focus = None;
            }
            if resp.has_focus() {
                state.library.selected = Some(g.id);
                state.library.cursor_rect = Some(rect);
            }
            let play_has_focus = play_id(g.id).is_some_and(|pid| focused == Some(pid));
            let keyboard = resp.has_focus() && crate::widgets::focus_visible(ui.ctx());
            let hot =
                resp.contains_pointer() || keyboard || play_has_focus || menu_game == Some(g.id);
            let hot_t =
                ui.ctx()
                    .animate_bool_with_time(id.with("hot"), hot, theme.motion.fade(Dur::Fast));
            // The plate reaches 8 past the content, so the thumb stays on the page's axis.
            let plate = rect.expand2(vec2(layout::ROW_OVERHANG, 0.0));
            let corner = CornerRadius::same(radius::ROW);
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(placed.opacity);
            if multi {
                painter.rect_filled(plate, corner, p.tint);
            } else if hot_t > 0.0 {
                painter.rect_filled(plate, corner, fade(p.o1, hot_t));
            }
            if keyboard {
                // The ring sits inside the plate.
                painter.rect_stroke(
                    plate,
                    corner,
                    Stroke::new(theme.focus_width(), p.accent_mark),
                    egui::StrokeKind::Inside,
                );
            }
            if !g.is_steam() {
                state
                    .dnd
                    .target(rect, crate::input::dnd::DropTarget::Cover(g.id));
            }
            let cy = rect.center().y;
            // Name: thumb · title · star · tags.
            if !state.flight.hides(g.id) {
                let thumb_painter = if placed.cross {
                    // Crossing from the wall: drawn above the rows, unclipped.
                    ui.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Middle,
                        egui::Id::unique("library.crossing"),
                    ))
                } else {
                    painter.clone()
                };
                let src = thumb_source(env, g, jumped);
                let look = CoverLook {
                    missing: is_missing(g),
                    mark: !g.is_steam(),
                    opacity: 1.0,
                };
                cover::paint(&thumb_painter, thumb, src, &g.name, look, &theme);
                if g.is_steam() {
                    cover::paint_steam_badge(&thumb_painter, thumb, src, &g.name, look, &theme);
                }
            }
            let gap = space::S3;
            if let Some((x0, x1)) = cells[0] {
                let mut x = x0 + layout::LIST_THUMB + gap;
                let title = Type::BodyStrong.galley(ui, &g.name, p.t1, (x1 - x).max(1.0), 1);
                let tw = title.size().x;
                painter.galley(pos2(x, cy - title.size().y / 2.0), title, p.t1);
                x += tw + gap;
                if g.favorite && x + icon_size::SMALL < x1 {
                    painter.text(
                        pos2(x + icon_size::SMALL / 2.0, cy),
                        Align2::CENTER_CENTER,
                        icons::STAR.text(true),
                        icons::font(icon_size::SMALL),
                        p.t1,
                    );
                    x += icon_size::SMALL + gap;
                }
                let tags: Vec<&str> = g
                    .tags
                    .iter()
                    .filter_map(|t| lib.tag(*t))
                    .map(|t| &*t.name)
                    .collect();
                if !tags.is_empty() && x < x1 {
                    let t = Type::Caption.galley(ui, &tags.join(", "), p.t3, x1 - x, 1);
                    painter.galley(pos2(x, cy - t.size().y / 2.0), t, p.t3);
                }
            }
            if let Some((x0, x1)) = cells[1] {
                // A Steam game has no level: "Run by Steam" (ADR-0031).
                let t = Type::Body.galley(ui, protection_word(g), p.t2, x1 - x0, 1);
                painter.galley(pos2(x0, cy - t.size().y / 2.0), t, p.t2);
            }
            if let Some((x0, x1)) = cells[2] {
                let (text, color) = if is_missing(g) {
                    (strings::card::NOT_FOUND().to_owned(), p.amber)
                } else if matches!(g.activity, Activity::Running { .. } | Activity::Lingering) {
                    (strings::card::RUNNING().to_owned(), p.t2)
                } else if g.is_steam() && g.last_played_at.is_none() && !g.never_played() {
                    // Steam counted time but gave no date (ADR-0031).
                    (strings::steam::LAST_UNKNOWN().to_owned(), p.t2)
                } else {
                    (
                        strings::fmt::last_played(g.last_played_at, env.now, env.utc_offset_s),
                        p.t2,
                    )
                };
                let t = Type::Body.galley(ui, &text, color, x1 - x0, 1);
                painter.galley(pos2(x0, cy - t.size().y / 2.0), t, color);
            }
            if let Some((_, x1)) = cells[3]
                && g.total_playtime_ms >= 60_000
            {
                let t = Type::Body.numeric_galley(
                    ui,
                    &strings::fmt::play_time(g.total_playtime_ms),
                    p.t2,
                );
                let at =
                    Rect::from_min_size(pos2(x1 - t.size().x, cy - t.size().y / 2.0), t.size());
                painter.galley(at.min, t, p.t2);
                // A Steam game's time is Steam's: the figure says so when pointed at.
                if g.is_steam() {
                    let tip = ui.interact(at, id.with("time"), Sense::hover());
                    let _ =
                        crate::widgets::tooltip::attach(tip, strings::steam::PLAYTIME_FROM_STEAM());
                }
            }
            if let Some((x0, x1)) = cells[4] {
                // Out of sight until the row is pointed at or focused; a game that runs or
                // starts keeps it.
                let shown = if g.activity != Activity::Idle || state.controller {
                    1.0
                } else {
                    hot_t
                };
                let slot = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom()));
                let mut cell_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(slot)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                cell_ui.multiply_opacity(placed.opacity);
                let (pill, plays) = match play_view(g) {
                    PlayView::State(st) => (
                        play_pill(&mut cell_ui, &st, &g.name, PillSize::Row, shown),
                        true,
                    ),
                    // Steam runs it: no Stop, the quiet Play says why.
                    PlayView::Blocked(why) => (
                        play_pill_blocked(&mut cell_ui, &g.name, why, PillSize::Row, shown),
                        false,
                    ),
                };
                ui.ctx()
                    .data_mut(|d| d.insert_temp(id.with("play_id"), pill.id));
                if plays && pill.clicked() {
                    action = Some((g.id, GameAction::Play));
                }
            }
            if action.is_none() {
                if resp.secondary_clicked() {
                    let at = resp.interact_pointer_pos().unwrap_or(rect.center());
                    super::open_menu_at(state, g.id, at);
                } else if resp.clicked() {
                    let m = ui.input(|i| i.modifiers);
                    if selection_active || m.shift || m.command {
                        state.library.click(lib, visible, g.id, m.shift, m.command);
                    } else if single_click_plays {
                        action = Some((g.id, GameAction::Play));
                    } else {
                        action = Some((g.id, GameAction::Open));
                    }
                }
            }
        }
    });
    flow.end(ui.ctx());
    drop(flow);
    // The header last, so it covers the rows that scroll under it.
    let mut head_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(head_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    head_ui.set_clip_rect(
        head_rect
            .expand2(vec2(layout::ROW_OVERHANG, 0.0))
            .intersect(clip),
    );
    header(&mut head_ui, state, head_rect, cols, content);
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_give_way_on_narrow_pages() {
        assert_eq!(
            Columns::for_width(1280.0),
            Columns {
                protection: true,
                last_played: true
            }
        );
        assert!(!Columns::for_width(840.0).last_played);
        assert!(Columns::for_width(840.0).protection);
        assert!(!Columns::for_width(600.0).protection);
    }

    #[test]
    fn cells_fill_from_the_right() {
        let c = Columns::for_width(1280.0).cells(0.0, 1000.0);
        let [name, prot, last, time, play] = c;
        assert_eq!(play, Some((970.0, 1000.0)));
        assert_eq!(time, Some((858.0, 954.0)));
        assert_eq!(last, Some((712.0, 842.0)));
        assert_eq!(prot, Some((586.0, 696.0)));
        assert_eq!(name, Some((0.0, 570.0)));
        let narrow = Columns::for_width(600.0).cells(0.0, 568.0);
        assert_eq!(narrow[1], None);
        assert_eq!(narrow[2], None);
    }
}
