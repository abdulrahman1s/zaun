//! Statistics (SCREEN_SPECS_2 §12, STATISTICS §8) and the bar chart shared with Activity.

use zaun_core::clock::Timestamp;
use zaun_core::stats::Bucket;

texts! {
    /// Toast.
    CLEARED = "stats.cleared";
    /// Range control.
    RANGE = "stats.range";
}
/// Ranges.
#[allow(non_snake_case)]
pub fn RANGES() -> [&'static str; 4] {
    [
        crate::i18n::text("stats.ranges.7_days"),
        crate::i18n::text("stats.ranges.30_days"),
        crate::i18n::text("stats.ranges.year"),
        crate::i18n::text("stats.ranges.all_time"),
    ]
}
/// What each range covers, after a duration: "9 h 48 min in the last 7 days".
#[allow(non_snake_case)]
pub fn RANGE_SAYS() -> [&'static str; 4] {
    [
        crate::i18n::text("stats.range_says.in_the_last_7"),
        crate::i18n::text("stats.range_says.in_the_last_30"),
        crate::i18n::text("stats.range_says.in_the_last_year"),
        crate::i18n::text("stats.range_says.in_all"),
    ]
}
texts! {
    /// The chart's toggle.
    SHOW_TABLE = "stats.show_table";
    /// The chart's toggle.
    SHOW_CHART = "stats.show_chart";
    /// Empty.
    EMPTY_TITLE = "stats.empty_title";
    /// Empty.
    EMPTY_BODY = "stats.empty_body";
    /// Range without play.
    NO_PLAY_IN_RANGE = "stats.no_play_in_range";
    /// Failure.
    FAILED = "stats.failed";
    /// Button.
    RETRY = "stats.retry";
    /// Section.
    MOST_PLAYED = "stats.most_played";
    /// Section.
    RECENT = "stats.recent";
    /// Link (Activity).
    SHOW_ALL = "stats.show_all";
    /// Button.
    SHOW_FEWER = "stats.show_fewer";
    /// Button under Recent sessions: opens the full play history.
    SHOW_HISTORY = "stats.show_history";
    /// Section.
    WEEKDAYS = "stats.weekdays";
    /// What the numbers count; fine print at the foot of the page.
    ABOUT_BODY = "stats.about_body";
    /// History sheet.
    HISTORY = "stats.history";
    /// Filter.
    FILTER_GAME = "stats.filter_game";
    /// Filter.
    ALL_GAMES = "stats.all_games";
    /// Column.
    COL_DATE = "stats.col_date";
    /// Column.
    COL_DURATION = "stats.col_duration";
    /// Column.
    COL_END = "stats.col_end";
}
/// Weekday names, Monday first.
#[allow(non_snake_case)]
pub fn WEEKDAY_SHORT() -> [&'static str; 7] {
    [
        crate::i18n::text("stats.weekday_short.mon"),
        crate::i18n::text("stats.weekday_short.tue"),
        crate::i18n::text("stats.weekday_short.wed"),
        crate::i18n::text("stats.weekday_short.thu"),
        crate::i18n::text("stats.weekday_short.fri"),
        crate::i18n::text("stats.weekday_short.sat"),
        crate::i18n::text("stats.weekday_short.sun"),
    ]
}
fn ymd(key: u32) -> (u32, usize, u32) {
    (
        key / 10_000,
        ((key / 100) % 100).clamp(1, 12) as usize - 1,
        key % 100,
    )
}

/// "16 Nov" for a day or a week, "Nov" for a month: the label under a bar.
pub fn bucket_label(key: u32, b: Bucket) -> String {
    let (_, m, d) = ymd(key);
    match b {
        Bucket::Day | Bucket::Week => super::date::day_month(d, super::date::month_short(m)),
        Bucket::Month => super::date::month_short(m).to_owned(),
    }
}

/// The bar's tooltip, its row in the table and what is announced: "16 Nov: 29 min",
/// "Week of 9 Nov: 3 h", "November 2026: 15 h". The part before ": " names the bar.
pub fn bucket_spoken(key: u32, b: Bucket, ms: u64) -> String {
    let (y, m, d) = ymd(key);
    let day_month = super::date::day_month(d, super::date::month_short(m));
    let when = match b {
        Bucket::Day => day_month,
        Bucket::Week => rust_i18n::t!("stats.week_of", date = day_month).into_owned(),
        Bucket::Month => super::date::month_year(super::date::month_full(m), y),
    };
    rust_i18n::t!(
        "stats.bucket_spoken",
        when = when,
        time = super::fmt::play_time(ms)
    )
    .into_owned()
}

/// "Monday: 4 h 10 min".
pub fn weekday_spoken(i: usize, ms: u64) -> String {
    rust_i18n::t!(
        "stats.weekday_spoken",
        day = super::date::weekday_full(i.min(6)),
        time = super::fmt::play_time(ms)
    )
    .into_owned()
}

/// The page's headline: "9 h 48 min in the last 7 days".
pub fn headline(ms: u64, range: usize) -> String {
    rust_i18n::t!(
        "stats.headline",
        time = super::fmt::play_time(ms),
        range = RANGE_SAYS()[range.min(3)]
    )
    .into_owned()
}

/// The sentence under the headline: "9 games and 12 sessions. You have played 7 days in a
/// row." The streak is said only from two days on.
pub fn summary(games: u32, sessions: u32, streak: u32) -> String {
    let games = crate::i18n::plural::counted("stats.games", u64::from(games));
    let sessions = crate::i18n::plural::counted("stats.sessions", u64::from(sessions));
    if streak > 1 {
        crate::i18n::plural::counted_args(
            "stats.summary_streak",
            u64::from(streak),
            &[("games", &games), ("sessions", &sessions)],
        )
    } else {
        rust_i18n::t!("stats.summary", games = games, sessions = sessions).into_owned()
    }
}

/// The chart's accessible name: "Playtime, in the last 7 days. Show as table for the
/// numbers."
pub fn chart_name(says: &str) -> String {
    rust_i18n::t!("stats.chart_name", says = says).into_owned()
}

/// "Show all 9".
pub fn show_all(n: usize) -> String {
    rust_i18n::t!("stats.show_all_n", n = n).into_owned()
}

/// The day of a session in a list: "Today", "Yesterday", the weekday within a week, else
/// "4 Sept" (with the year when it is another year).
pub fn day(t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    let local = |t: Timestamp| (t.as_millis() + i64::from(offset_s) * 1000).div_euclid(86_400_000);
    let (d, today) = (local(t), local(now));
    match today - d {
        0 => super::date::today().to_owned(),
        1 => super::date::yesterday().to_owned(),
        2..=6 => super::date::weekday_full((d + 3).rem_euclid(7) as usize).to_owned(),
        _ => {
            let (y, m, dd) = zaun_core::stats::rollup::civil_from_days(d);
            let (ty, _, _) = zaun_core::stats::rollup::civil_from_days(today);
            let month = super::date::month_short((m.clamp(1, 12) - 1) as usize);
            if y == ty {
                super::date::day_month(dd, month)
            } else {
                super::date::day_month_year(dd, month, y)
            }
        }
    }
}

/// "4 days".
pub fn days(n: u32) -> String {
    crate::i18n::plural::counted("stats.days", u64::from(n))
}

/// "1. Hollow Knight, 12 hours 5 minutes".
pub fn ranked(rank: usize, name: &str, spoken: &str) -> String {
    rust_i18n::t!("stats.ranked", rank = rank, name = name, spoken = spoken).into_owned()
}

/// "Today, Starfall Tactics, 29 minutes".
pub fn session_spoken(day: &str, name: &str, spoken: &str) -> String {
    rust_i18n::t!(
        "stats.session_spoken",
        day = day,
        name = name,
        spoken = spoken
    )
    .into_owned()
}

/// "21:04" in local time.
pub fn clock_time(t: Timestamp, offset_s: i32) -> String {
    let ms = t.as_millis() + i64::from(offset_s) * 1000;
    let min = ms.div_euclid(60_000).rem_euclid(24 * 60);
    format!("{:02}:{:02}", min / 60, min % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_labels() {
        assert_eq!(bucket_label(20_261_103, Bucket::Day), "3 Nov");
        assert_eq!(
            bucket_spoken(20_261_103, Bucket::Day, 7_800_000),
            "3 Nov: 2 h 10 min"
        );
        assert_eq!(bucket_label(20_261_101, Bucket::Month), "Nov");
        assert_eq!(bucket_label(20_260_901, Bucket::Month), "Sept");
        assert_eq!(
            bucket_spoken(20_261_101, Bucket::Month, 54_000_000),
            "November 2026: 15 h"
        );
        assert_eq!(
            clock_time(Timestamp::from_millis(21 * 3_600_000 + 4 * 60_000), 0),
            "21:04"
        );
    }

    #[test]
    fn the_headline_and_its_sentence() {
        assert_eq!(headline(588 * 60_000, 0), "9 h 48 min in the last 7 days");
        assert_eq!(headline(24 * 3_600_000, 3), "24 h in all");
        assert_eq!(
            summary(9, 12, 7),
            "9 games and 12 sessions. You have played 7 days in a row."
        );
        assert_eq!(summary(1, 1, 1), "1 game and 1 session.");
    }

    #[test]
    fn days_of_sessions() {
        let day_ms = 86_400_000;
        // 2026-11-16 is a Monday.
        let today = zaun_core::stats::rollup::days_from_civil(2026, 11, 16);
        let now = Timestamp::from_millis(today * day_ms + 12 * 3_600_000);
        assert_eq!(day(now, now, 0), "Today");
        assert_eq!(
            day(Timestamp::from_millis(now.as_millis() - day_ms), now, 0),
            "Yesterday"
        );
        assert_eq!(
            day(Timestamp::from_millis(now.as_millis() - 2 * day_ms), now, 0),
            "Saturday"
        );
        assert_eq!(
            day(
                Timestamp::from_millis(now.as_millis() - 73 * day_ms),
                now,
                0
            ),
            "4 Sept"
        );
    }
}
