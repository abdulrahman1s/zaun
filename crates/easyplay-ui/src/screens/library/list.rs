//! List view (SCREEN_SPECS §2, UI-010): `icon · Name · columns… · ▶`, rows through
//! `ScrollArea::show_rows`. Headers sort (a second click flips the direction; the state is
//! announced), the chooser shows or hides columns, header edges drag to resize. `↑`/`↓` move
//! the cursor, `←`/`→` move between a row and its Play button.

use std::sync::Arc;

use egui::accesskit::Role;
use egui::{
    Align, CornerRadius, Key, Layout, Modifiers, Popup, ScrollArea, Sense, Stroke, Ui, vec2,
};

use crate::a11y;
use crate::app::UiState;
use crate::bridge::LibraryView;
use crate::screens::Env;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Type, icons};
use crate::vm::library::Column;
use crate::widgets::button::{PillSize, play_pill};
use crate::widgets::card::play_state;
use crate::widgets::context_menu::check_item;
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::icon_button::IconButton;
use crate::widgets::shield;

/// The stable id of a game's row.
pub fn row_id(game: easyplay_core::ids::GameId) -> egui::Id {
    egui::Id::unique(("row", game))
}

fn head(ui: &mut Ui, label: &str, w: f32, active: bool, desc: bool) -> egui::Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let text = if active {
        strings::library::sorted_header(label, desc)
    } else {
        label.to_owned()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, layout::ROW_H_COMPACT), Sense::click());
    let color = if active { p.text_primary } else { p.text_muted };
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        &text,
        Type::Micro.font_id(),
        color,
    );
    a11y::option(&resp, Role::ColumnHeader, &text, active);
    resp
}

fn header(ui: &mut Ui, state: &mut UiState, name_w: f32, cols: &[Column]) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let sort = state.library.sort;
    let desc = state.library.descending;
    let mut clicked: Option<(easyplay_core::library::SortKey, &str)> = None;
    ui.horizontal(|ui| {
        ui.set_height(layout::ROW_H_COMPACT);
        ui.spacing_mut().item_spacing.x = space::S3;
        ui.add_space(space::S2 + icon_size::NAV + 4.0 + space::S3);
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
            let edge = egui::Rect::from_min_max(
                egui::pos2(r.rect.right() - 3.0, r.rect.top()),
                egui::pos2(r.rect.right() + 3.0, r.rect.bottom()),
            );
            let e = ui.interact(edge, r.id.with("resize"), Sense::drag());
            if e.hovered() || e.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if e.dragged() {
                let nw = (w + e.drag_delta().x).clamp(60.0, 360.0);
                state.library.widths.insert(*c, nw);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = ui.add(IconButton::new(icons::DOTS, strings::library::COLUMNS));
            Popup::menu(&r).show(|ui| {
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
            });
        });
    });
    if let Some((k, label)) = clicked {
        state.library.set_sort(k, true);
        a11y::Announcer::announce(
            ui.ctx(),
            strings::library::sorted_by(label, state.library.descending),
            a11y::Politeness::Polite,
        );
    }
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        r.x_range(),
        r.center().y,
        Stroke::new(stroke::BORDER, p.border_subtle),
    );
}

/// Show the list; returns what the user asked for.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    lib: &Arc<LibraryView>,
    visible: &Arc<[u32]>,
) -> Option<(easyplay_core::ids::GameId, GameAction)> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut action = None;
    let wide = ui.available_width() > 720.0;
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
    let name_w = (ui.available_width() - fixed - 200.0).max(140.0);
    header(ui, state, name_w, &cols);

    // Keyboard.
    let focused = ui.ctx().memory(|m| m.focused());
    let pos = state
        .library
        .selected
        .and_then(|id| visible.iter().position(|i| lib.games[*i as usize].id == id));
    let row_focus = pos.is_some_and(|p| focused == Some(row_id(lib.games[visible[p] as usize].id)));
    let play_id = |g: easyplay_core::ids::GameId| -> Option<egui::Id> {
        ui.ctx().data(|d| d.get_temp(row_id(g).with("play_id")))
    };
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
        let (up, down, left, right, enter, space_key) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                row_focus && i.consume_key(Modifiers::NONE, Key::Enter),
                row_focus && i.consume_key(Modifiers::NONE, Key::Space),
            )
        });
        let t = if up && p > 0 {
            Some(p - 1)
        } else if down && p + 1 < visible.len() {
            Some(p + 1)
        } else {
            None
        };
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

    let row_h = layout::ROW_H;
    let salt = egui::IdSalt::new(("library.list", state.library.collection));
    let mut area = ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false]);
    if let Some(t) = scroll_to {
        let sa_id = ui.make_persistent_id(salt);
        let cur =
            egui::scroll_area::State::load(ui.ctx(), sa_id).map_or(0.0, |s| s.clamped_offset().y);
        let top = t as f32 * row_h;
        let view_h = ui.available_height();
        if top < cur {
            area = area.vertical_scroll_offset(top);
        } else if top + row_h > cur + view_h {
            area = area.vertical_scroll_offset(top + row_h - view_h);
        }
    }
    let selection_active = !state.library.selection.is_empty();
    area.show_rows(ui, row_h, visible.len(), |ui, range| {
        a11y::labelled(ui.ctx(), ui.scope_id(), Role::Table, strings::library::GRID);
        for pos in range {
            let g = &lib.games[visible[pos] as usize];
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
            let id = row_id(g.id);
            let resp = ui.interact(rect, id, Sense::click());
            let state_text = strings::card::state(g.activity, g.availability, env.now);
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
            if state.pending_focus == Some(g.id) {
                resp.request_focus();
                state.pending_focus = None;
            }
            if resp.has_focus() {
                state.library.selected = Some(g.id);
                state.library.cursor_rect = Some(rect);
            }
            let painter = ui.painter();
            if multi {
                painter.rect_filled(rect, CornerRadius::same(radius::SM), p.accent_subtle);
            } else if resp.hovered() || resp.has_focus() {
                painter.rect_filled(rect, CornerRadius::same(radius::SM), p.bg_surface);
            }
            if resp.has_focus() {
                crate::widgets::focus_ring(
                    painter,
                    rect.shrink(1.0),
                    CornerRadius::same(radius::SM),
                    &theme,
                );
            }
            painter.hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(stroke::BORDER, p.border_subtle),
            );
            state
                .dnd
                .target(rect, crate::input::dnd::DropTarget::Cover(g.id));
            let inner = rect.shrink2(vec2(space::S2, 0.0));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = space::S3;
                    let (ir, _) = ui.allocate_exact_size(
                        vec2(icon_size::NAV + 4.0, icon_size::NAV + 4.0),
                        Sense::hover(),
                    );
                    paint_cover(
                        ui.painter(),
                        ir,
                        CornerRadius::same(radius::XS),
                        CoverSource::Placeholder,
                        &g.name,
                        &theme,
                    );
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
                                match c {
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
                                        let t = state_text.clone().unwrap_or_else(|| {
                                            g.last_played_at.map_or_else(
                                                || strings::card::NOT_PLAYED.to_owned(),
                                                |t| {
                                                    strings::fmt::relative_date(
                                                        t,
                                                        env.now,
                                                        env.utc_offset_s,
                                                    )
                                                },
                                            )
                                        });
                                        ui.add(
                                            egui::Label::new(
                                                Type::Body.numeric(t).color(p.text_muted),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    Column::Playtime => {
                                        let t = if g.total_playtime_ms > 0 {
                                            strings::fmt::duration(g.total_playtime_ms)
                                        } else {
                                            "—".to_owned()
                                        };
                                        ui.label(Type::Body.numeric(t).color(p.text_muted));
                                    }
                                    Column::Tags => {
                                        let names: Vec<&str> = g
                                            .tags
                                            .iter()
                                            .filter_map(|t| lib.tag(*t))
                                            .map(|t| &*t.name)
                                            .collect();
                                        ui.add(
                                            egui::Label::new(
                                                Type::Caption
                                                    .rich(names.join(", "))
                                                    .color(p.text_secondary),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    Column::Layer => {
                                        ui.add(
                                            egui::Label::new(
                                                Type::Caption
                                                    .rich(&*g.runtime_label)
                                                    .color(p.text_secondary),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    Column::Added => {
                                        ui.label(
                                            Type::Body
                                                .numeric(strings::fmt::relative_date(
                                                    g.added_at,
                                                    env.now,
                                                    env.utc_offset_s,
                                                ))
                                                .color(p.text_muted),
                                        );
                                    }
                                }
                            },
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let pill = play_pill(ui, &play_state(g), &g.name, PillSize::Row, 1.0);
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(id.with("play_id"), pill.id));
                        if pill.clicked() {
                            action = Some((g.id, GameAction::Play));
                        }
                    });
                },
            );
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
