//! Activity tab (SCREEN_SPECS §4.1, UI-021): tiles, range, chart with a table alternative,
//! paged session history ("Load more", 50 at a time), session context menu, reset.

use easyplay_core::model::GameSummary;
use easyplay_core::stats::StatsRequest;
use egui::{Popup, Sense, Ui};

use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::screens::Env;
use crate::screens::statistics::{bars, range_segments, tile_grid};
use crate::strings;
use crate::strings::details as s;
use crate::strings::stats as st;
use crate::theme::tokens::space;
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::bar_chart::{ChartOpts, bar_chart, bar_table};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::context_menu::{danger_item, item};
use crate::widgets::empty_state::empty_state;

/// History page size.
pub const PAGE: u32 = 50;

fn load_page(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let q = env.bridge.query_id();
    state.details.activity.history_q = Some(q);
    state.details.activity.history_failed = false;
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::SessionHistory {
            game: Some(g.id),
            page: state.details.activity.next_page,
            page_size: PAGE,
            include_uncounted: false,
            include_tools: env.bridge.prefs().general.show_tool_sessions_in_history,
        },
        reply_to: q,
    });
}

/// Show the Activity tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if g.session_count == 0 {
        ui.add_space(space::S8);
        empty_state(ui, icons::CHART, s::NO_SESSIONS, "", None);
        return;
    }
    if state.details.activity.history.is_empty()
        && state.details.activity.history_q.is_none()
        && state.details.activity.next_page == 0
        && !state.details.activity.history_failed
    {
        load_page(state, env, g);
    }
    let mut range = state.details.activity.range;
    if range_segments(ui, &mut range) {
        state.details.activity.range = range;
        super::overview::request(state, env, g.id);
    }
    ui.add_space(space::S3);
    match &state.details.activity.overview {
        AsyncValue::Ready(o) => {
            let o = o.clone();
            let stats = o.stats.clone();
            let tiles = vec![
                (
                    st::TOTAL,
                    strings::fmt::duration(o.totals.ms),
                    strings::fmt::duration_spoken(o.totals.ms),
                ),
                (
                    st::SESSIONS,
                    o.totals.sessions.to_string(),
                    o.totals.sessions.to_string(),
                ),
                (
                    st::AVERAGE,
                    o.average_ms
                        .map_or_else(|| "—".to_owned(), strings::fmt::duration),
                    o.average_ms
                        .map_or_else(String::new, strings::fmt::duration_spoken),
                ),
                (
                    st::LONGEST,
                    stats
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |x| strings::fmt::duration(x.longest_ms)),
                    stats
                        .as_ref()
                        .map_or_else(String::new, |x| strings::fmt::duration_spoken(x.longest_ms)),
                ),
                (
                    st::FIRST_PLAYED,
                    stats.as_ref().and_then(|x| x.first_played_at).map_or_else(
                        || "—".to_owned(),
                        |t| strings::fmt::relative_date(t, env.now, env.utc_offset_s),
                    ),
                    String::new(),
                ),
                (
                    st::LAST_PLAYED,
                    stats.as_ref().and_then(|x| x.last_played_at).map_or_else(
                        || "—".to_owned(),
                        |t| strings::fmt::relative_date(t, env.now, env.utc_offset_s),
                    ),
                    String::new(),
                ),
            ];
            tile_grid(ui, &tiles, 3);
            ui.add_space(space::S4);
            let b = bars(&o.series, env.utc_offset_s);
            ui.horizontal(|ui| {
                ui.label(Type::Heading.rich(st::PER_DAY).color(p.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if state.details.activity.table {
                        st::SHOW_CHART
                    } else {
                        st::SHOW_TABLE
                    };
                    if ui.add(Button::ghost(label).size(Size::Sm)).clicked() {
                        state.details.activity.table = !state.details.activity.table;
                    }
                });
            });
            if state.details.activity.table {
                bar_table(ui, st::PER_DAY, &b);
            } else {
                bar_chart(
                    ui,
                    st::PER_DAY,
                    &b,
                    ChartOpts {
                        height: 160.0,
                        axes: true,
                    },
                    &mut state.details.activity.bar,
                );
            }
        }
        v if v.show_skeleton(env.mono) => super::skeleton_lines(ui, 6),
        AsyncValue::Failed(_) => {
            ui.horizontal(|ui| {
                ui.label(Type::Body.rich(s::COULDNT_LOAD).color(p.text_secondary));
                if ui.add(Button::new(s::RETRY).size(Size::Sm)).clicked() {
                    super::overview::request(state, env, g.id);
                }
            });
        }
        _ => {}
    }
    // History.
    ui.add_space(space::S5);
    ui.label(Type::Heading.rich(s::HISTORY).color(p.text_primary));
    let rows = state.details.activity.history.clone();
    let mut remove = None;
    let mut view_log = None;
    egui::Grid::new("activity.history")
        .num_columns(4)
        .striped(true)
        .spacing(egui::vec2(space::S6, space::S2))
        .show(ui, |ui| {
            for h in [st::COL_DATE, st::COL_START, st::COL_DURATION, st::COL_END] {
                ui.label(Type::Label.rich(h).color(p.text_muted));
            }
            ui.end_row();
            for r in &rows {
                let date = ui.add(
                    egui::Label::new(
                        Type::Body
                            .numeric(strings::fmt::relative_date(
                                r.started_at,
                                env.now,
                                env.utc_offset_s,
                            ))
                            .color(p.text_primary),
                    )
                    .sense(Sense::click()),
                );
                ui.label(
                    Type::Body
                        .numeric(st::clock_time(r.started_at, env.utc_offset_s))
                        .color(p.text_secondary),
                );
                ui.label(
                    Type::Body
                        .numeric(strings::fmt::duration(r.duration_ms))
                        .color(p.text_primary),
                );
                ui.label(
                    Type::Body
                        .rich(
                            r.end_reason
                                .map_or(strings::launch::RUNNING, strings::launch::ended),
                        )
                        .color(p.text_secondary),
                );
                ui.end_row();
                Popup::context_menu(&date).show(|ui| {
                    if item(
                        ui,
                        Some(icons::TERMINAL),
                        strings::launch::VIEW_LOG,
                        None,
                        false,
                    )
                    .clicked()
                    {
                        view_log = Some(r.id);
                    }
                    if danger_item(ui, Some(icons::TRASH), s::REMOVE_SESSION).clicked() {
                        remove = Some(r.id);
                    }
                });
            }
        });
    if let Some(id) = remove {
        state.details.activity.history.retain(|r| r.id != id);
        env.bridge.send(UiCommand::RemoveSession { id });
    }
    if let Some(sid) = view_log {
        env.bridge.send(UiCommand::ViewLog {
            session: Some(sid),
            game: Some(g.id),
        });
    }
    if state.details.activity.has_more
        && ui
            .add(
                Button::new(s::LOAD_MORE)
                    .size(Size::Sm)
                    .busy(state.details.activity.history_q.is_some()),
            )
            .clicked()
    {
        load_page(state, env, g);
    }
    ui.add_space(space::S5);
    if ui
        .add(Button::new(s::RESET_STATS).variant(Variant::Link))
        .clicked()
    {
        state.show_dialog(Dialog::Confirm(Confirm::ResetGameStatistics(g.id)));
    }
}
