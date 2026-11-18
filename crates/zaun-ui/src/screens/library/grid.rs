//! The wall (the design's `.wall`, PERFORMANCE §5): covers only, virtualised by row. Rows of
//! covers and group headings have precomputed offsets; only rows that meet the viewport
//! (plus one of overscan) are laid out, and covers of the next screenful are asked for
//! ahead. Arrow keys move between covers, `Enter` opens, `Space` selects; the covers are one
//! tab stop.
//!
//! The wall adapts to the size of the library, not of the current search: covers are as
//! wide as the cover-size preference says, raised to 280 for one game, 240 for up to three
//! and 208 for up to six. A library of up to six games (scope All, nothing filtered, no
//! groups) keeps its covers at that width and ends with a dashed "Add game" tile; up to three
//! sit in the middle of the window.
//!
//! When what is shown changes (search, filter, sort, scope, group, add, remove, hide), the
//! covers still shown slide from where they were, new ones rise in 18 ms apart, and covers
//! that left are gone at once ([`Reflow`]). Switching covers ↔ list, only the cover travels.

use std::collections::HashMap;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::mutex::Mutex;
use egui::{
    Align2, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2,
};
use zaun_core::ids::GameId;

use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::input::focus::{self, GridMove};
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::motion::ease_out;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{icon as icon_size, layout, overlay, stroke, travel};
use crate::theme::{self, Density, Dur, Type, icons, motion};
use crate::vm::library::{Section, Shelf, ViewMode};
use crate::widgets::card::{CardCtx, WallLook, wall_card};
use crate::widgets::cover;

/// The width of a cover for a library of `size` games at `density`.
pub fn cover_width(density: Density, size: usize) -> f32 {
    let by_size = if size <= 1 {
        layout::COVER_ONE
    } else if size <= layout::BIG_UP_TO {
        layout::COVER_FEW
    } else if size <= layout::SPARSE_UP_TO {
        layout::COVER_SPARSE
    } else {
        0.0
    };
    density.cover_width().max(by_size)
}

/// Wall geometry for a width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    /// Columns.
    pub cols: usize,
    /// Cover width.
    pub card_w: f32,
    /// Cover height (2:3).
    pub cover_h: f32,
    /// Gap between columns.
    pub gap: f32,
    /// Gap between rows.
    pub row_gap: f32,
    /// A small library: fixed covers and the "Add game" tile.
    pub sparse: bool,
    /// Up to three games: the row is centred and sits in the middle of the window.
    pub big: bool,
    /// Where the first column starts (a centred row).
    pub left: f32,
    /// Space above the first row (a small display in the middle of the window).
    pub top: f32,
}

impl GridLayout {
    /// Geometry for `avail_w` at `density` for a library of `size` games. `sparse` is the
    /// small library's wall (see [`crate::vm::library::LibraryVm::sparse`]); `cells` is the
    /// number of cells it shows (covers and the tile); `view_h` is the height of the
    /// scrolling page's content box.
    pub fn new(
        avail_w: f32,
        density: Density,
        size: usize,
        sparse: bool,
        cells: usize,
        view_h: f32,
    ) -> Self {
        let cw = cover_width(density, size);
        if sparse {
            let (gap, row_gap) = (layout::SPARSE_GAP_X, layout::SPARSE_GAP_Y);
            let cols = (((avail_w + gap) / (cw + gap)).floor() as usize).max(1);
            let big = size <= layout::BIG_UP_TO;
            let (left, top) = if big {
                let k = cells.clamp(1, cols) as f32;
                let row_w = k * cw + (k - 1.0) * gap;
                let top = (layout::BIG_CENTRE * view_h
                    - cw * layout::COVER_ASPECT / 2.0
                    - layout::BIG_LESS)
                    .max(layout::BIG_TOP_MIN);
                (((avail_w - row_w) / 2.0).max(0.0), top)
            } else {
                (0.0, 0.0)
            };
            return Self {
                cols,
                card_w: cw,
                cover_h: cw * layout::COVER_ASPECT,
                gap,
                row_gap,
                sparse: true,
                big,
                left,
                top,
            };
        }
        let gap = layout::WALL_GAP_X;
        let cols = (((avail_w + gap) / (cw + gap)).floor() as usize).max(1);
        let stretched = (avail_w - gap * (cols as f32 - 1.0)) / cols as f32;
        let card_w = stretched.max(cw.min(avail_w)).max(1.0);
        Self {
            cols,
            card_w,
            cover_h: card_w * layout::COVER_ASPECT,
            gap,
            row_gap: layout::WALL_GAP_Y,
            sparse: false,
            big: false,
            left: 0.0,
            top: 0.0,
        }
    }

    /// Row pitch.
    pub fn row_h(&self) -> f32 {
        self.cover_h + self.row_gap
    }

    /// Rows for `n` cells.
    pub fn rows(&self, n: usize) -> usize {
        n.div_ceil(self.cols)
    }
}

/// Height of a group heading and the space under it.
fn heading_h() -> f32 {
    Type::BodyStrong.line_height() + layout::GROUP_HEAD_GAP
}

/// One laid-out row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A group heading (index into the sections).
    Header(usize),
    /// Cells `start..start + len` of the shown order.
    Cards {
        /// First position.
        start: usize,
        /// Count.
        len: usize,
    },
}

/// Rows with their top offsets (from the wall's top, after [`GridLayout::top`]), and the
/// total height.
pub fn layout_rows(g: &GridLayout, n: usize, groups: &[Section]) -> (Vec<(Row, f32)>, f32) {
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
            if gi > 0 {
                y += layout::GROUP_GAP - g.row_gap;
            }
            rows.push((Row::Header(gi), y));
            y += heading_h();
            push_cards(&mut rows, &mut y, span.start as usize, span.len as usize);
        }
    }
    (rows, (y - g.row_gap).max(0.0))
}

/// The stable id of a game's card.
pub fn card_id(game: GameId) -> Id {
    Id::unique(("card", game))
}

// ── Reflow ──────────────────────────────────────────────────────────────────────────────────

/// One cover's motion: where it travels from and to (relative to the page's content), and
/// since when. A cover newly shown "enters": it fades in and rises.
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: Rect,
    to: Rect,
    start: f64,
    dur: f32,
    delay: f32,
    enter: bool,
    /// The covers ↔ list switch: only the cover travels; its row stays.
    cross: bool,
}

/// Where a cover is drawn this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    /// The cover's rect, relative to the page's content.
    pub rect: Rect,
    /// Its opacity (a cover entering).
    pub opacity: f32,
    /// It is crossing between the wall and the list: its row does not move with it.
    pub cross: bool,
}

/// The wall's memory of where every cover was (the design's `snapshot` / `playFlip`, as one
/// tween per game id).
#[derive(Default)]
pub(crate) struct Reflow {
    tweens: HashMap<GameId, Tween>,
    /// Where each cover was drawn last (relative to the page's content).
    drawn: HashMap<GameId, Rect>,
    generation: u64,
    view: Option<ViewMode>,
    shown: bool,
    last_pass: u64,
    // This frame:
    now: f64,
    pass: u64,
    changed: bool,
    crossed: bool,
    returning: bool,
    quick: bool,
    reduced: bool,
    entering: usize,
    seen: Vec<GameId>,
    running: bool,
}

/// Where the wall or the list last drew the cover of `id` (a list row's thumb in the list),
/// relative to the top left of the page's content; `None` when it was not laid out. For
/// tests and review renders of the reflow.
pub fn last_drawn(ctx: &egui::Context, id: GameId) -> Option<Rect> {
    reflow(ctx).lock().drawn.get(&id).copied()
}

fn reflow(ctx: &egui::Context) -> Arc<Mutex<Reflow>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<Reflow>>>(Id::unique("library.reflow"))
            .clone()
    })
}

impl Reflow {
    /// Start a frame of the page: `generation` is the shown order's, `quick` while typing in
    /// the search field.
    pub(crate) fn begin(
        ctx: &egui::Context,
        generation: u64,
        view: ViewMode,
        quick: bool,
    ) -> Arc<Mutex<Reflow>> {
        let r = reflow(ctx);
        {
            let mut f = r.lock();
            f.pass = ctx.cumulative_pass_nr();
            f.now = ctx.input(|i| i.time);
            f.reduced = theme::theme(ctx).motion.reduced;
            f.returning = f.shown && f.last_pass + 1 < f.pass;
            if f.returning {
                f.tweens.clear();
            }
            f.changed = f.shown && f.generation != generation;
            f.crossed = f.shown && f.view.is_some_and(|v| v != view);
            f.generation = generation;
            f.view = Some(view);
            f.quick = quick;
            f.entering = 0;
            f.seen.clear();
            f.running = false;
        }
        r
    }

    fn at(&self, t: &Tween) -> (Rect, f32) {
        let raw = if t.dur <= 0.0 {
            1.0
        } else {
            (((self.now - t.start) as f32 - t.delay) / t.dur).clamp(0.0, 1.0)
        };
        let e = ease_out(raw);
        (
            Rect::from_min_max(t.from.min.lerp(t.to.min, e), t.from.max.lerp(t.to.max, e)),
            raw,
        )
    }

    /// Where the cover of `id`, laid out at `target` this frame, is drawn. `near` is whether
    /// it is within 100 of the viewport (only those animate).
    pub(crate) fn place(&mut self, id: GameId, target: Rect, near: bool) -> Placed {
        self.seen.push(id);
        let fresh = !self.shown || self.changed || self.crossed;
        let motion = theme::Motion {
            reduced: self.reduced,
        };
        let now = self.now;
        let tween = match self.tweens.get(&id).copied() {
            None if fresh && near => {
                let delay = if self.quick || self.reduced {
                    0.0
                } else {
                    self.entering.min(travel::STAGGER_CAP) as f32 * travel::STAGGER
                };
                self.entering += 1;
                Tween {
                    from: target,
                    to: target,
                    start: now,
                    dur: if self.quick && !self.reduced {
                        travel::REFLOW_TYPING
                    } else {
                        motion.fade(Dur::Move)
                    },
                    delay,
                    enter: true,
                    cross: false,
                }
            }
            None => Tween {
                from: target,
                to: target,
                start: now,
                dur: 0.0,
                delay: 0.0,
                enter: false,
                cross: false,
            },
            Some(t) if t.to != target => {
                let (now_rect, raw) = self.at(&t);
                // Still on its way (the page's scroll bar came or went, the window was
                // resized): the journey goes on to the new place instead of ending at once.
                let in_flight = raw < 1.0 && (t.enter || t.from != t.to);
                let travels = (self.changed || self.crossed) && !self.returning && !self.reduced;
                let near = near || now_rect.intersects(target.expand(travel::REFLOW_MARGIN));
                if travels && near {
                    let dur = if self.crossed {
                        motion.movement(Dur::Page)
                    } else if self.quick {
                        travel::REFLOW_TYPING
                    } else {
                        motion.movement(Dur::Move)
                    };
                    Tween {
                        from: now_rect,
                        to: target,
                        start: now,
                        dur,
                        delay: 0.0,
                        enter: false,
                        cross: self.crossed,
                    }
                } else if in_flight {
                    Tween {
                        from: if t.from == t.to { target } else { t.from },
                        to: target,
                        ..t
                    }
                } else {
                    Tween {
                        from: target,
                        to: target,
                        start: now,
                        dur: 0.0,
                        delay: 0.0,
                        enter: t.enter,
                        cross: false,
                    }
                }
            }
            Some(t) => t,
        };
        self.tweens.insert(id, tween);
        let (rect, raw) = self.at(&tween);
        let running = raw < 1.0 && (tween.enter || tween.from != tween.to);
        self.running |= running;
        if tween.enter && raw < 1.0 {
            let e = ease_out(raw);
            let (opacity, rise, scale) = if self.reduced {
                (raw, 0.0, 1.0)
            } else {
                (
                    e,
                    travel::RISE * (1.0 - e),
                    travel::POP_FROM + (1.0 - travel::POP_FROM) * e,
                )
            };
            let r = motion::scaled(rect, scale).translate(vec2(0.0, rise));
            self.drawn.insert(id, r);
            return Placed {
                rect: r,
                opacity,
                cross: false,
            };
        }
        self.drawn.insert(id, rect);
        Placed {
            rect,
            opacity: 1.0,
            cross: tween.cross && running,
        }
    }

    /// End the frame: covers not laid out are forgotten; repaint while anything moves.
    pub(crate) fn end(&mut self, ctx: &egui::Context) {
        let seen: std::collections::HashSet<GameId> = self.seen.iter().copied().collect();
        self.tweens.retain(|id, _| seen.contains(id));
        self.drawn.retain(|id, _| seen.contains(id));
        self.shown = true;
        self.last_pass = self.pass;
        if self.running {
            ctx.request_repaint();
        }
    }
}

// ── Loading ─────────────────────────────────────────────────────────────────────────────────

/// Loading: cover-shaped blocks on the wall's grid, pulsing 1 ↔ .45 over 1.4 s, each column
/// 90 ms after the one before it ([`crate::widgets::skeleton::cover`]).
pub fn skeleton(ui: &mut Ui, density: Density) {
    let n = layout::SKELETON_COVERS;
    let g = GridLayout::new(ui.available_width(), density, usize::MAX, false, n, 0.0);
    let (area, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), g.rows(n) as f32 * g.row_h()),
        Sense::hover(),
    );
    for k in 0..n {
        let (r, c) = (k / g.cols, k % g.cols);
        let rect = Rect::from_min_size(
            pos2(
                area.left() + c as f32 * (g.card_w + g.gap),
                area.top() + r as f32 * g.row_h(),
            ),
            vec2(g.card_w, g.cover_h),
        );
        if ui.is_rect_visible(rect) {
            crate::widgets::skeleton::cover(ui, rect, c);
        }
    }
}

// ── The "Add game" tile ─────────────────────────────────────────────────────────────────────

/// A rounded rectangle as a closed path (for a dashed outline).
fn rounded_path(rect: Rect, r: f32) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let corners = [
        (rect.right() - r, rect.top() + r, -90.0_f32),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=6 {
            let a = (start + 15.0 * i as f32).to_radians();
            pts.push(pos2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    if let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    pts
}

/// The dashed "Add game" tile after the last cover of a small library.
fn add_tile(ui: &mut Ui, rect: Rect, opacity: f32) -> egui::Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = ui.interact(rect, Id::unique("library.add_tile"), Sense::click());
    a11y::button(&resp, strings::library::ADD_GAME());
    let hover = crate::widgets::hover_t(&resp, &theme);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);
    let half = stroke::FIELD_FOCUS / 2.0;
    let r = f32::from(cover::corner(rect).nw);
    painter.rect_filled(rect, cover::corner(rect), fade(p.o1, hover));
    painter.extend(Shape::dashed_line(
        &rounded_path(rect.shrink(half), (r - half).max(0.0)),
        Stroke::new(stroke::FIELD_FOCUS, mix(p.line2, p.t3, hover)),
        overlay::DROP_DASH[0],
        overlay::DROP_DASH[1],
    ));
    let ink = mix(p.t3, p.t1, hover);
    let label = Type::BodyStrong.line(ui, strings::library::ADD_GAME(), ink);
    let gap = layout::WORD_PAD;
    let h = icon_size::INLINE + gap + label.size().y;
    let top = rect.center().y - h / 2.0;
    painter.text(
        pos2(rect.center().x, top + icon_size::INLINE / 2.0),
        Align2::CENTER_CENTER,
        icons::PLUS.text(false),
        icons::font(icon_size::INLINE),
        ink,
    );
    painter.galley(
        pos2(
            rect.center().x - label.size().x / 2.0,
            top + icon_size::INLINE + gap,
        ),
        label,
        ink,
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(&painter, rect, cover::corner(rect), &theme);
    }
    resp
}

// ── The wall ────────────────────────────────────────────────────────────────────────────────

/// Show the wall; returns what the user asked for. `size` is the library's size (games not
/// hidden), `view_h` the height of the page's content box, `origin` the top left of the
/// page's content (where reflow measures from).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    shelf: &Shelf,
    size: usize,
    view_h: f32,
    origin: Pos2,
) -> Option<(GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let visible: &[u32] = &shelf.items;
    let sparse = state.library.sparse(size);
    let cells = visible.len() + usize::from(sparse);
    let avail_w = ui.available_width();
    let clip = ui.clip_rect();
    let g = GridLayout::new(avail_w, theme.density, size, sparse, cells, view_h);
    let (rows, total_h) = layout_rows(&g, cells, &shelf.groups);
    let card_rows: Vec<(usize, usize)> = rows
        .iter()
        .filter_map(|(r, _)| match r {
            Row::Cards { start, len } => {
                Some((*start, (*len).min(visible.len().saturating_sub(*start))))
            }
            Row::Header(_) => None,
        })
        .filter(|(_, l)| *l > 0)
        .collect();
    let mut action = None;
    let ppp = ui.ctx().pixels_per_point();
    let thumb = ThumbSize::cover_for(g.card_w * ppp);
    let prefs = env.bridge.prefs().clone();
    let show_playtime = prefs.appearance.show_playtime_on_cards;
    let single_click_plays = prefs.integration.single_click_play;
    let game_at = |pos: usize| lib.games[visible[pos] as usize].id;

    // Keyboard: the covers are one tab stop; arrows move inside it.
    let sel_pos = state
        .library
        .selected
        .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
    let focused = ui.ctx().memory(|m| m.focused());
    let wall_has_focus = sel_pos.is_some_and(|p| focused == Some(card_id(game_at(p))));
    state.library.focused = wall_has_focus;
    let mut scroll_to: Option<usize> = None;
    if wall_has_focus && let Some(pos) = sel_pos {
        let (mv, enter, space) = ui.input_mut(|i| {
            (
                focus::take_move(i),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Space),
            )
        });
        let page = ((clip.height() / g.row_h().max(1.0)).floor() as usize).max(1);
        if let Some((mv, shift)) = mv {
            // Left and right run through the order; Home and End go to the ends.
            let t = match mv {
                GridMove::Left => pos.checked_sub(1),
                GridMove::Right => (pos + 1 < visible.len()).then_some(pos + 1),
                GridMove::RowStart | GridMove::First => (pos > 0).then_some(0),
                GridMove::RowEnd | GridMove::Last => {
                    (pos + 1 < visible.len()).then(|| visible.len() - 1)
                }
                other => focus::target(&card_rows, pos, other, page),
            };
            if let Some(t) = t {
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
        } else if space {
            state.library.toggle_select(game_at(pos));
        }
    }
    // A pending focus outside the viewport scrolls it into view first.
    if scroll_to.is_none()
        && let Some(pf) = state.pending_focus
        && let Some(t) = visible.iter().position(|i| lib.games[*i as usize].id == pf)
    {
        scroll_to = Some(t);
    }

    let reflow = Reflow::begin(
        ui.ctx(),
        state.library.generation(),
        ViewMode::Grid,
        state.library.typing,
    );
    let mut flow = reflow.lock();
    let generation = state.library.generation();
    let grid_name = strings::library::grid_name(visible.len(), g.cols);
    let menu_game = state.library.menu.map(|(id, _)| id);
    let edge = clip.right() - layout::CAPTION_EDGE;
    ui.scope_builder(UiBuilder::new().id_salt("library.grid"), |ui| {
        a11y::labelled(ui.ctx(), ui.scope_id(), Role::Grid, &grid_name);
        let (area, _) = ui.allocate_exact_size(vec2(avail_w, g.top + total_h), Sense::hover());
        let wall = area.min + vec2(g.left, g.top);
        // The first row's ring (3 out, 2 or 3 wide) and its lift reach above the page's
        // top edge: while that row stands below the edge, the covers may paint that far
        // up. The allowance shrinks to nothing as the row reaches the edge, so nothing
        // that scrolls under it shows above it.
        let overhang =
            stroke::COVER_RING_GAP + theme.focus_width() + layout::COVER_LIFT + stroke::BORDER;
        let room = ((wall.y - clip.top()) / layout::PAGE_PAD[0]).clamp(0.0, 1.0);
        let mut painted = clip;
        painted.min.y -= overhang * room;
        ui.set_clip_rect(painted);
        let view_top = clip.top() - wall.y;
        let view_bottom = clip.bottom() - wall.y;
        let view_h_now = view_bottom - view_top;
        let near_view = clip.expand(travel::REFLOW_MARGIN);
        let cell = |row_y: f32, col: usize| {
            Rect::from_min_size(
                pos2(wall.x + col as f32 * (g.card_w + g.gap), wall.y + row_y),
                vec2(g.card_w, g.cover_h),
            )
        };
        if let Some(t) = scroll_to
            && let Some((Row::Cards { start, .. }, y)) = rows.iter().find(
                |(r, _)| matches!(r, Row::Cards { start, len } if t >= *start && t < start + len),
            )
        {
            ui.scroll_to_rect(cell(*y, t - start).expand(layout::CAPTION_GAP * 2.0), None);
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
                let end = (start + len).min(visible.len());
                let (a, b) = laid_out.unwrap_or((*start, end));
                laid_out = Some((a.min(*start), b.max(end)));
            }
        }
        let pending_pos = state
            .pending_focus
            .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
        let laid_out = laid_out.map_or(0..0, |(a, b)| a..b);
        let rover = focus::rover(pending_pos.or(sel_pos), laid_out);
        // A page in flight shows what is cached and asks for nothing.
        let jumped = super::page_jumped(ui, view_top, view_h_now);
        let prefetch_to = if jumped {
            view_bottom + g.row_h()
        } else {
            view_bottom + view_h_now.max(0.0)
        };
        let selection_active = !state.library.selection.is_empty();
        let look = WallLook {
            big: g.big,
            sparse: g.sparse,
            menu_open: false,
            caption_edge: edge,
            hidden: false,
        };
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
                    let span = &shelf.groups[*gi];
                    let a = motion::since_change(
                        ui.ctx(),
                        Id::unique(("library.heading", &*span.label)),
                        generation,
                        theme.motion.fade(Dur::Base),
                        true,
                    )
                    .map_or(1.0, |t| t / theme.motion.fade(Dur::Base));
                    let top = wall.y + y;
                    let line = Type::BodyStrong.line_height();
                    let label = Type::BodyStrong.line(ui, &span.label, p.t3);
                    let label_w = label.size().x;
                    let mut painter = ui.painter().clone();
                    painter.multiply_opacity(a);
                    painter.galley(
                        pos2(area.left(), top + (line - label.size().y) / 2.0),
                        label,
                        p.t3,
                    );
                    let count = Type::MicroStrong.numeric_galley(ui, &span.len.to_string(), p.t3);
                    painter.galley(
                        pos2(
                            area.left() + label_w + layout::GROUP_COUNT_GAP,
                            top + (line - count.size().y) / 2.0,
                        ),
                        count,
                        p.t3,
                    );
                }
                Row::Cards { start, len } => {
                    for k in 0..*len {
                        let pos = start + k;
                        let target = cell(*y, k);
                        if pos >= visible.len() {
                            // The "Add game" tile.
                            if on_screen {
                                let a = motion::since_change(
                                    ui.ctx(),
                                    Id::unique("library.add_tile.in"),
                                    generation,
                                    theme.motion.fade(Dur::Base),
                                    true,
                                )
                                .map_or(1.0, |t| t / theme.motion.fade(Dur::Base));
                                if add_tile(ui, target, a).clicked() {
                                    crate::screens::add_game::open(state);
                                }
                            }
                            continue;
                        }
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
                        let rel = target.translate(-origin.to_vec2());
                        let placed = flow.place(game.id, rel, near_view.intersects(target));
                        let rect = placed.rect.translate(origin.to_vec2());
                        state.flight.note_wall(game.id, rect);
                        let (texture, placeholder) = match game.cover {
                            Some(art) => {
                                let key = TexKey { art, size: thumb };
                                let s = if jumped {
                                    env.textures.peek(key, true)
                                } else {
                                    env.textures.get(key, true, env.art)
                                };
                                // While the larger size loads, the list's size stands in (a
                                // cover arriving from its row keeps its picture).
                                let s = match s {
                                    TexState::Pending(c) if thumb != ThumbSize::Cover240 => {
                                        match env.textures.peek(
                                            TexKey {
                                                art,
                                                size: ThumbSize::Cover240,
                                            },
                                            false,
                                        ) {
                                            TexState::Ready(t) => TexState::Ready(t),
                                            TexState::Pending(_) => TexState::Pending(c),
                                        }
                                    }
                                    s => s,
                                };
                                match s {
                                    TexState::Ready(t) => (Some(t), None),
                                    TexState::Pending(c) => (None, c),
                                }
                            }
                            None => (None, None),
                        };
                        let ctx = CardCtx {
                            texture,
                            placeholder,
                            now: env.now,
                            utc_offset_s: env.utc_offset_s,
                            selected: state.library.selected == Some(game.id),
                            multi_selected: state.library.is_selected(game.id),
                            controller: state.controller,
                            focusable: rover == Some(pos),
                            show_playtime,
                            density: theme.density,
                            opacity: placed.opacity,
                        };
                        let id = card_id(game.id);
                        let look = WallLook {
                            menu_open: menu_game == Some(game.id),
                            hidden: state.flight.hides(game.id),
                            ..look
                        };
                        let r = wall_card(ui, rect, id, game, &ctx, &look);
                        // A Steam game's cover is Steam's: an image dropped on it is not
                        // taken (ADR-0031).
                        if !game.is_steam() {
                            state
                                .dnd
                                .target(rect, crate::input::dnd::DropTarget::Cover(game.id));
                        }
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
                        if r.play {
                            action = Some((game.id, GameAction::Play));
                        } else if r.response.secondary_clicked() {
                            let at = r.response.interact_pointer_pos().unwrap_or(rect.center());
                            state.library.cursor_rect = Some(rect);
                            super::open_menu_at(state, game.id, at);
                        } else if r.response.clicked() {
                            let m = ui.input(|i| i.modifiers);
                            let selecting = selection_active || m.shift || m.command;
                            if selecting {
                                state
                                    .library
                                    .click(lib, visible, game.id, m.shift, m.command);
                            } else if single_click_plays {
                                action = Some((game.id, GameAction::Play));
                            } else {
                                action = Some((game.id, GameAction::Open));
                            }
                        }
                    }
                }
            }
        }
    });
    flow.end(ui.ctx());
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(avail: f32, size: usize) -> GridLayout {
        GridLayout::new(avail, Density::Comfortable, size, false, size, 670.0)
    }

    /// One frame of the page at `time`, placing `placed` and returning where they went.
    fn frame(
        ctx: &egui::Context,
        time: f64,
        generation: u64,
        placed: &[(GameId, Rect)],
    ) -> Vec<Placed> {
        let mut out = Vec::new();
        let input = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut o = ctx.run_ui(input, |ui| {
            let r = Reflow::begin(ui.ctx(), generation, ViewMode::Grid, false);
            let mut f = r.lock();
            out = placed
                .iter()
                .map(|(id, t)| f.place(*id, *t, true))
                .collect();
            f.end(ui.ctx());
        });
        o.textures_delta.clear();
        out
    }

    #[test]
    fn covers_slide_to_their_new_places_and_new_ones_rise_in() {
        let ctx = egui::Context::default();
        theme::install(&ctx, &theme::Appearance::default());
        let a = GameId::from_uuid(uuid::Uuid::from_u128(1));
        let b = GameId::from_uuid(uuid::Uuid::from_u128(2));
        let here = Rect::from_min_size(pos2(0.0, 0.0), vec2(164.0, 246.0));
        let there = here.translate(vec2(180.0, 0.0));
        // The first paint: covers rise in, 18 ms apart.
        let first = frame(&ctx, 0.0, 1, &[(a, here), (b, there)]);
        assert_eq!(first[0].opacity, 0.0);
        let later = frame(&ctx, 1.0, 1, &[(a, here), (b, there)]);
        assert_eq!(later[0].opacity, 1.0);
        assert_eq!(later[0].rect, here);
        // The order changes: `a` slides from where it was over 300 ms.
        let moved = frame(&ctx, 1.0, 2, &[(a, there)]);
        assert_eq!(moved[0].rect, here);
        let half = frame(&ctx, 1.05, 2, &[(a, there)]);
        assert!(half[0].rect.left() > here.left() && half[0].rect.left() < there.left());
        let done = frame(&ctx, 1.4, 2, &[(a, there)]);
        assert_eq!(done[0].rect, there);
        // `b` left the wall and comes back: it rises in again.
        let back = frame(&ctx, 1.4, 3, &[(a, there), (b, here)]);
        assert!(back[1].opacity < 1.0);
        // A resize (same order) moves covers at once.
        let resized = there.translate(vec2(10.0, 0.0));
        let snap = frame(&ctx, 2.0, 3, &[(a, resized), (b, here)]);
        assert_eq!(snap[0].rect, resized);
    }

    #[test]
    fn covers_stretch_to_fill_the_row() {
        let g = wall(1224.0, 17);
        // floor((1224 + 16) / (164 + 16)) = 6
        assert_eq!(g.cols, 6);
        assert!((g.card_w * 6.0 + 16.0 * 5.0 - 1224.0).abs() < 0.01);
        assert!((g.cover_h - g.card_w * 1.5).abs() < 0.01);
        assert_eq!(g.row_h(), g.cover_h + 36.0);
        assert_eq!(g.rows(11), 2);
        let narrow = GridLayout::new(100.0, Density::Large, 24, false, 24, 600.0);
        assert_eq!(narrow.cols, 1);
        assert!(narrow.card_w <= 100.0);
    }

    #[test]
    fn small_libraries_get_larger_covers() {
        assert_eq!(cover_width(Density::Comfortable, 1), 280.0);
        assert_eq!(cover_width(Density::Comfortable, 3), 240.0);
        assert_eq!(cover_width(Density::Comfortable, 6), 208.0);
        assert_eq!(cover_width(Density::Compact, 7), 132.0);
        assert_eq!(cover_width(Density::Large, 5), 208.0);
    }

    #[test]
    fn a_sparse_wall_keeps_its_width_and_a_big_one_is_centred() {
        let six = GridLayout::new(1224.0, Density::Comfortable, 6, true, 7, 670.0);
        assert_eq!(six.card_w, 208.0);
        assert_eq!((six.gap, six.row_gap), (20.0, 60.0));
        assert!(!six.big);
        let three = GridLayout::new(1224.0, Density::Comfortable, 3, true, 4, 670.0);
        assert!(three.big);
        // Four cells of 240 and three gaps of 20, centred.
        assert!((three.left - (1224.0 - (4.0 * 240.0 + 60.0)) / 2.0).abs() < 0.01);
        // The covers' middle at 46 % of the page's content box.
        assert!((three.top - (0.46 * 670.0 - 180.0 - 30.0)).abs() < 0.01);
    }

    #[test]
    fn rows_with_group_headings() {
        let g = wall(1224.0, 24);
        let span = |start, len| Section {
            label: Arc::from("A"),
            start,
            len,
        };
        let (rows, h) = layout_rows(&g, 7, &[span(0, 6), span(6, 1)]);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, Row::Header(0));
        assert_eq!(rows[1].0, Row::Cards { start: 0, len: 6 });
        assert_eq!(rows[2].0, Row::Header(1));
        // 44 between the first group's covers and the next heading.
        assert!((rows[2].1 - (rows[1].1 + g.cover_h + 44.0)).abs() < 0.01);
        assert!((h - (rows[3].1 + g.cover_h)).abs() < 0.01);
    }
}
