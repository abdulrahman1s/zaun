//! Statistics queries (STATISTICS §3, §6): totals, zero-filled series bucketed by day, week or
//! month, rankings, streaks, weekday distribution and history pages.
//!
//! Runs on the job pool against a read-only connection. Only counted sessions contribute
//! (aggregates contain nothing else; history filters on `counted` unless asked not to).

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::{Date, Weekday};

use crate::clock::Timestamp;
use crate::ids::{GameId, SessionId};
use crate::model::{EndReason, GameKey, GameStats, Session, SessionKind, SessionState};
use crate::storage::{SessionRepo, StatsRepo, StorageResult};

use super::rollup::{civil_from_days, date_key, day_key, days_from_civil, key_days};

/// An inclusive range of local days.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateRange {
    /// First day; `None` = all time (the series starts at the earliest data).
    pub from: Option<Date>,
    /// Last day.
    pub to: Date,
}

impl DateRange {
    /// The last `n` days ending `today` (n ≥ 1).
    pub fn last_days(today: Date, n: u32) -> Self {
        let from = key_days(date_key(today)) - i64::from(n.max(1)) + 1;
        Self {
            from: crate::stats::rollup::key_date(day_key(from)),
            to: today,
        }
    }

    /// 1 January of `today`'s year to `today`.
    pub fn this_year(today: Date) -> Self {
        Self {
            from: Date::new(today.year(), 1, 1).ok(),
            to: today,
        }
    }

    /// Everything up to `today`.
    pub fn all_time(today: Date) -> Self {
        Self {
            from: None,
            to: today,
        }
    }

    fn keys(&self) -> (u32, u32) {
        (self.from.map_or(0, date_key), date_key(self.to))
    }
}

/// Series granularity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Days.
    Day,
    /// Weeks starting on the configured weekday.
    Week,
    /// Calendar months.
    Month,
}

/// A zero-filled time series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    /// Granularity.
    pub bucket: Bucket,
    /// `(YYYYMMDD of the bucket start, ms)`, chronological, no gaps.
    pub points: Vec<(i32, u64)>,
}

/// Bucket for a range of `days` days: ≤ 31 daily, ≤ 26 weeks weekly, otherwise monthly.
pub fn bucket_for(days: i64) -> Bucket {
    if days <= 31 {
        Bucket::Day
    } else if days <= 26 * 7 {
        Bucket::Week
    } else {
        Bucket::Month
    }
}

/// Monday-zero weekday of a day number (1970-01-01 was a Thursday).
fn weekday_of(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

fn week_start(days: i64, first: Weekday) -> i64 {
    let off = (weekday_of(days) - i64::from(first.to_monday_zero_offset())).rem_euclid(7);
    days - off
}

fn month_start(days: i64) -> i64 {
    let (y, m, _) = civil_from_days(days);
    days_from_civil(y, m, 1)
}

fn next_month(days: i64) -> i64 {
    let (y, m, _) = civil_from_days(days);
    if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    }
}

/// Group daily `(day key, ms)` rows into a zero-filled series over `[from, to]` (day
/// numbers) with `bucket`, weeks starting on `first`.
pub fn build_series(
    daily: &BTreeMap<u32, u64>,
    from: i64,
    to: i64,
    bucket: Bucket,
    first: Weekday,
) -> Series {
    let start_of = |d: i64| match bucket {
        Bucket::Day => d,
        Bucket::Week => week_start(d, first),
        Bucket::Month => month_start(d),
    };
    let step = |d: i64| match bucket {
        Bucket::Day => d + 1,
        Bucket::Week => d + 7,
        Bucket::Month => next_month(d),
    };
    let mut sums: BTreeMap<i64, u64> = BTreeMap::new();
    for (k, ms) in daily {
        let d = key_days(*k);
        if d < from || d > to {
            continue;
        }
        *sums.entry(start_of(d)).or_insert(0) += ms;
    }
    let mut points = Vec::new();
    let mut b = start_of(from);
    while b <= to {
        let key = i32::try_from(day_key(b)).unwrap_or(i32::MAX);
        points.push((key, sums.get(&b).copied().unwrap_or(0)));
        b = step(b);
    }
    Series { bucket, points }
}

/// `(current, longest)` streaks of consecutive days with play; the current streak must end
/// today or yesterday.
pub fn streaks(days_with_play: &BTreeSet<i64>, today: i64) -> (u32, u32) {
    let mut longest = 0u32;
    let mut run = 0u32;
    let mut prev: Option<i64> = None;
    for &d in days_with_play {
        run = if prev == Some(d - 1) { run + 1 } else { 1 };
        longest = longest.max(run);
        prev = Some(d);
    }
    let current = match prev {
        Some(last) if last == today || last == today - 1 => run,
        _ => 0,
    };
    (current, longest)
}

/// Context of a request: the local date and the week start from preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryContext {
    /// Today, local.
    pub today: Date,
    /// First day of the week.
    pub week_start: Weekday,
}

/// A request (STATISTICS §6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StatsRequest {
    /// Totals, streaks, top games and a series for the whole library.
    LibraryOverview {
        /// Range.
        range: DateRange,
    },
    /// Totals, series and recent sessions of one game.
    GameOverview {
        /// Game.
        game: GameId,
        /// Range.
        range: DateRange,
    },
    /// A page of the session history, newest first.
    SessionHistory {
        /// One game, or all.
        game: Option<GameId>,
        /// Page number from 0.
        page: u32,
        /// Rows per page (1 … 500).
        page_size: u32,
        /// Also uncounted `Game` sessions (failed or too short).
        include_uncounted: bool,
        /// Also `Tool` and `Installer` sessions (`general.show_tool_sessions_in_history`).
        include_tools: bool,
    },
    /// Most played games in a range.
    TopGames {
        /// Range.
        range: DateRange,
        /// At most this many.
        limit: u32,
    },
    /// Playtime by weekday.
    WeekdayDistribution {
        /// Range.
        range: DateRange,
    },
}

/// Totals over a range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Playtime.
    pub ms: u64,
    /// Counted sessions that started in the range.
    pub sessions: u32,
    /// Distinct games with playtime in the range.
    pub games_played: u32,
}

/// One row of a ranking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranking {
    /// The game or removed game.
    pub key: GameKey,
    /// Playtime in the range.
    pub ms: u64,
}

/// [`StatsRequest::LibraryOverview`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryOverview {
    /// Totals in the range.
    pub totals: Totals,
    /// Zero-filled series.
    pub series: Series,
    /// Top five games in the range.
    pub top: Vec<Ranking>,
    /// Current streak in days (all-time data).
    pub current_streak: u32,
    /// Longest streak in days (all-time data).
    pub longest_streak: u32,
}

/// One history row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    /// Session.
    pub id: SessionId,
    /// Game (`None` for kept history of a removed game).
    pub game_id: Option<GameId>,
    /// Name snapshot.
    pub game_name: String,
    /// Kind.
    pub kind: SessionKind,
    /// Start.
    pub started_at: Timestamp,
    /// Duration.
    pub duration_ms: u64,
    /// End reason.
    pub end_reason: Option<EndReason>,
    /// Counted.
    pub counted: bool,
}

impl HistoryRow {
    fn of(s: Session) -> Self {
        Self {
            id: s.id,
            game_id: s.game_id,
            game_name: s.game_name,
            kind: s.kind,
            started_at: s.started_at,
            duration_ms: s.duration_ms.unwrap_or(0),
            end_reason: s.end_reason,
            counted: s.counted,
        }
    }
}

/// A history page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryPage {
    /// Rows, newest first.
    pub rows: Vec<HistoryRow>,
    /// More rows follow.
    pub has_more: bool,
}

/// [`StatsRequest::GameOverview`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameOverview {
    /// All-time aggregates (`None`: never launched).
    pub stats: Option<GameStats>,
    /// Totals in the range (`games_played` is 0 or 1).
    pub totals: Totals,
    /// Zero-filled series.
    pub series: Series,
    /// The five newest counted sessions.
    pub recent: Vec<HistoryRow>,
    /// Duration of the newest counted session.
    pub last_session_ms: Option<u64>,
    /// `total_ms / sessions`, hidden when there are no sessions.
    pub average_ms: Option<u64>,
}

/// Result of [`run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatsResult {
    /// Library overview.
    Library(LibraryOverview),
    /// Game overview.
    Game(GameOverview),
    /// History page.
    History(HistoryPage),
    /// Ranking.
    Top(Vec<Ranking>),
    /// Monday-first playtime per weekday.
    Weekdays([u64; 7]),
}

/// Most rows of one history page.
pub const MAX_PAGE_SIZE: u32 = 500;

fn series_bounds(range: &DateRange, first_data: Option<u32>) -> (i64, i64) {
    let to = key_days(date_key(range.to));
    let from = match range.from {
        Some(f) => key_days(date_key(f)),
        None => first_data.map_or(to, key_days).min(to),
    };
    (from, to)
}

fn rank(sums: BTreeMap<GameKey, u64>, limit: usize) -> Vec<Ranking> {
    let mut v: Vec<Ranking> = sums
        .into_iter()
        .filter(|(_, ms)| *ms > 0)
        .map(|(key, ms)| Ranking { key, ms })
        .collect();
    v.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.key.cmp(&b.key)));
    v.truncate(limit);
    v
}

/// Per-key sums in a range: all time from `game_stats` (one row per game) plus kept history
/// of removed games; bounded ranges with one grouped scan of `playtime_daily`
/// (`StatsRepo::playtime_by_key`), which includes kept history too.
fn key_sums<S: StatsRepo + ?Sized>(
    store: &mut S,
    range: &DateRange,
) -> StorageResult<BTreeMap<GameKey, u64>> {
    let (from, to) = range.keys();
    if range.from.is_some() {
        return Ok(store.playtime_by_key(from, to)?.into_iter().collect());
    }
    let mut sums: BTreeMap<GameKey, u64> = store
        .playtime_by_key(0, u32::MAX)?
        .into_iter()
        .filter(|(k, _)| matches!(k, GameKey::Removed(_)))
        .collect();
    for s in store.all_game_stats()? {
        sums.insert(GameKey::Game(s.game_id), s.total_ms);
    }
    Ok(sums)
}

fn library<S: StatsRepo + ?Sized>(
    store: &mut S,
    ctx: &QueryContext,
    range: &DateRange,
) -> StorageResult<LibraryOverview> {
    let today = key_days(date_key(ctx.today));
    let played: BTreeSet<i64> = store
        .played_days(0, u32::MAX)?
        .into_iter()
        .map(key_days)
        .collect();
    let (current_streak, longest_streak) = streaks(&played, today);
    let (kf, kt) = range.keys();
    let in_range = store.daily_totals(kf, kt)?;
    let daily: BTreeMap<u32, u64> = in_range.iter().map(|d| (d.day, d.ms)).collect();
    let (from, to) = series_bounds(range, in_range.first().map(|d| d.day));
    let sums = key_sums(store, range)?;
    let totals = Totals {
        ms: in_range.iter().map(|d| d.ms).sum(),
        sessions: in_range.iter().map(|d| d.sessions).sum(),
        games_played: sums.values().filter(|ms| **ms > 0).count() as u32,
    };
    Ok(LibraryOverview {
        totals,
        series: build_series(&daily, from, to, bucket_for(to - from + 1), ctx.week_start),
        top: rank(sums, 5),
        current_streak,
        longest_streak,
    })
}

fn game<S: StatsRepo + SessionRepo + ?Sized>(
    store: &mut S,
    ctx: &QueryContext,
    game: GameId,
    range: &DateRange,
) -> StorageResult<GameOverview> {
    let key = GameKey::Game(game);
    let (kf, kt) = range.keys();
    let rows = store.playtime_range(Some(&key), kf, kt)?;
    let daily: BTreeMap<u32, u64> = rows.iter().map(|d| (d.day, d.ms)).collect();
    let (from, to) = series_bounds(range, rows.first().map(|d| d.day));
    let stats = store.game_stats(game)?;
    let ms: u64 = rows.iter().map(|d| d.ms).sum();
    let totals = Totals {
        ms,
        sessions: rows.iter().map(|d| d.sessions).sum(),
        games_played: u32::from(ms > 0),
    };
    let recent = history(store, Some(game), 0, 5, false, false)?.rows;
    Ok(GameOverview {
        average_ms: stats
            .as_ref()
            .filter(|s| s.sessions > 0)
            .map(|s| s.total_ms / u64::from(s.sessions)),
        stats,
        totals,
        series: build_series(&daily, from, to, bucket_for(to - from + 1), ctx.week_start),
        last_session_ms: recent.first().map(|r| r.duration_ms),
        recent,
    })
}

/// A history page: ended sessions newest first, counted only unless `include_uncounted`,
/// `Game` kind only unless `include_tools`.
pub fn history<S: SessionRepo + ?Sized>(
    store: &mut S,
    game: Option<GameId>,
    page: u32,
    page_size: u32,
    include_uncounted: bool,
    include_tools: bool,
) -> StorageResult<HistoryPage> {
    let size = page_size.clamp(1, MAX_PAGE_SIZE) as usize;
    let skip = page as usize * size;
    let kinds: &[SessionKind] = if include_tools {
        &[SessionKind::Game, SessionKind::Tool, SessionKind::Installer]
    } else {
        &[SessionKind::Game]
    };
    let mut rows = Vec::new();
    let mut matched = 0usize;
    let mut offset = 0u64;
    let chunk = (size as u64 * 2).max(64);
    loop {
        let batch = store.session_history(game, kinds, offset, chunk)?;
        let n = batch.len() as u64;
        offset += n;
        for s in batch {
            let wanted = s.state == SessionState::Ended
                && (s.counted || include_uncounted || s.kind != SessionKind::Game);
            if !wanted {
                continue;
            }
            matched += 1;
            if matched <= skip {
                continue;
            }
            if rows.len() == size {
                return Ok(HistoryPage {
                    rows,
                    has_more: true,
                });
            }
            rows.push(HistoryRow::of(s));
        }
        if n < chunk {
            return Ok(HistoryPage {
                rows,
                has_more: false,
            });
        }
    }
}

/// Answer `req`.
pub fn run<S: StatsRepo + SessionRepo + ?Sized>(
    store: &mut S,
    ctx: &QueryContext,
    req: &StatsRequest,
) -> StorageResult<StatsResult> {
    Ok(match req {
        StatsRequest::LibraryOverview { range } => {
            StatsResult::Library(library(store, ctx, range)?)
        }
        StatsRequest::GameOverview { game: g, range } => {
            StatsResult::Game(game(store, ctx, *g, range)?)
        }
        StatsRequest::SessionHistory {
            game,
            page,
            page_size,
            include_uncounted,
            include_tools,
        } => StatsResult::History(history(
            store,
            *game,
            *page,
            *page_size,
            *include_uncounted,
            *include_tools,
        )?),
        StatsRequest::TopGames { range, limit } => {
            let sums = key_sums(store, range)?;
            StatsResult::Top(rank(sums, *limit as usize))
        }
        StatsRequest::WeekdayDistribution { range } => {
            let (kf, kt) = range.keys();
            let mut out = [0u64; 7];
            for d in store.daily_totals(kf, kt)? {
                out[weekday_of(key_days(d.day)) as usize] += d.ms;
            }
            StatsResult::Weekdays(out)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::rollup::key_date;

    fn d(k: u32) -> i64 {
        key_days(k)
    }

    #[test]
    fn buckets() {
        assert_eq!(bucket_for(1), Bucket::Day);
        assert_eq!(bucket_for(31), Bucket::Day);
        assert_eq!(bucket_for(32), Bucket::Week);
        assert_eq!(bucket_for(182), Bucket::Week);
        assert_eq!(bucket_for(183), Bucket::Month);
    }

    #[test]
    fn weekdays_and_week_starts() {
        assert_eq!(weekday_of(d(20_261_112)), 3); // Thursday
        assert_eq!(
            day_key(week_start(d(20_261_112), Weekday::Monday)),
            20_261_109
        );
        assert_eq!(
            day_key(week_start(d(20_261_112), Weekday::Sunday)),
            20_261_108
        );
        assert_eq!(
            day_key(week_start(d(20_261_108), Weekday::Sunday)),
            20_261_108
        );
        // Year boundary: 2027-01-01 is a Friday.
        assert_eq!(
            day_key(week_start(d(20_270_101), Weekday::Monday)),
            20_261_228
        );
        assert_eq!(
            day_key(week_start(d(20_270_103), Weekday::Sunday)),
            20_270_103
        );
    }

    #[test]
    fn series_zero_filled() {
        let daily: BTreeMap<u32, u64> = [(20_261_110, 5), (20_261_112, 7)].into_iter().collect();
        let s = build_series(
            &daily,
            d(20_261_109),
            d(20_261_113),
            Bucket::Day,
            Weekday::Monday,
        );
        assert_eq!(
            s.points,
            vec![
                (20_261_109, 0),
                (20_261_110, 5),
                (20_261_111, 0),
                (20_261_112, 7),
                (20_261_113, 0)
            ]
        );
        let w = build_series(
            &daily,
            d(20_261_101),
            d(20_261_130),
            Bucket::Week,
            Weekday::Monday,
        );
        assert_eq!(w.points.first(), Some(&(20_261_026, 0)));
        assert!(w.points.contains(&(20_261_109, 12)));
        let m = build_series(
            &daily,
            d(20_260_901),
            d(20_261_231),
            Bucket::Month,
            Weekday::Monday,
        );
        assert_eq!(
            m.points,
            vec![
                (20_260_901, 0),
                (20_261_001, 0),
                (20_261_101, 12),
                (20_261_201, 0)
            ]
        );
        let across = build_series(
            &BTreeMap::new(),
            d(20_261_201),
            d(20_270_131),
            Bucket::Month,
            Weekday::Monday,
        );
        assert_eq!(across.points.len(), 2);
    }

    #[test]
    fn streak_cases() {
        let set = |ks: &[u32]| ks.iter().map(|k| d(*k)).collect::<BTreeSet<_>>();
        let today = d(20_261_112);
        assert_eq!(streaks(&set(&[]), today), (0, 0));
        assert_eq!(streaks(&set(&[20_261_112]), today), (1, 1));
        assert_eq!(streaks(&set(&[20_261_110, 20_261_111]), today), (2, 2));
        assert_eq!(streaks(&set(&[20_261_109, 20_261_110]), today), (0, 2));
        assert_eq!(
            streaks(
                &set(&[20_261_101, 20_261_102, 20_261_103, 20_261_111, 20_261_112]),
                today
            ),
            (2, 3)
        );
        assert_eq!(
            streaks(&set(&[20_261_231, 20_270_101]), d(20_270_101)),
            (2, 2)
        );
    }

    #[test]
    fn ranges() {
        let today = key_date(20_261_112).unwrap();
        let r = DateRange::last_days(today, 7);
        assert_eq!(r.keys(), (20_261_106, 20_261_112));
        assert_eq!(DateRange::this_year(today).keys(), (20_260_101, 20_261_112));
        assert_eq!(DateRange::all_time(today).keys(), (0, 20_261_112));
    }
}
