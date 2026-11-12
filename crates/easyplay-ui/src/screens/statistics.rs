//! Statistics (SCREEN_SPECS_2 §12, UI-022): range control, four tiles, playtime chart with a
//! table alternative, most played, recent sessions with the full history sheet, the busiest
//! weekday on wide layouts and "About these numbers". Results are cached per range and
//! library revision, so returning to the screen shows them in the first frame.

use easyplay_core::model::GameKey;
use easyplay_core::stats::{Bucket, Series, StatsRequest};
use egui::{Context, CornerRadius, Id, Popup, Sense, Ui, vec2};

use super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::strings::stats as s;
use crate::theme::tokens::{layout, radius, space};
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::details::range_for;
use crate::widgets::bar_chart::{Bar, ChartOpts, bar_chart, bar_table};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::empty_state::empty_state;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::sheet::sheet;
use crate::widgets::stat_tile::stat_tile;

/// Bars for a series.
pub fn bars(series: &Series, _offset_s: i32) -> Vec<Bar> {
    series
        .points
        .iter()
        .map(|(key, ms)| {
            let label = s::bucket_label(*key as u32, series.bucket);
            Bar {
                spoken: s::bucket_spoken(*key as u32, series.bucket, *ms),
                value_text: strings::fmt::duration(*ms),
                value: *ms as f64,
                label,
            }
        })
        .collect()
}

/// The range control (7 days, 30 days, year, all time). Returns true when it changed.
pub fn range_segments(ui: &mut Ui, range: &mut usize) -> bool {
    let opts: Vec<Segment<'_>> = s::RANGES
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect();
    segmented(ui, s::RANGE, range, &opts, false).changed()
}

/// Tiles in rows of `cols` (2 per row below 720 px). `(label, value, spoken)`.
pub fn tile_grid(ui: &mut Ui, tiles: &[(&str, String, String)], cols: usize) {
    let cols = if ui.available_width() < layout::COMPACT_BELOW {
        2
    } else {
        cols
    };
    for chunk in tiles.chunks(cols) {
        ui.columns(cols, |c| {
            for (i, (label, value, spoken)) in chunk.iter().enumerate() {
                let spoken = if spoken.is_empty() { value } else { spoken };
                stat_tile(&mut c[i], label, value, spoken, None);
            }
        });
        ui.add_space(space::S3);
    }
}

fn request(state: &mut UiState, env: &mut Env<'_>) {
    let rev = env.bridge.library().revision;
    let range = state.stats.range;
    if state.stats.cache.contains_key(&(range, rev))
        || state
            .stats
            .pending
            .is_some_and(|(_, r, v, _)| r == range && v == rev)
    {
        return;
    }
    let Some(dr) = range_for(range, env.now, env.utc_offset_s) else {
        return;
    };
    let q = env.bridge.query_id();
    state.stats.pending = Some((q, range, rev, env.mono));
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::LibraryOverview { range: dr },
        reply_to: q,
    });
    if state.stats.recent_q.is_none_or(|(_, v)| v != rev) {
        let q = env.bridge.query_id();
        state.stats.recent_q = Some((q, rev));
        if !matches!(state.stats.recent, AsyncValue::Ready(_)) {
            state.stats.recent = AsyncValue::Loading { since: env.mono };
        }
        env.bridge.send(UiCommand::QueryStats {
            request: StatsRequest::SessionHistory {
                game: None,
                page: 0,
                page_size: 10,
                include_uncounted: false,
                include_tools: false,
            },
            reply_to: q,
        });
    }
    let q = env.bridge.query_id();
    state.stats.weekdays_q = Some(q);
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::WeekdayDistribution { range: dr },
        reply_to: q,
    });
}

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let lib = env.bridge.library().clone();
    request(state, env);
    let m = super::library::margin(ui);
    egui::ScrollArea::vertical()
        .id_salt("statistics")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(m as i8, space::S4 as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let h = ui.label(
                            Type::Title
                                .rich(strings::nav::STATISTICS)
                                .color(p.text_primary),
                        );
                        crate::a11y::labelled(
                            ui.ctx(),
                            h.id,
                            egui::Role::Heading,
                            strings::nav::STATISTICS,
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut r = state.stats.range;
                            if range_segments(ui, &mut r) {
                                state.stats.range = r;
                                state.stats.bar = None;
                            }
                        });
                    });
                    ui.add_space(space::S4);
                    if lib.totals.total_playtime_ms == 0 {
                        ui.add_space(space::S12);
                        empty_state(ui, icons::CHART, s::EMPTY_TITLE, s::EMPTY_BODY, None);
                        return;
                    }
                    let key = (state.stats.range, lib.revision);
                    let Some(o) = state.stats.cache.get(&key).cloned().or_else(|| {
                        // While a new revision loads, keep showing the latest result for the range.
                        state
                            .stats
                            .cache
                            .iter()
                            .filter(|((r, _), _)| *r == state.stats.range)
                            .max_by_key(|((_, v), _)| *v)
                            .map(|(_, o)| o.clone())
                    }) else {
                        if let Some(e) = &state.stats.failed {
                            let _ = e;
                            ui.add_space(space::S8);
                            if empty_state(
                                ui,
                                icons::WARNING,
                                s::FAILED,
                                "",
                                Some(Button::new(s::RETRY)),
                            )
                            .is_some_and(|r| r.clicked())
                            {
                                state.stats.failed = None;
                                state.stats.pending = None;
                            }
                        } else if state.stats.pending.is_some_and(|(_, _, _, since)| {
                            env.mono.saturating_duration_since(since).as_secs_f32()
                                >= theme::tokens::timing::SKELETON_DELAY
                        }) {
                            skeleton(ui);
                        } else if state.stats.pending.is_some() {
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(150));
                        }
                        return;
                    };
                    let streak = s::days(o.current_streak);
                    let tiles = vec![
                        (
                            s::TOTAL,
                            strings::fmt::duration(o.totals.ms),
                            strings::fmt::duration_spoken(o.totals.ms),
                        ),
                        (
                            s::GAMES_PLAYED,
                            o.totals.games_played.to_string(),
                            String::new(),
                        ),
                        (s::SESSIONS, o.totals.sessions.to_string(), String::new()),
                        (s::STREAK, streak.clone(), streak),
                    ];
                    tile_grid(ui, &tiles, 4);
                    if o.totals.ms == 0 {
                        ui.label(Type::Body.rich(s::NO_PLAY_IN_RANGE).color(p.text_secondary));
                    }
                    ui.add_space(space::S3);
                    let b = bars(&o.series, env.utc_offset_s);
                    ui.horizontal(|ui| {
                        ui.label(
                            Type::Heading
                                .rich(s::per_bucket(o.series.bucket))
                                .color(p.text_primary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if state.stats.table {
                                s::SHOW_CHART
                            } else {
                                s::SHOW_TABLE
                            };
                            if ui.add(Button::ghost(label).size(Size::Sm)).clicked() {
                                state.stats.table = !state.stats.table;
                            }
                        });
                    });
                    if state.stats.table {
                        bar_table(ui, &s::per_bucket(o.series.bucket), &b);
                    } else {
                        bar_chart(
                            ui,
                            &s::per_bucket(o.series.bucket),
                            &b,
                            ChartOpts {
                                height: 180.0,
                                axes: true,
                            },
                            &mut state.stats.bar,
                        );
                    }
                    ui.add_space(space::S6);
                    let wide = ui.available_width() >= 900.0;
                    if wide {
                        ui.columns(2, |c| {
                            most_played(&mut c[0], state, env, &o.top);
                            recent(&mut c[1], state, env);
                        });
                        if let Some(w) = state.stats.weekdays {
                            ui.add_space(space::S5);
                            ui.label(Type::Heading.rich(s::WEEKDAYS).color(p.text_primary));
                            let wb: Vec<Bar> = w
                                .iter()
                                .enumerate()
                                .map(|(i, ms)| Bar {
                                    label: s::WEEKDAY_SHORT[i].to_owned(),
                                    spoken: s::weekday_spoken(i, *ms),
                                    value_text: strings::fmt::duration(*ms),
                                    value: *ms as f64,
                                })
                                .collect();
                            let mut f = None;
                            ui.scope(|ui| {
                                ui.set_max_width(360.0);
                                bar_chart(
                                    ui,
                                    s::WEEKDAYS,
                                    &wb,
                                    ChartOpts {
                                        height: 72.0,
                                        axes: true,
                                    },
                                    &mut f,
                                );
                            });
                        }
                    } else {
                        most_played(ui, state, env, &o.top);
                        ui.add_space(space::S5);
                        recent(ui, state, env);
                    }
                    ui.add_space(space::S5);
                    let about = ui.add(Button::ghost(s::ABOUT).icon(icons::INFO).size(Size::Sm));
                    Popup::menu(&about).width(360.0).show(|ui| {
                        ui.label(Type::Body.rich(s::ABOUT_BODY).color(p.text_secondary));
                    });
                });
        });
}

fn skeleton(ui: &mut Ui) {
    ui.columns(4, |c| {
        for col in c.iter_mut() {
            let (r, _) = col.allocate_exact_size(vec2(col.available_width(), 84.0), Sense::hover());
            crate::widgets::skeleton::block(col, r, Some(CornerRadius::same(radius::MD)));
        }
    });
    ui.add_space(space::S4);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 180.0), Sense::hover());
    crate::widgets::skeleton::block(ui, r, Some(CornerRadius::same(radius::MD)));
}

fn most_played(
    ui: &mut Ui,
    state: &mut UiState,
    env: &Env<'_>,
    top: &[easyplay_core::stats::Ranking],
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let lib = env.bridge.library().clone();
    ui.label(Type::Heading.rich(s::MOST_PLAYED).color(p.text_primary));
    ui.add_space(space::S2);
    let max = top.first().map_or(1, |r| r.ms.max(1));
    let n = if state.stats.all_games {
        top.len()
    } else {
        top.len().min(5)
    };
    for (i, r) in top.iter().take(n).enumerate() {
        let (name, id) = match &r.key {
            GameKey::Game(g) => (
                lib.get(*g).map_or_else(String::new, |x| x.name.to_string()),
                Some(*g),
            ),
            GameKey::Removed(n) => (n.clone(), None),
        };
        let row_name = s::ranked(i + 1, &name, &strings::fmt::duration_spoken(r.ms));
        let (rect, resp) = ui.allocate_exact_size(
            vec2(ui.available_width(), layout::ROW_H_COMPACT),
            Sense::click(),
        );
        crate::a11y::button(&resp, &row_name);
        if resp.hovered() || resp.has_focus() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::SM), p.bg_surface);
        }
        if resp.has_focus() {
            crate::widgets::focus_ring(
                ui.painter(),
                rect,
                CornerRadius::same(radius::SM),
                &theme::theme(ui.ctx()),
            );
        }
        let painter = ui.painter();
        painter.text(
            rect.left_center() + vec2(space::S2, 0.0),
            egui::Align2::LEFT_CENTER,
            (i + 1).to_string(),
            Type::Body.numeric_font_id(),
            p.text_muted,
        );
        // Long names end in an ellipsis; the full name is the tooltip and the accessible name.
        let name_g = egui::WidgetText::from(Type::BodyStrong.rich(&name)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            rect.width() * 0.45 - space::S2,
            Type::BodyStrong.font_id(),
        );
        let elided = name_g.elided;
        let painter = ui.painter();
        painter.galley(
            rect.left_center() + vec2(space::S8, -name_g.size().y / 2.0),
            name_g,
            p.text_primary,
        );
        let t = strings::fmt::duration(r.ms);
        let bar_x = rect.left() + space::S8 + rect.width() * 0.45 + space::S3;
        painter.text(
            egui::pos2(bar_x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &t,
            Type::Body.numeric_font_id(),
            p.text_secondary,
        );
        let track = egui::Rect::from_min_max(
            egui::pos2(bar_x + 96.0, rect.center().y - 3.0),
            egui::pos2(rect.right() - space::S2, rect.center().y + 3.0),
        );
        if track.width() > 8.0 {
            let w = track.width() * (r.ms as f32 / max as f32);
            painter.rect_filled(
                egui::Rect::from_min_size(track.min, vec2(w.max(2.0), track.height())),
                CornerRadius::same(radius::PILL),
                p.accent,
            );
        }
        let resp = if elided {
            resp.on_hover_text(&name)
        } else {
            resp
        };
        if (resp.clicked() || resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            && let Some(g) = id
        {
            state.nav.push(Route::Game(g));
        }
    }
    if top.len() > 5
        && ui
            .add(
                Button::new(if state.stats.all_games {
                    s::SHOW_FEWER
                } else {
                    s::SHOW_ALL
                })
                .variant(Variant::Link),
            )
            .clicked()
    {
        state.stats.all_games = !state.stats.all_games;
    }
}

fn recent(ui: &mut Ui, state: &mut UiState, env: &Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.label(Type::Heading.rich(s::RECENT).color(p.text_primary));
    ui.add_space(space::S2);
    match &state.stats.recent {
        AsyncValue::Ready(rows) => {
            egui::Grid::new("stats.recent")
                .num_columns(3)
                .spacing(vec2(space::S4, space::S2))
                .show(ui, |ui| {
                    for r in rows.iter().take(10) {
                        ui.label(
                            Type::Body
                                .numeric(strings::fmt::relative_date(
                                    r.started_at,
                                    env.now,
                                    env.utc_offset_s,
                                ))
                                .color(p.text_secondary),
                        );
                        ui.label(Type::BodyStrong.rich(&r.game_name).color(p.text_primary));
                        ui.label(
                            Type::Body
                                .numeric(strings::fmt::duration(r.duration_ms))
                                .color(p.text_secondary),
                        );
                        ui.end_row();
                    }
                });
            if ui
                .add(Button::new(s::SHOW_ALL).variant(Variant::Link))
                .clicked()
            {
                state.stats.history.clear();
                state.stats.history_page = 0;
                state.stats.history_more = true;
                state.nav.open(Overlay::History);
            }
        }
        AsyncValue::Failed(_) => {
            ui.label(Type::Body.rich(s::FAILED).color(p.text_secondary));
        }
        v if v.show_skeleton(env.mono) => super::details::skeleton_lines(ui, 5),
        _ => {}
    }
}

/// The full history sheet with a game filter.
pub fn history_sheet(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    if state.stats.history_q.is_none() && state.stats.history_more && state.stats.history.is_empty()
    {
        load_history(state, env);
    }
    let lib = env.bridge.library().clone();
    let r =
        sheet(
            ctx,
            id,
            s::HISTORY,
            false,
            |ui| {
                let p = theme::theme(ui.ctx()).palette.clone();
                let mut names: Vec<String> = vec![s::ALL_GAMES.to_owned()];
                names.extend(lib.games.iter().map(|g| g.name.to_string()));
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let mut sel = state
                    .stats
                    .history_game
                    .and_then(|g| lib.games.iter().position(|x| x.id == g))
                    .map_or(0, |i| i + 1);
                if select(
                    ui,
                    s::FILTER_GAME,
                    Some(s::FILTER_GAME),
                    None,
                    &mut sel,
                    &refs,
                    false,
                )
                .changed()
                {
                    state.stats.history_game = (sel > 0).then(|| lib.games[sel - 1].id);
                    state.stats.history.clear();
                    state.stats.history_page = 0;
                    state.stats.history_more = true;
                    load_history(state, env);
                }
                ui.add_space(space::S3);
                let rows = state.stats.history.clone();
                egui::ScrollArea::vertical().max_height(360.0).show_rows(
                    ui,
                    layout::ROW_H_COMPACT,
                    rows.len(),
                    |ui, range| {
                        for r in &rows[range] {
                            ui.horizontal(|ui| {
                                ui.set_height(layout::ROW_H_COMPACT);
                                ui.label(
                                    Type::Body
                                        .numeric(strings::fmt::relative_date(
                                            r.started_at,
                                            env.now,
                                            env.utc_offset_s,
                                        ))
                                        .color(p.text_secondary),
                                );
                                ui.label(Type::BodyStrong.rich(&r.game_name).color(p.text_primary));
                                ui.label(
                                    Type::Body
                                        .numeric(strings::fmt::duration(r.duration_ms))
                                        .color(p.text_secondary),
                                );
                                ui.label(
                                    Type::Body
                                        .rich(r.end_reason.map_or(
                                            strings::launch::RUNNING,
                                            strings::launch::ended,
                                        ))
                                        .color(p.text_muted),
                                );
                            });
                        }
                    },
                );
                if state.stats.history_more
                    && !rows.is_empty()
                    && ui
                        .add(Button::new(strings::details::LOAD_MORE).size(Size::Sm))
                        .clicked()
                {
                    load_history(state, env);
                }
            },
            |ui| ui.add(Button::primary(strings::feedback::CLOSE)).clicked(),
        );
    if r.close_requested || r.footer {
        crate::widgets::modal::closed(ctx, id);
        state.nav.close(Overlay::History);
    }
}

fn load_history(state: &mut UiState, env: &mut Env<'_>) {
    let q = env.bridge.query_id();
    state.stats.history_q = Some(q);
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::SessionHistory {
            game: state.stats.history_game,
            page: state.stats.history_page,
            page_size: 50,
            include_uncounted: false,
            include_tools: env.bridge.prefs().general.show_tool_sessions_in_history,
        },
        reply_to: q,
    });
}

/// The bucket of a series as text, for tests.
pub fn bucket_name(b: Bucket) -> &'static str {
    match b {
        Bucket::Day => "day",
        Bucket::Week => "week",
        Bucket::Month => "month",
    }
}
