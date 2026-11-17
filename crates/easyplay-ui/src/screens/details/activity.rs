//! Activity tab (easyplay-redesign.html `tabActivity`; SCREEN_SPECS §4.1, UI-021): the
//! period (7 days, 30 days, year, all time), a headline of the time played in it, one
//! sentence of figures, the bar chart (with "Show as table"), then the history as a table,
//! fifty sessions at a time (*Load more*); a session's row offers its log and its removal on
//! a secondary click. A game never played says so instead.

use easyplay_core::model::GameSummary;
use easyplay_core::stats::StatsRequest;
use egui::{Id, Popup, Sense, Ui};

use crate::app::UiState;
use crate::bridge::Command;
use crate::screens::Env;
use crate::screens::statistics::bars;
use crate::strings;
use crate::strings::details as s;
use crate::strings::stats as st;
use crate::theme::tokens::{game as gm, layout, space};
use crate::theme::{self, Type};
use crate::vm::AsyncValue;
use crate::vm::details::{RANGE_DAYS, range_for};
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::bar_chart::chart;
use crate::widgets::button::{Button, Variant};
use crate::widgets::context_menu::{danger_item, item};
use crate::widgets::rows::{self, Col, table_row, table_text};
use crate::widgets::segmented::{Segment, segmented};

/// History page size.
pub const PAGE: u32 = 50;

fn load_page(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let q = env.bridge.query_id();
    state.details.activity.history_q = Some(q);
    state.details.activity.history_failed = false;
    env.bridge.send(Command::QueryStats {
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

/// A game never played: a blank in the column, 200 high at least.
fn never(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width().min(layout::BLANK_W);
    let title = Type::Blank.galley(ui, s::NOT_PLAYED_YET(), p.t1, w, usize::MAX);
    let body = Type::Lead.galley(ui, s::NEVER_BODY(), p.t2, w, usize::MAX);
    let h = title.size().y + gm::LEDE_GAP + body.size().y;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), gm::BLANK_MIN_H.max(h)),
        Sense::hover(),
    );
    // Centred in a 440 column at the left, as the library's blank is in its page.
    let content_w = title.size().x.max(body.size().x);
    let left = rect.left() + ((w - content_w) / 2.0).max(0.0);
    let top = rect.center().y - h / 2.0;
    let th = title.size().y;
    ui.painter().galley(egui::pos2(left, top), title, p.t1);
    ui.painter()
        .galley(egui::pos2(left, top + th + gm::LEDE_GAP), body, p.t2);
    crate::a11y::item(
        &resp,
        egui::Role::Label,
        &format!("{}. {}", s::NOT_PLAYED_YET(), s::NEVER_BODY()),
    );
}

/// The longest session of the period (from its first day), when the history loaded so far
/// covers it.
fn longest_in(state: &UiState, env: &Env<'_>, range: usize) -> Option<u64> {
    use easyplay_core::stats::rollup::{date_key, local_day};
    let a = &state.details.activity;
    let from = range_for(range, env.now, env.utc_offset_s)?.from?;
    let first = date_key(from);
    let inside = |t| local_day(t, (env.utc_offset_s / 60) as i16) >= first;
    let covered = !a.has_more || a.history.last().is_some_and(|r| !inside(r.started_at));
    covered
        .then(|| {
            a.history
                .iter()
                .filter(|r| inside(r.started_at))
                .map(|r| r.duration_ms)
                .max()
        })
        .flatten()
}

/// Show the Activity tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if g.session_count == 0 {
        never(ui);
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
    let opts: Vec<Segment<'_>> = st::RANGES()
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect();
    if segmented(ui, s::PERIOD(), &mut range, &opts, false).changed() {
        state.details.activity.range = range.min(st::RANGES().len() - 1);
        super::overview::request(state, env, g.id);
    }
    ui.add_space(gm::RANGE_GAP);
    let range = state.details.activity.range;
    match &state.details.activity.overview {
        AsyncValue::Ready(o) => {
            let o = o.clone();
            let total = if RANGE_DAYS.get(range).copied().flatten().is_none() {
                g.total_playtime_ms
            } else {
                o.totals.ms
            };
            let headline = s::activity_headline(&strings::fmt::play_time(total), range);
            let h = Type::Lede.paragraph_with(ui, &headline, p.t1, true);
            crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, &headline);
            ui.add_space(gm::LEDE_GAP);
            let n = o.totals.sessions;
            let average = (n > 0).then(|| strings::fmt::play_time(o.totals.ms / u64::from(n)));
            let longest = if RANGE_DAYS.get(range).copied().flatten().is_none() {
                o.stats.as_ref().map(|x| x.longest_ms)
            } else {
                longest_in(state, env, range)
            }
            .filter(|_| n > 0)
            .map(strings::fmt::play_time);
            let first = o
                .stats
                .as_ref()
                .and_then(|x| x.first_played_at)
                .or(g.first_played_at)
                .map(|t| s::date_label(t, env.now, env.utc_offset_s));
            let sentence =
                s::activity_sentence(n, average.as_deref(), longest.as_deref(), first.as_deref());
            Type::Lead.paragraph(ui, &sentence, p.t2);
            let b = bars(&o.series, env.utc_offset_s);
            let says = s::RANGE_SAYS()
                .get(range)
                .copied()
                .unwrap_or(s::RANGE_SAYS()[3]);
            let a = &mut state.details.activity;
            chart(ui, &st::chart_name(says), &b, &mut a.table, &mut a.bar);
        }
        v if v.show_skeleton(env.mono) => super::skeleton_lines(ui, 6),
        AsyncValue::Failed(_) => {
            ui.horizontal(|ui| {
                ui.label(Type::Body.rich(s::COULDNT_LOAD()).color(p.t2));
                if ui.add(Button::ghost(s::RETRY())).clicked() {
                    super::overview::request(state, env, g.id);
                }
            });
        }
        _ => {}
    }

    // History.
    rows::heading(ui, s::HISTORY(), false);
    let [c1, c2, c3] = gm::HISTORY_COLS;
    let cols = [
        Col::Flex(1.0),
        Col::Fixed(c1),
        Col::Fixed(c2),
        Col::Fixed(c3),
    ];
    let heads = [s::COL_DATE, s::COL_STARTED, s::COL_DURATION, s::COL_ENDED];
    table_row(ui, &cols, true, |ui, i| {
        table_text(ui, heads[i](), i, true, false);
    });
    let rows = state.details.activity.history.clone();
    let mut remove = None;
    let mut view_log = None;
    for r in &rows {
        let cells = [
            s::day_label(r.started_at, env.now, env.utc_offset_s),
            st::clock_time(r.started_at, env.utc_offset_s),
            strings::fmt::play_time(r.duration_ms),
            r.end_reason
                .map_or(strings::launch::RUNNING(), |r| strings::launch::ended(r))
                .to_owned(),
        ];
        let row = table_row(ui, &cols, false, |ui, i| {
            table_text(ui, &cells[i], i, false, i == 1 || i == 2);
        });
        let menu = ui.interact(row.rect, Id::unique(("history", r.id)), Sense::click());
        crate::a11y::labelled(ui.ctx(), menu.id, egui::Role::Row, &cells.join(", "));
        Popup::context_menu(&menu).show(|ui| {
            if item(ui, None, strings::launch::VIEW_LOG(), None, false).clicked() {
                view_log = Some(r.id);
            }
            if danger_item(ui, None, s::REMOVE_SESSION()).clicked() {
                remove = Some(r.id);
            }
        });
    }
    if let Some(id) = remove {
        state.details.activity.history.retain(|r| r.id != id);
        env.bridge.send(Command::RemoveSession { id });
    }
    if let Some(sid) = view_log {
        env.bridge.send(Command::ViewLog {
            session: Some(sid),
            game: Some(g.id),
        });
    }
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        if state.details.activity.has_more
            && ui
                .add(Button::ghost(s::LOAD_MORE()).busy(state.details.activity.history_q.is_some()))
                .clicked()
        {
            load_page(state, env, g);
        }
        if ui
            .add(Button::new(s::RESET_STATS()).variant(Variant::Link))
            .clicked()
        {
            state.show_dialog(Dialog::Confirm(Confirm::ResetGameStatistics(g.id)));
        }
    });
}
