//! Statistics (SCREEN_SPECS_2 §12, UI-022): an 860 column at the left of a scrolling page.
//! A headline in the display face says the period's time ("9 h 48 min in the last 7 days"),
//! one sentence gives the games, sessions and streak, then the bar chart (days for 7 and 30
//! days, twelve months for Year and All time) and two lists side by side: Most played and
//! Recent sessions, each row opening its game. The period control lives in the top bar.
//! Results are cached per range and library revision, so returning to the screen shows them
//! in the first frame.
//!
//! Kept from the earlier page and placed under the lists: the full play history (a quiet
//! button under Recent sessions opens the sheet), the busiest days of the week, and what the
//! numbers count, as fine print.

use easyplay_core::model::{GameKey, GameSummary};
use easyplay_core::stats::rollup::{
    civil_from_days, day_key, days_from_civil, key_date, key_days, local_day,
};
use easyplay_core::stats::{DateRange, Ranking, Series, StatsRequest};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Context, CornerRadius, Id, Layout, Margin, Rect, Response, Sense, Ui, UiBuilder, pos2,
    vec2,
};

use super::Env;
use crate::app::UiState;
use crate::bridge::Command;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::strings::stats as s;
use crate::textures::{TexKey, TexState, ThumbSize};
use crate::theme::palette::fade;
use crate::theme::tokens::{control, layout, radius, space, travel};
use crate::theme::{self, Dur, Type, motion};
use crate::vm::AsyncValue;
use crate::widgets::bar_chart::{Bar, ChartOpts, bar_chart, chart};
use crate::widgets::button::Button;
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::empty_state::blank;
use crate::widgets::rows;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::sheet::sheet;
use crate::widgets::{focus_ring, hover_t};

const DAY_MS: i64 = 86_400_000;
/// The period whose twelve months also chart All time.
const YEAR: usize = 2;
/// All time.
const ALL: usize = 3;
/// Sessions asked for to fill "Recent sessions" (those outside the period are left out).
const RECENT_ASKED: u32 = 40;

/// Bars for a series.
pub fn bars(series: &Series, _offset_s: i32) -> Vec<Bar> {
    series
        .points
        .iter()
        .map(|(key, ms)| Bar {
            label: s::bucket_label(*key as u32, series.bucket),
            spoken: s::bucket_spoken(*key as u32, series.bucket, *ms),
            value_text: strings::fmt::play_time(*ms),
            value: *ms as f64,
        })
        .collect()
}

/// The range control (7 days, 30 days, Year, All time). Returns true when it changed.
pub fn range_segments(ui: &mut Ui, range: &mut usize) -> bool {
    let opts: Vec<Segment<'_>> = s::RANGES()
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect();
    segmented(ui, s::RANGE(), range, &opts, false).changed()
}

/// The local day number of today.
fn today(env: &Env<'_>) -> i64 {
    key_days(local_day(env.now, (env.utc_offset_s / 60) as i16))
}

/// The days a period covers: the last 7 or 30 days, the twelve months up to this one, or
/// everything (`None` for its start).
fn period(range: usize, env: &Env<'_>) -> Option<(Option<i64>, i64)> {
    let to = today(env);
    let from = match range {
        0 => Some(to - 6),
        1 => Some(to - 29),
        YEAR => {
            let (y, m, _) = civil_from_days(to);
            let back = i64::from(m) - 1 - 11;
            let (y, m) = (y + back.div_euclid(12), back.rem_euclid(12) as u32 + 1);
            Some(days_from_civil(y, m, 1))
        }
        _ => None,
    };
    Some((from, to))
}

fn date_range(range: usize, env: &Env<'_>) -> Option<DateRange> {
    let (from, to) = period(range, env)?;
    let to_date = key_date(day_key(to))?;
    Some(match from {
        Some(f) => DateRange {
            from: Some(key_date(day_key(f))?),
            to: to_date,
        },
        None => DateRange::all_time(to_date),
    })
}

/// Ask for what the page shows that is not cached yet, one overview at a time.
fn request(state: &mut UiState, env: &mut Env<'_>) {
    // After a failure the page offers *Try again*; asking by itself would repeat the failing
    // query on every frame.
    if state.stats.failed.is_some() {
        return;
    }
    let rev = env.bridge.library().revision;
    let range = state.stats.range;
    // All time takes its totals from all time and its chart from the twelve months.
    let wanted: &[usize] = if range == ALL { &[ALL, YEAR] } else { &[range] };
    if let Some(&r) = wanted
        .iter()
        .find(|r| !state.stats.cache.contains_key(&(**r, rev)))
        && !state
            .stats
            .pending
            .is_some_and(|(_, pr, pv, _)| pr == r && pv == rev)
        && let Some(dr) = date_range(r, env)
    {
        let q = env.bridge.query_id();
        state.stats.pending = Some((q, r, rev, env.mono));
        env.bridge.send(Command::QueryStats {
            request: StatsRequest::LibraryOverview { range: dr },
            reply_to: q,
        });
    }
    if state.stats.recent_q.is_none_or(|(_, v)| v != rev) {
        let q = env.bridge.query_id();
        state.stats.recent_q = Some((q, rev));
        if !matches!(state.stats.recent, AsyncValue::Ready(_)) {
            state.stats.recent = AsyncValue::Loading { since: env.mono };
        }
        env.bridge.send(Command::QueryStats {
            request: StatsRequest::SessionHistory {
                game: None,
                page: 0,
                page_size: RECENT_ASKED,
                include_uncounted: false,
                include_tools: false,
            },
            reply_to: q,
        });
    }
    // The longer "Most played" list, once it is asked for.
    if state.stats.all_games
        && !state.stats.top.contains_key(&(range, rev))
        && !state
            .stats
            .top_q
            .is_some_and(|(_, r, v)| r == range && v == rev)
        && let Some(dr) = date_range(range, env)
    {
        let q = env.bridge.query_id();
        state.stats.top_q = Some((q, range, rev));
        env.bridge.send(Command::QueryStats {
            request: StatsRequest::TopGames {
                range: dr,
                limit: layout::TOP_MAX as u32,
            },
            reply_to: q,
        });
    }
    if state.stats.weekdays_for != Some((range, rev))
        && state.stats.weekdays_q.is_none()
        && let Some(dr) = date_range(range, env)
    {
        state.stats.weekdays_for = Some((range, rev));
        state.stats.weekdays = None;
        let q = env.bridge.query_id();
        state.stats.weekdays_q = Some(q);
        env.bridge.send(Command::QueryStats {
            request: StatsRequest::WeekdayDistribution { range: dr },
            reply_to: q,
        });
    }
}

/// Statistics' side of the top bar: the period control (7 days / 30 days / Year / All
/// time). Called by the shell inside a right-to-left, centred layout.
pub fn top_bar(ui: &mut Ui, state: &mut UiState, _env: &mut Env<'_>) {
    let mut r = state.stats.range;
    if range_segments(ui, &mut r) {
        state.stats.range = r;
        state.stats.bar = None;
        state.stats.failed = None;
    }
}

/// Show the statistics route.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    request(state, env);
    let page_w = ui.available_width();
    let gutter = super::shell::gutter(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt("statistics")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(Margin {
                    left: gutter as i8,
                    right: gutter as i8,
                    top: layout::STATS_TOP as i8,
                    bottom: layout::PAGE_PAD[1] as i8,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // Nothing recorded at all, or the period's figures failed: an empty
                    // state in the middle of the page.
                    if env.bridge.library().totals.total_playtime_ms == 0 {
                        blank(ui, s::EMPTY_TITLE(), s::EMPTY_BODY(), |_| (), None);
                        return;
                    }
                    let failed = state.stats.failed.is_some()
                        && !state
                            .stats
                            .cache
                            .keys()
                            .any(|(r, _)| *r == state.stats.range);
                    if failed {
                        let retry = blank(
                            ui,
                            s::FAILED(),
                            "",
                            |ui| ui.add(Button::primary(s::RETRY())).clicked(),
                            None,
                        );
                        if retry {
                            state.stats.failed = None;
                            state.stats.pending = None;
                        }
                        return;
                    }
                    let w = ui.available_width().min(layout::STATS_MAX);
                    // The column stands in the middle of a wider page.
                    let indent = super::centre_indent(ui.available_width(), layout::STATS_MAX);
                    let at = ui.cursor().min + vec2(indent, 0.0);
                    let mut column = ui.new_child(
                        UiBuilder::new()
                            .max_rect(Rect::from_min_size(at, vec2(w, f32::INFINITY)))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    let filled = column.allocate_ui_with_layout(
                        vec2(w, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(w);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            // The page's content rises into place when it arrives.
                            let m = theme::theme(ui.ctx()).motion;
                            let (alpha, below) = motion::arrive(
                                ui.ctx(),
                                Id::unique("stats.arrive"),
                                m,
                                travel::PAGE_RISE_AT[0],
                                Dur::Page,
                                travel::PAGE_RISE,
                            );
                            ui.multiply_opacity(alpha);
                            ui.add_space(below);
                            page(ui, state, env, page_w);
                        },
                    );
                    let h = filled.response.rect.height();
                    ui.allocate_space(vec2(ui.available_width(), h));
                });
        });
}

fn page(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, page_w: f32) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let lib = env.bridge.library().clone();
    let range = state.stats.range;
    let key = (range, lib.revision);
    let latest = |r: usize, state: &UiState| {
        state
            .stats
            .cache
            .get(&(r, lib.revision))
            .cloned()
            .or_else(|| {
                // While a new revision loads, keep showing the latest result for the range.
                state
                    .stats
                    .cache
                    .iter()
                    .filter(|((x, _), _)| *x == r)
                    .max_by_key(|((_, v), _)| *v)
                    .map(|(_, o)| o.clone())
            })
    };
    let Some(o) = latest(range, state) else {
        return;
    };
    // The headline: the display face, tabular figures, balanced over the lines it takes.
    let hero = if page_w < layout::NARROW_BELOW {
        Type::Lede
    } else {
        Type::Hero
    };
    let w = ui.available_width();
    let headline = s::headline(o.totals.ms, range);
    let g = balanced(ui, &headline, hero, true, p.t1, w);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, &headline);
    ui.add_space(layout::LEDE_GAP);
    let sentence = if o.totals.sessions == 0 {
        s::NO_PLAY_IN_RANGE().to_owned()
    } else {
        s::summary(o.totals.games_played, o.totals.sessions, o.current_streak)
    };
    paragraph(ui, &sentence, Type::Lead, p.t2);
    // The chart: All time plots the twelve months of the year's result.
    let series = if range == ALL {
        latest(YEAR, state).map_or_else(|| o.series.clone(), |y| y.series)
    } else {
        o.series.clone()
    };
    let b = bars(&series, env.utc_offset_s);
    chart(
        ui,
        &s::chart_name(s::RANGE_SAYS()[range.min(3)]),
        &b,
        &mut state.stats.table,
        &mut state.stats.bar,
    );
    if o.totals.sessions == 0 {
        steam_line(ui, &lib, layout::HEADING_GAP[0]);
        return;
    }
    ui.add_space(layout::STATS_LISTS_TOP);
    let top: Vec<Ranking> = if state.stats.all_games {
        state
            .stats
            .top
            .get(&key)
            .cloned()
            .unwrap_or_else(|| o.top.clone())
    } else {
        o.top.clone()
    };
    let games_played = o.totals.games_played as usize;
    let since = period(range, env).and_then(|(from, _)| from).map(|d| {
        easyplay_core::clock::Timestamp::from_millis(
            d * DAY_MS - i64::from(env.utc_offset_s) * 1000,
        )
    });
    let two = page_w >= layout::STACK_BELOW;
    let recent = state.stats.recent.clone();
    let mut all_games = state.stats.all_games;
    let (opened, history) = if two {
        let gap = layout::TWO_COL_GAP[1];
        let col_w = (ui.available_width() - gap) / 2.0;
        let top_left = ui.cursor().min;
        let column = |x: f32| {
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    pos2(x, top_left.y),
                    vec2(col_w, f32::INFINITY),
                ))
                .layout(Layout::top_down(Align::Min))
        };
        let mut left = ui.new_child(column(top_left.x));
        left.spacing_mut().item_spacing.y = 0.0;
        left.set_width(col_w);
        let a = most_played(&mut left, env, &lib, &top, games_played, &mut all_games);
        let mut right = ui.new_child(column(top_left.x + col_w + gap));
        right.spacing_mut().item_spacing.y = 0.0;
        right.set_width(col_w);
        let (b, history) = recent_sessions(&mut right, env, &lib, &recent, since);
        let bottom = left.min_rect().bottom().max(right.min_rect().bottom());
        ui.allocate_rect(
            Rect::from_min_max(top_left, pos2(top_left.x + ui.available_width(), bottom)),
            Sense::hover(),
        );
        (a.or(b), history)
    } else {
        let a = most_played(ui, env, &lib, &top, games_played, &mut all_games);
        ui.add_space(layout::TWO_COL_GAP[0]);
        let (b, history) = recent_sessions(ui, env, &lib, &recent, since);
        (a.or(b), history)
    };
    state.stats.all_games = all_games;
    // Kept from the earlier page: the busiest days of the week, and what the numbers count.
    let weekdays_current =
        state.stats.weekdays_q.is_none() && state.stats.weekdays_for == Some((range, lib.revision));
    if let Some(wd) = state.stats.weekdays.filter(|_| weekdays_current)
        && wd.iter().any(|ms| *ms > 0)
    {
        let col_w = if two {
            (ui.available_width() - layout::TWO_COL_GAP[1]) / 2.0
        } else {
            ui.available_width()
        };
        ui.allocate_ui_with_layout(vec2(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(col_w);
            rows::heading(ui, s::WEEKDAYS(), false);
            let wb: Vec<Bar> = wd
                .iter()
                .enumerate()
                .map(|(i, ms)| Bar {
                    label: s::WEEKDAY_SHORT()[i].to_owned(),
                    spoken: s::weekday_spoken(i, *ms),
                    value_text: strings::fmt::play_time(*ms),
                    value: *ms as f64,
                })
                .collect();
            let mut f = None;
            bar_chart(
                ui,
                s::WEEKDAYS(),
                &wb,
                ChartOpts {
                    height: layout::WEEKDAY_CHART_H,
                    axes: true,
                },
                &mut f,
            );
        });
    }
    ui.add_space(layout::HEADING_GAP[0]);
    paragraph(ui, s::ABOUT_BODY(), Type::Caption, p.t3);
    steam_line(ui, &lib, space::S2);
    if let Some(g) = opened {
        state.nav.push(Route::Game(g));
    }
    if history {
        state.stats.history.clear();
        state.stats.history_page = 0;
        state.stats.history_more = true;
        state.nav.open(Overlay::History);
    }
}

/// "Most played": the first five games of the period, up to twenty after "Show all".
/// Returns the game whose row was pressed.
fn most_played(
    ui: &mut Ui,
    env: &mut Env<'_>,
    lib: &crate::bridge::LibraryView,
    top: &[Ranking],
    games_played: usize,
    all_games: &mut bool,
) -> Option<easyplay_core::ids::GameId> {
    let mut opened = None;
    rows::heading(ui, s::MOST_PLAYED(), true);
    let n = if *all_games {
        layout::TOP_MAX
    } else {
        layout::TOP_SHOWN
    };
    for (i, r) in top.iter().take(n).enumerate() {
        let (game, name) = match &r.key {
            GameKey::Game(g) => (
                lib.get(*g),
                lib.get(*g).map_or_else(String::new, |x| x.name.to_string()),
            ),
            GameKey::Removed(n) => (None, n.clone()),
        };
        if list_row(ui, env, Lead::Rank(i + 1), game, &name, r.ms).clicked()
            && let Some(g) = game
        {
            opened = Some(g.id);
        }
    }
    if games_played > layout::TOP_SHOWN {
        ui.add_space(layout::LIST_ACTIONS_GAP);
        let label = if *all_games {
            s::SHOW_FEWER().to_owned()
        } else {
            s::show_all(games_played.min(layout::TOP_MAX))
        };
        if quiet_button(ui, &label).clicked() {
            *all_games = !*all_games;
        }
    }
    opened
}

/// "Recent sessions": the period's last eight sessions, newest first, then the button that
/// opens the full play history. Returns the game whose row was pressed, and whether the
/// history was asked for.
fn recent_sessions(
    ui: &mut Ui,
    env: &mut Env<'_>,
    lib: &crate::bridge::LibraryView,
    recent: &AsyncValue<Vec<easyplay_core::stats::HistoryRow>, easyplay_core::error::UserError>,
    since: Option<easyplay_core::clock::Timestamp>,
) -> (Option<easyplay_core::ids::GameId>, bool) {
    let mut opened = None;
    rows::heading(ui, s::RECENT(), true);
    if let AsyncValue::Ready(rows) = recent {
        for r in rows
            .iter()
            .filter(|r| since.is_none_or(|t| r.started_at >= t))
            .take(layout::RECENT_SHOWN)
        {
            let game = r.game_id.and_then(|g| lib.get(g));
            let day = s::day(r.started_at, env.now, env.utc_offset_s);
            if list_row(ui, env, Lead::Day(&day), game, &r.game_name, r.duration_ms).clicked()
                && let Some(g) = game
            {
                opened = Some(g.id);
            }
        }
    }
    ui.add_space(layout::LIST_ACTIONS_GAP);
    let history = quiet_button(ui, s::SHOW_HISTORY()).clicked();
    (opened, history)
}

/// `text` wrapped at `width` in `style`, with tabular figures when `numeric`.
fn wrapped(
    ui: &Ui,
    text: &str,
    style: Type,
    numeric: bool,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(
        theme::typography::shaped(text).into_owned(),
        style.format(color, numeric),
    );
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        ..TextWrapping::default()
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A headline wrapped at `width` with its lines balanced (CSS `text-wrap: balance`): the
/// narrowest width that keeps the same number of lines.
pub(super) fn balanced(
    ui: &Ui,
    text: &str,
    style: Type,
    numeric: bool,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = wrapped(ui, text, style, numeric, color, width);
    let rows = full.rows.len();
    if rows < 2 {
        return full;
    }
    let (mut lo, mut hi) = (width / rows as f32, width);
    for _ in 0..8 {
        let mid = (lo + hi) / 2.0;
        if wrapped(ui, text, style, numeric, color, mid).rows.len() > rows {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    wrapped(ui, text, style, numeric, color, hi.ceil())
}

/// One quiet line about Steam games (ADR-0031): they are not in the charts, and Steam's own
/// total for them, labelled as Steam's, `gap` below what precedes it. Only while the
/// library has Steam games.
fn steam_line(ui: &mut Ui, lib: &crate::bridge::LibraryView, gap: f32) {
    if !lib.games.iter().any(|g| g.is_steam()) {
        return;
    }
    ui.add_space(gap);
    let p = theme::theme(ui.ctx()).palette.clone();
    let total = strings::fmt::play_time(lib.totals.steam_playtime_ms);
    paragraph(
        ui,
        &strings::steam::statistics_line(&total),
        Type::Caption,
        p.t3,
    );
}

/// A paragraph the width of the column.
fn paragraph(ui: &mut Ui, text: &str, style: Type, color: egui::Color32) {
    let w = ui.available_width();
    let g = wrapped(ui, text, style, false, color, w);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, color);
    crate::a11y::item(&resp, egui::Role::Label, text);
}

/// A quiet text button whose text stands on the column's axis (the button's padding reaches
/// past it to the left).
fn quiet_button(ui: &mut Ui, label: &str) -> Response {
    on_axis(ui, |ui| ui.add(Button::ghost(label)))
}

/// A line of quiet buttons, side by side, whose first text stands on the column's axis: the
/// line starts a button's padding to the left of the column without widening it.
pub(super) fn on_axis<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let top = ui.cursor().min;
    let pad = control::BUTTON_PAD;
    let mut line = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(top.x - pad, top.y),
                vec2(ui.available_width() + pad, control::BUTTON),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    line.spacing_mut().item_spacing.x = 0.0;
    let out = add(&mut line);
    let h = line.min_rect().height();
    ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    out
}

/// What leads a list row.
#[derive(Clone, Copy)]
enum Lead<'a> {
    /// Its rank in Most played.
    Rank(usize),
    /// The day of a session.
    Day(&'a str),
}

/// A row of the lists: 44 high, reaching 8 past the text on both sides, a faint fill under
/// the pointer. The lead (rank or day) in quiet text, a 24 wide cover, the title cut with an
/// ellipsis, the time at the right in tabular figures. A row of a game still in the library
/// opens it.
fn list_row(
    ui: &mut Ui,
    env: &mut Env<'_>,
    lead: Lead<'_>,
    game: Option<&GameSummary>,
    name: &str,
    ms: u64,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let w = ui.available_width();
    let sense = if game.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, layout::LIST_ROW_H), sense);
    let time = strings::fmt::play_time(ms);
    let spoken = strings::fmt::duration_spoken(ms);
    let (lead_text, lead_w, accessible) = match lead {
        Lead::Rank(n) => (n.to_string(), layout::RANK_W, s::ranked(n, name, &spoken)),
        Lead::Day(d) => (
            d.to_owned(),
            layout::SESSION_DAY_W,
            s::session_spoken(d, name, &spoken),
        ),
    };
    if game.is_some() {
        crate::a11y::button(&resp, &accessible);
    } else {
        crate::a11y::item(&resp, egui::Role::ListItem, &accessible);
    }
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let plate = rect.expand2(vec2(layout::ROW_OVERHANG, 0.0));
    let corner = CornerRadius::same(radius::ROW);
    if game.is_some() {
        let hover = hover_t(&resp, &theme);
        ui.painter().rect_filled(plate, corner, fade(p.o1, hover));
        if resp.has_focus() {
            focus_ring(ui.painter(), plate, corner, &theme);
        }
    }
    let cy = rect.center().y;
    let gap = layout::STATS_COL_GAP;
    let lead_g = match lead {
        Lead::Rank(_) => Type::Body.numeric_galley(ui, &lead_text, p.t3),
        Lead::Day(_) => Type::Body.galley(ui, &lead_text, p.t3, lead_w, 1),
    };
    ui.painter()
        .galley(pos2(rect.left(), cy - lead_g.size().y / 2.0), lead_g, p.t3);
    let thumb = Rect::from_min_size(
        pos2(
            rect.left() + lead_w + gap,
            cy - layout::LIST_THUMB * layout::COVER_ASPECT / 2.0,
        ),
        vec2(
            layout::LIST_THUMB,
            layout::LIST_THUMB * layout::COVER_ASPECT,
        ),
    );
    let source = game.map_or(CoverSource::Placeholder, |g| {
        thumb_source(ui.ctx(), env, g, thumb)
    });
    cover::paint(
        ui.painter(),
        thumb,
        source,
        name,
        CoverLook::default(),
        &theme,
    );
    let t = Type::Body.numeric_galley(ui, &time, p.t2);
    let time_x = rect.right() - t.size().x;
    ui.painter()
        .galley(pos2(time_x, cy - t.size().y / 2.0), t, p.t2);
    let title_x = thumb.right() + gap;
    let title_w = (time_x - gap - title_x).max(1.0);
    let title = Type::BodyStrong.galley(ui, name, p.t1, title_w, 1);
    let elided = title.elided;
    ui.painter()
        .galley(pos2(title_x, cy - title.size().y / 2.0), title, p.t1);
    if elided {
        crate::widgets::tooltip::tooltip_at_pointer(resp, name, None)
    } else {
        resp
    }
}

/// The cover of `game` in a list thumb: its artwork when loaded, otherwise its poster.
fn thumb_source(ctx: &Context, env: &mut Env<'_>, game: &GameSummary, rect: Rect) -> CoverSource {
    match game.cover {
        Some(art) => {
            let key = TexKey {
                art,
                size: ThumbSize::cover_for(rect.width() * ctx.pixels_per_point()),
            };
            match env.textures.get(key, true, env.art) {
                TexState::Ready(t) => CoverSource::Texture(t, 1.0),
                TexState::Pending(c) => c.map_or(CoverSource::Placeholder, CoverSource::Pending),
            }
        }
        None => CoverSource::Placeholder,
    }
}

/// The full history sheet with a game filter.
pub fn history_sheet(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    if state.stats.history_q.is_none() && state.stats.history_more && state.stats.history.is_empty()
    {
        load_history(state, env);
    }
    let lib = env.bridge.library().clone();
    let r = sheet(
        ctx,
        id,
        s::HISTORY(),
        false,
        |ui| {
            let mut names: Vec<String> = vec![s::ALL_GAMES().to_owned()];
            names.extend(lib.games.iter().map(|g| g.name.to_string()));
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let mut sel = state
                .stats
                .history_game
                .and_then(|g| lib.games.iter().position(|x| x.id == g))
                .map_or(0, |i| i + 1);
            if select(
                ui,
                s::FILTER_GAME(),
                Some(s::FILTER_GAME()),
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
            let cols = [
                rows::Col::Flex(1.0),
                rows::Col::Flex(1.4),
                rows::Col::Fixed(layout::LIST_COLS[2]),
                rows::Col::Flex(1.0),
            ];
            let heads = [s::COL_DATE, s::FILTER_GAME, s::COL_DURATION, s::COL_END];
            rows::table_row(ui, &cols, true, |ui, i| {
                rows::table_text(ui, heads[i](), i, true, false);
            });
            let rows_v = state.stats.history.clone();
            egui::ScrollArea::vertical()
                .max_height(layout::LOG_H[1])
                .show_rows(ui, layout::TABLE_ROW, rows_v.len(), |ui, range| {
                    for r in &rows_v[range] {
                        let day = s::day(r.started_at, env.now, env.utc_offset_s);
                        let when =
                            format!("{day} {}", s::clock_time(r.started_at, env.utc_offset_s));
                        let end = r
                            .end_reason
                            .map_or(strings::launch::RUNNING(), strings::launch::ended);
                        let cells = [
                            when,
                            r.game_name.clone(),
                            strings::fmt::play_time(r.duration_ms),
                            end.to_owned(),
                        ];
                        rows::table_row(ui, &cols, false, |ui, i| {
                            rows::table_text(ui, &cells[i], i, false, i == 2);
                        });
                    }
                });
            if state.stats.history_more
                && !rows_v.is_empty()
                && ui
                    .add(Button::ghost(strings::details::LOAD_MORE()))
                    .clicked()
            {
                load_history(state, env);
            }
        },
        |ui| {
            ui.add(Button::primary(strings::feedback::CLOSE()))
                .clicked()
        },
    );
    if r.close_requested || r.footer {
        crate::widgets::modal::closed(ctx, id);
        state.nav.close(Overlay::History);
    }
}

fn load_history(state: &mut UiState, env: &mut Env<'_>) {
    let q = env.bridge.query_id();
    state.stats.history_q = Some(q);
    env.bridge.send(Command::QueryStats {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_year_is_the_twelve_months_up_to_this_one() {
        // 16 Nov 2026 → 1 Dec 2025.
        let to = days_from_civil(2026, 11, 16);
        let (y, m, _) = civil_from_days(to);
        let back = i64::from(m) - 1 - 11;
        let (y, m) = (y + back.div_euclid(12), back.rem_euclid(12) as u32 + 1);
        assert_eq!((y, m), (2025, 12));
        // March → April of the year before.
        let back = 3 - 1 - 11_i64;
        assert_eq!((back.div_euclid(12), back.rem_euclid(12) + 1), (-1, 4));
    }
}
