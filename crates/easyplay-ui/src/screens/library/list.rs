//! List view (SCREEN_SPECS §2, UI-010): `art · Name · columns… · ▶`, virtualized by row
//! inside the library's scroll area. The column header sticks to the top of the viewport
//! while the page heading scrolls away. Headers sort (a second click flips the direction; the
//! state is announced), the chooser shows or hides columns, header edges drag to resize.
//! `↑`/`↓` move the cursor, `←`/`→` move between a row and its Play button.

use std::sync::Arc;

use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, GameSummary};
use egui::accesskit::Role;
use egui::epaint::RectShape;
use egui::{
    Align, Color32, CornerRadius, Key, Layout, Modifiers, Popup, Rect, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::{LibraryView, UiCommand};
use crate::input::focus;
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::fade;
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Dur, Type, icons};
use crate::vm::library::Column;
use crate::widgets::button::{PillSize, play_pill};
use crate::widgets::card::play_state;
use crate::widgets::context_menu::{self, check_item};
use crate::widgets::cover::{CoverSource, paint_cover, paint_edge};
use crate::widgets::icon_button::IconButton;
use crate::widgets::shield;

/// The stable id of a game's row.
pub fn row_id(game: GameId) -> egui::Id {
    egui::Id::unique(("row", game))
}

/// Space the row's art takes before the name.
const LEAD: f32 = layout::LIST_THUMB + space::S3;

fn head(ui: &mut Ui, label: &str, w: f32, active: bool, desc: bool) -> egui::Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let text = if active {
        strings::library::sorted_header(label, desc)
    } else {
        label.to_owned()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, layout::ROW_H_COMPACT), Sense::click());
    let color = if active || resp.hovered() {
        p.text_primary
    } else {
        p.text_muted
    };
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        &text,
        Type::Label.font_id(),
        color,
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(
            ui.painter(),
            rect.shrink2(vec2(0.0, space::S2)),
            CornerRadius::same(radius::XS),
            &theme::theme(ui.ctx()),
        );
    }
    a11y::option(&resp, Role::ColumnHeader, &text, active);
    resp
}

fn header(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, name_w: f32, cols: &[Column]) {
    let sort = state.library.sort;
    let desc = state.library.descending;
    let mut clicked: Option<(easyplay_core::library::SortKey, &str)> = None;
    ui.spacing_mut().item_spacing.x = space::S3;
    ui.add_space(LEAD);
    let name_key = easyplay_core::library::SortKey::Name;
    if head(
        ui,
        strings::library::COL_NAME,
        name_w,
        sort == name_key,
        desc,
    )
    .clicked()
    {
        clicked = Some((name_key, strings::library::COL_NAME));
    }
    for c in cols {
        let w = state.library.width(*c);
        let r = head(ui, c.label(), w, c.sort() == Some(sort), desc);
        if r.clicked()
            && let Some(k) = c.sort()
        {
            clicked = Some((k, c.label()));
        }
        // Drag the right edge to resize.
        let edge = Rect::from_min_max(
            pos2(r.rect.right() - 3.0, r.rect.top()),
            pos2(r.rect.right() + 3.0, r.rect.bottom()),
        );
        let e = ui.interact(edge, r.id.with("resize"), Sense::drag());
        if e.hovered() || e.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if e.dragged() {
            let nw = (w + e.drag_delta().x).clamp(60.0, 360.0);
            state.library.widths.insert(*c, nw);
        }
        if e.drag_stopped() {
            let value = easyplay_core::config::ColumnWidth::to_value(&state.library.width_prefs());
            let patch = easyplay_core::config::PreferencePatch::new()
                .set("appearance.list_column_widths", value);
            let mut preview = (**env.bridge.prefs()).clone();
            preview.appearance.list_column_widths = state.library.width_prefs();
            env.bridge.send(UiCommand::SetPreference {
                patch,
                preview: Box::new(preview),
            });
        }
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let r = ui.add(IconButton::new(icons::DOTS, strings::library::COLUMNS));
        context_menu::popup(
            Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
            |ui| {
                ui.set_min_width(200.0);
                for c in Column::ALL {
                    let on = state.library.columns.contains(&c);
                    if check_item(ui, c.label(), on).clicked() {
                        if on {
                            state.library.columns.retain(|x| *x != c);
                        } else {
                            state.library.columns.push(c);
                            let order = |x: &Column| Column::ALL.iter().position(|y| y == x);
                            state.library.columns.sort_by_key(order);
                        }
                    }
                }
            },
        );
    });
    if let Some((k, label)) = clicked {
        state.library.set_sort(k, true);
        a11y::Announcer::announce(
            ui.ctx(),
            strings::library::sorted_by(label, state.library.descending),
            a11y::Politeness::Polite,
        );
    }
}

/// The art of a row: the game's icon, else its cover cropped square, else the generated
/// cover.
fn thumb(
    ui: &Ui,
    env: &mut Env<'_>,
    rect: Rect,
    g: &GameSummary,
    theme: &theme::Theme,
    jumped: bool,
) {
    let corner = CornerRadius::same(radius::SM);
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    // The middle square of a 2:3 cover.
    let third = (1.0 - 1.0 / layout::COVER_ASPECT) / 2.0;
    let square = Rect::from_min_max(pos2(0.0, third), pos2(1.0, 1.0 - third));
    let source = g
        .icon
        .map(|art| (art, ThumbSize::Icon64, full))
        .or(g.cover.map(|art| (art, ThumbSize::Cover240, square)));
    let painted = source.is_some_and(|(art, size, uv)| {
        let key = TexKey { art, size };
        // A page in flight shows what is cached and asks for nothing.
        let state = if jumped {
            env.textures.peek(key, true)
        } else {
            env.textures.get(key, true, env.art)
        };
        match state {
            TexState::Ready(tex) => {
                ui.painter()
                    .add(RectShape::filled(rect, corner, Color32::WHITE).with_texture(tex, uv));
                true
            }
            TexState::Pending(Some(c)) => {
                ui.painter().rect_filled(rect, corner, c);
                true
            }
            TexState::Pending(None) => false,
        }
    });
    if !painted {
        paint_cover(
            ui.painter(),
            rect,
            corner,
            CoverSource::Placeholder,
            &g.name,
            theme,
        );
    }
    paint_edge(ui.painter(), rect, corner, 0.0, theme);
}

/// Show the list; returns what the user asked for.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
) -> Option<(GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut action = None;
    let avail_w = ui.available_width();
    let wide = avail_w > 720.0;
    let prefs = Arc::clone(env.bridge.prefs());
    state
        .library
        .load_widths(&prefs.appearance.list_column_widths);
    let cols: Vec<Column> = if wide {
        state.library.columns.clone()
    } else {
        state
            .library
            .columns
            .iter()
            .copied()
            .filter(|c| matches!(c, Column::LastPlayed))
            .collect()
    };
    let fixed: f32 = cols
        .iter()
        .map(|c| state.library.width(*c) + space::S3)
        .sum();
    let name_w = (avail_w - fixed - LEAD - 150.0).max(120.0);
    let row_h = layout::LIST_ROW_H;
    let head_h = layout::ROW_H_COMPACT;
    let clip = ui.clip_rect();

    // The column header: at its place in the page until that scrolls past the top of the
    // viewport, then stuck there. Rows are clipped below it.
    let (head_slot, _) = ui.allocate_exact_size(vec2(avail_w, head_h), Sense::hover());
    let stuck = head_slot.top() < clip.top();
    let head_rect = Rect::from_min_size(
        pos2(head_slot.left(), head_slot.top().max(clip.top())),
        head_slot.size(),
    );
    // A child that does not move the page's cursor: the rows keep their place while the
    // header is stuck.
    let mut head_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(head_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    header(&mut head_ui, state, env, name_w, &cols);
    let line = if stuck { p.hairline } else { p.border_subtle };
    ui.painter().hline(
        if stuck {
            clip.x_range()
        } else {
            head_rect.x_range()
        },
        head_rect.bottom() - 0.5,
        Stroke::new(stroke::BORDER, line),
    );

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
            action = Some((id, GameAction::Play));
        }
    }
    // A pending focus outside the viewport scrolls it into view first.
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
        // Clear of the stuck header above and of the window edge below.
        let r = row_rect(t);
        ui.scroll_to_rect(
            Rect::from_min_max(
                pos2(r.left(), r.top() - head_h - space::S1),
                pos2(r.right(), r.bottom() + space::S1),
            ),
            None,
        );
    }
    let rows_clip = Rect::from_min_max(pos2(clip.left(), head_rect.bottom()), clip.max);
    let first =
        (((rows_clip.top() - area.top()) / row_h).floor().max(0.0) as usize).min(visible.len());
    let last =
        (((rows_clip.bottom() - area.top()) / row_h).ceil().max(0.0) as usize).min(visible.len());
    let range = first..last;
    let jumped = super::page_jumped(ui, rows_clip.top() - area.top(), rows_clip.height());
    ui.scope_builder(UiBuilder::new().id_salt("library.list"), |ui| {
        ui.set_clip_rect(rows_clip);
        a11y::labelled(ui.ctx(), ui.scope_id(), Role::Table, strings::library::GRID);
        let pending_pos = state
            .pending_focus
            .and_then(|g| visible.iter().position(|i| lib.games[*i as usize].id == g));
        let rover = focus::rover(pending_pos.or(pos), range.clone());
        for pos in range {
            let g = &lib.games[visible[pos] as usize];
            let rect = row_rect(pos);
            let id = row_id(g.id);
            // Roving focus: only the cursor's row (or the first laid out) is in the `Tab`
            // order (ACCESSIBILITY §2.1).
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
                g.preset,
                state_text.as_deref(),
                g.total_playtime_ms,
                g.last_played_at,
                env.now,
                env.utc_offset_s,
            );
            let multi = state.library.is_selected(g.id);
            a11y::option(
                &resp,
                Role::Row,
                &name,
                state.library.selected == Some(g.id) || multi,
            );
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
            let hot = resp.contains_pointer() || resp.has_focus() || play_has_focus;
            let hot_t =
                ui.ctx()
                    .animate_bool_with_time(id.with("hot"), hot, theme.motion.fade(Dur::Fast));
            // The fill reaches into the page margin, so the art stays on the page's axis.
            let plate = rect.expand2(vec2(space::S2, 0.0)).shrink2(vec2(0.0, 1.0));
            let corner = CornerRadius::same(radius::ROW);
            let painter = ui.painter().clone();
            if multi {
                painter.rect_filled(plate, corner, p.accent_subtle);
            } else if hot_t > 0.0 {
                painter.rect_filled(plate, corner, fade(p.fill_hover, hot_t));
            }
            if resp.has_focus() {
                crate::widgets::focus_ring(&painter, plate.shrink(1.0), corner, &theme);
            }
            if !multi {
                // A hairline under the text columns; it steps aside for the hovered row.
                painter.hline(
                    rect.left() + LEAD..=rect.right(),
                    rect.bottom() - 0.5,
                    Stroke::new(stroke::BORDER, fade(p.border_subtle, 1.0 - hot_t)),
                );
            }
            state
                .dnd
                .target(rect, crate::input::dnd::DropTarget::Cover(g.id));
            let mut row_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            {
                let ui = &mut row_ui;
                {
                    ui.spacing_mut().item_spacing.x = space::S3;
                    let (ir, _) = ui.allocate_exact_size(
                        vec2(layout::LIST_THUMB, layout::LIST_THUMB),
                        Sense::hover(),
                    );
                    thumb(ui, env, ir, g, &theme, jumped);
                    ui.allocate_ui_with_layout(
                        vec2(name_w, row_h),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.set_width(name_w);
                            ui.add(
                                egui::Label::new(
                                    Type::BodyStrong.rich(&*g.name).color(p.text_primary),
                                )
                                .truncate(),
                            );
                        },
                    );
                    for c in &cols {
                        let w = state.library.width(*c);
                        ui.allocate_ui_with_layout(
                            vec2(w, row_h),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_width(w);
                                cell(ui, *c, g, lib, env, state_text.as_deref(), &p);
                            },
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Out of sight until the row is pointed at or focused; a game that
                        // is running or starting keeps its control.
                        let shown = if g.activity != Activity::Idle || state.controller {
                            1.0
                        } else {
                            hot_t
                        };
                        let pill = play_pill(ui, &play_state(g), &g.name, PillSize::Row, shown);
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(id.with("play_id"), pill.id));
                        if pill.clicked() {
                            action = Some((g.id, GameAction::Play));
                        }
                    });
                }
            }
            if action.is_none() {
                if resp.secondary_clicked() {
                    let at = resp.interact_pointer_pos().unwrap_or(rect.center());
                    super::open_menu(state, g.id, at);
                } else if resp.double_clicked() {
                    action = Some((g.id, GameAction::Play));
                } else if resp.clicked() {
                    let m = ui.input(|i| i.modifiers);
                    if m.command {
                        state.library.toggle_select(g.id);
                    } else if m.shift {
                        state.library.select_range(lib, visible, g.id);
                    } else if selection_active {
                        state.library.toggle_select(g.id);
                    } else {
                        action = Some((g.id, GameAction::Open));
                    }
                }
            }
        }
    });
    action
}

/// One optional column's cell.
fn cell(
    ui: &mut Ui,
    column: Column,
    g: &GameSummary,
    lib: &LibraryView,
    env: &Env<'_>,
    state_text: Option<&str>,
    p: &theme::Palette,
) {
    let muted = |ui: &mut Ui, text: String| {
        ui.add(egui::Label::new(Type::Body.numeric(text).color(p.text_muted)).truncate());
    };
    match column {
        Column::Protection => {
            shield::badge(
                ui,
                g.preset,
                g.flags.needs_attention || g.flags.suspected_tamper,
                icon_size::INLINE,
            );
            ui.label(
                Type::Body
                    .rich(strings::shield::preset_name(g.preset))
                    .color(p.text_secondary),
            );
        }
        Column::LastPlayed => {
            let active = g.activity != Activity::Idle;
            let t = state_text.map(str::to_owned).unwrap_or_else(|| {
                g.last_played_at.map_or_else(
                    || strings::card::NOT_PLAYED.to_owned(),
                    |t| strings::fmt::relative_date(t, env.now, env.utc_offset_s),
                )
            });
            let color = if active { p.text_primary } else { p.text_muted };
            ui.add(egui::Label::new(Type::Body.numeric(t).color(color)).truncate());
        }
        Column::Playtime => {
            let t = if g.total_playtime_ms > 0 {
                strings::fmt::duration(g.total_playtime_ms)
            } else {
                "—".to_owned()
            };
            muted(ui, t);
        }
        Column::Tags => {
            let names: Vec<&str> = g
                .tags
                .iter()
                .filter_map(|t| lib.tag(*t))
                .map(|t| &*t.name)
                .collect();
            ui.add(
                egui::Label::new(Type::Body.rich(names.join(", ")).color(p.text_secondary))
                    .truncate(),
            );
        }
        Column::Layer => {
            ui.add(
                egui::Label::new(Type::Body.rich(&*g.runtime_label).color(p.text_secondary))
                    .truncate(),
            );
        }
        Column::Added => {
            muted(
                ui,
                strings::fmt::relative_date(g.added_at, env.now, env.utc_offset_s),
            );
        }
    }
}
