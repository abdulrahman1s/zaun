//! Statistics (SCREEN_SPECS_2 §12, STATISTICS §8).

use easyplay_core::clock::Timestamp;
use easyplay_core::stats::Bucket;

/// Toast.
pub const CLEARED: &str = "Play history cleared";
/// Range control.
pub const RANGE: &str = "Range";
/// Ranges.
pub const RANGES: [&str; 4] = ["7 days", "30 days", "Year", "All time"];
/// Tile.
pub const TOTAL: &str = "Total playtime";
/// Tile.
pub const GAMES_PLAYED: &str = "Games played";
/// Tile.
pub const SESSIONS: &str = "Sessions";
/// Tile.
pub const STREAK: &str = "Current streak";
/// Tile.
pub const AVERAGE: &str = "Average session";
/// Tile.
pub const LONGEST: &str = "Longest session";
/// Tile.
pub const FIRST_PLAYED: &str = "First played";
/// Tile.
pub const LAST_PLAYED: &str = "Last played";
/// Chart heading.
pub const PER_DAY: &str = "Playtime per day";
/// Toggle.
pub const SHOW_TABLE: &str = "Show as table";
/// Toggle.
pub const SHOW_CHART: &str = "Show as chart";
/// Empty.
pub const EMPTY_TITLE: &str = "Nothing to show yet";
/// Empty.
pub const EMPTY_BODY: &str = "Play something and your time will show up here.";
/// Range without play.
pub const NO_PLAY_IN_RANGE: &str = "No play in this period.";
/// Failure.
pub const FAILED: &str = "Couldn't load statistics";
/// Button.
pub const RETRY: &str = "Retry";
/// Section.
pub const MOST_PLAYED: &str = "Most played";
/// Section.
pub const RECENT: &str = "Recent sessions";
/// Link.
pub const SHOW_ALL: &str = "Show all";
/// Link.
pub const SHOW_FEWER: &str = "Show fewer";
/// Section.
pub const WEEKDAYS: &str = "Busiest days";
/// Popover.
pub const ABOUT: &str = "About these numbers";
/// Popover body.
pub const ABOUT_BODY: &str = "Playtime counts the time a game's window was open, from start to exit, rounded to whole minutes. Sessions shorter than the minimum in Settings, and starts that failed, are not counted. Times are in your local time zone.";
/// History sheet.
pub const HISTORY: &str = "Play history";
/// Filter.
pub const FILTER_GAME: &str = "Game";
/// Filter.
pub const ALL_GAMES: &str = "All games";
/// Column.
pub const COL_DATE: &str = "Date";
/// Column.
pub const COL_START: &str = "Started";
/// Column.
pub const COL_DURATION: &str = "Duration";
/// Column.
pub const COL_END: &str = "How it ended";
/// Weekday initials, Monday first.
pub const WEEKDAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn ymd(key: u32) -> (u32, usize, u32) {
    (
        key / 10_000,
        ((key / 100) % 100).clamp(1, 12) as usize - 1,
        key % 100,
    )
}

fn weekday(key: u32) -> usize {
    let days = easyplay_core::stats::rollup::key_days(key);
    (days + 3).rem_euclid(7) as usize
}

/// "3 Nov", "Nov".
pub fn bucket_label(key: u32, b: Bucket) -> String {
    let (_, m, d) = ymd(key);
    match b {
        Bucket::Day | Bucket::Week => format!("{d} {}", MONTHS[m]),
        Bucket::Month => MONTHS[m].to_owned(),
    }
}

/// "Tuesday 3 November: 2 h 10 min".
pub fn bucket_spoken(key: u32, b: Bucket, ms: u64) -> String {
    let (y, m, d) = ymd(key);
    let when = match b {
        Bucket::Day => format!("{} {d} {}", WEEKDAYS_FULL[weekday(key)], MONTHS_FULL[m]),
        Bucket::Week => format!("Week of {d} {}", MONTHS_FULL[m]),
        Bucket::Month => format!("{} {y}", MONTHS_FULL[m]),
    };
    format!("{when}: {}", super::fmt::duration(ms))
}

/// "Monday: 4 h 10 min".
pub fn weekday_spoken(i: usize, ms: u64) -> String {
    format!("{}: {}", WEEKDAYS_FULL[i.min(6)], super::fmt::duration(ms))
}

/// "Playtime per day/week/month".
pub fn per_bucket(b: Bucket) -> String {
    match b {
        Bucket::Day => PER_DAY.to_owned(),
        Bucket::Week => "Playtime per week".to_owned(),
        Bucket::Month => "Playtime per month".to_owned(),
    }
}

/// "4 days".
pub fn days(n: u32) -> String {
    format!("{n} {}", super::plural(u64::from(n), "day", "days"))
}

/// "1. Hollow Knight, 12 hours 5 minutes".
pub fn ranked(rank: usize, name: &str, spoken: &str) -> String {
    format!("{rank}. {name}, {spoken}")
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
            "Tuesday 3 November: 2 h 10 min"
        );
        assert_eq!(bucket_label(20_261_101, Bucket::Month), "Nov");
        assert_eq!(
            clock_time(Timestamp::from_millis(21 * 3_600_000 + 4 * 60_000), 0),
            "21:04"
        );
    }
}
