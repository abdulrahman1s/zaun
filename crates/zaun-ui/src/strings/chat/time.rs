//! Chat strings: message times (chat-calls.html notes A). Keys live under `chat.time.` in
//! locales/en.yml and ar.yml. Local calendar days (never 24-hour spans), a 24-hour clock and
//! dd/mm/yyyy; Western digits in every language. Statistics keeps its own formats.

use std::collections::HashMap;
use std::sync::Arc;

use zaun_core::clock::Timestamp;
use zaun_core::stats::rollup::civil_from_days;

use crate::i18n::Language;

const DAY_MS: i64 = 86_400_000;

/// The local day number (days since 1970-01-01) and the minute of that day.
fn local(t: Timestamp, offset_s: i32) -> (i64, i64) {
    let ms = t.as_millis() + i64::from(offset_s) * 1000;
    (ms.div_euclid(DAY_MS), ms.rem_euclid(DAY_MS) / 60_000)
}

/// "13:59".
pub fn clock(t: Timestamp, offset_s: i32) -> String {
    let (_, min) = local(t, offset_s);
    format!("{:02}:{:02}", min / 60, min % 60)
}

/// The weekday of a local day, Monday = 0 (1970-01-01 was a Thursday).
fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

fn weekday_name(lang: Language, day: i64) -> &'static str {
    crate::i18n::text_in(lang, &format!("date.weekday_full.{}", weekday(day) + 1))
}

fn month_name(lang: Language, month: u32) -> &'static str {
    crate::i18n::text_in(lang, &format!("date.month_full.{}", month.clamp(1, 12)))
}

/// "12/11/2026".
fn numeric(lang: Language, day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    rust_i18n::t!(
        "chat.time.numeric",
        locale = lang.code(),
        dd = format!("{d:02}"),
        mm = format!("{m:02}"),
        yyyy = y
    )
    .into_owned()
}

/// A group's head, after the name: "Today at 13:59", "Yesterday at 13:59", else
/// "12/11/2026 21:40".
pub fn head_time(lang: Language, t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    let (day, today) = (local(t, offset_s).0, local(now, offset_s).0);
    let time = clock(t, offset_s);
    let code = lang.code();
    match today - day {
        0 => rust_i18n::t!("chat.time.today_at", locale = code, time = time).into_owned(),
        1 => rust_i18n::t!("chat.time.yesterday_at", locale = code, time = time).into_owned(),
        _ => rust_i18n::t!(
            "chat.time.date_time",
            locale = code,
            date = numeric(lang, day),
            time = time
        )
        .into_owned(),
    }
}

/// A day divider: "Tuesday, 17 November 2026" (always the full date, never "Today").
pub fn day_title(lang: Language, t: Timestamp, offset_s: i32) -> String {
    let day = local(t, offset_s).0;
    let (y, m, d) = civil_from_days(day);
    rust_i18n::t!(
        "chat.time.day",
        locale = lang.code(),
        weekday = weekday_name(lang, day),
        d = d,
        month = month_name(lang, m),
        y = y
    )
    .into_owned()
}

/// The tooltip on any time, and its spoken label: "Tuesday, 17 November 2026 at 13:59".
pub fn full_time(lang: Language, t: Timestamp, offset_s: i32) -> String {
    let day = local(t, offset_s).0;
    let (y, m, d) = civil_from_days(day);
    rust_i18n::t!(
        "chat.time.full",
        locale = lang.code(),
        weekday = weekday_name(lang, day),
        d = d,
        month = month_name(lang, m),
        y = y,
        time = clock(t, offset_s)
    )
    .into_owned()
}

/// A chat's time in the list: "13:59" today, "Yesterday", the weekday within six days, then
/// "09/11/2026".
pub fn list_time(lang: Language, t: Timestamp, now: Timestamp, offset_s: i32) -> String {
    let (day, today) = (local(t, offset_s).0, local(now, offset_s).0);
    match today - day {
        i64::MIN..=0 => clock(t, offset_s),
        1 => crate::i18n::text_in(lang, "date.yesterday").to_owned(),
        2..=6 => weekday_name(lang, day).to_owned(),
        _ => numeric(lang, day),
    }
}

/// Which of the formats above.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Clock,
    Head,
    Day,
    Full,
    List,
}

/// Formatted times, so a frame formats nothing it formatted before (AGENTS.md §6).
///
/// Eviction rule: the cache holds one local day, offset and language. A lookup for another
/// "today", offset or language empties it first, and it is emptied when it reaches
/// [`Times::CAP`] entries (a few hundred messages on screen need far fewer).
#[derive(Clone, Default)]
pub struct Times {
    today: i64,
    offset_s: i32,
    lang: Option<Language>,
    map: HashMap<(Kind, i64), Arc<str>>,
}

impl Times {
    /// The most entries kept before the cache starts over.
    pub const CAP: usize = 2048;

    /// `t` formatted as `kind` (to the minute), relative to `now`.
    pub fn get(
        &mut self,
        lang: Language,
        kind: Kind,
        t: Timestamp,
        now: Timestamp,
        offset_s: i32,
    ) -> Arc<str> {
        let today = local(now, offset_s).0;
        if self.lang != Some(lang) || self.today != today || self.offset_s != offset_s {
            self.map.clear();
            (self.lang, self.today, self.offset_s) = (Some(lang), today, offset_s);
        }
        if self.map.len() >= Self::CAP {
            self.map.clear();
        }
        let minute = t.as_millis().div_euclid(60_000);
        let at = Timestamp::from_millis(minute * 60_000);
        Arc::clone(self.map.entry((kind, minute)).or_insert_with(|| {
            Arc::from(match kind {
                Kind::Clock => clock(at, offset_s),
                Kind::Head => head_time(lang, at, now, offset_s),
                Kind::Day => day_title(lang, at, offset_s),
                Kind::Full => full_time(lang, at, offset_s),
                Kind::List => list_time(lang, at, now, offset_s),
            })
        }))
    }
}

/// `t` as `kind` in the interface's language, from the cache kept in `ctx`.
pub fn cached(
    ctx: &egui::Context,
    kind: Kind,
    t: Timestamp,
    now: Timestamp,
    offset_s: i32,
) -> Arc<str> {
    let lang = crate::i18n::language();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Times>(egui::Id::unique("chat-times"))
            .get(lang, kind, t, now, offset_s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaun_core::stats::rollup::days_from_civil;

    const EN: Language = Language::English;
    const AR: Language = Language::Arabic;

    /// The page's fixed clock: Tuesday 17 November 2026, 19:30 local.
    fn at(y: i64, m: u32, d: u32, h: i64, min: i64) -> Timestamp {
        Timestamp::from_millis(days_from_civil(y, m, d) * DAY_MS + (h * 60 + min) * 60_000)
    }

    fn now() -> Timestamp {
        at(2026, 11, 17, 19, 30)
    }

    #[test]
    fn a_head_today_says_today_at_the_clock() {
        assert_eq!(
            head_time(EN, at(2026, 11, 17, 13, 59), now(), 0),
            "Today at 13:59"
        );
        assert_eq!(
            head_time(EN, at(2026, 11, 17, 0, 0), now(), 0),
            "Today at 00:00"
        );
    }

    #[test]
    fn a_head_yesterday_counts_calendar_days_not_24_hours() {
        // 23:50 the day before is "Yesterday" although under 24 hours have passed.
        assert_eq!(
            head_time(EN, at(2026, 11, 16, 23, 50), now(), 0),
            "Yesterday at 23:50"
        );
        assert_eq!(
            head_time(EN, at(2026, 11, 16, 0, 5), now(), 0),
            "Yesterday at 00:05"
        );
    }

    #[test]
    fn an_older_head_gives_the_numeric_date_with_leading_zeros() {
        assert_eq!(
            head_time(EN, at(2026, 11, 12, 21, 40), now(), 0),
            "12/11/2026 21:40"
        );
        assert_eq!(
            head_time(EN, at(2026, 11, 3, 22, 14), now(), 0),
            "03/11/2026 22:14"
        );
    }

    #[test]
    fn days_are_local_days_at_the_utc_offset() {
        // 23:30 UTC on the 16th is 01:30 on the 17th at UTC+2: today, not yesterday.
        let t = at(2026, 11, 16, 23, 30);
        assert_eq!(head_time(EN, t, now(), 2 * 3600), "Today at 01:30");
        assert_eq!(head_time(EN, t, now(), 0), "Yesterday at 23:30");
    }

    #[test]
    fn a_divider_always_carries_the_full_date() {
        assert_eq!(day_title(EN, now(), 0), "Tuesday, 17 November 2026");
        assert_eq!(
            day_title(EN, at(2026, 11, 16, 8, 0), 0),
            "Monday, 16 November 2026"
        );
    }

    #[test]
    fn the_tooltip_says_the_whole_date_and_time() {
        assert_eq!(
            full_time(EN, at(2026, 11, 17, 13, 59), 0),
            "Tuesday, 17 November 2026 at 13:59"
        );
        assert_eq!(
            full_time(EN, at(2026, 1, 4, 9, 5), 0),
            "Sunday, 4 January 2026 at 09:05"
        );
    }

    #[test]
    fn the_list_says_clock_yesterday_weekday_within_six_days_then_the_date() {
        let n = now();
        assert_eq!(list_time(EN, at(2026, 11, 17, 13, 59), n, 0), "13:59");
        assert_eq!(list_time(EN, at(2026, 11, 16, 13, 59), n, 0), "Yesterday");
        assert_eq!(list_time(EN, at(2026, 11, 15, 13, 59), n, 0), "Sunday");
        assert_eq!(list_time(EN, at(2026, 11, 11, 13, 59), n, 0), "Wednesday");
        // Seven days back is a date again: the weekday would repeat today's.
        assert_eq!(list_time(EN, at(2026, 11, 10, 13, 59), n, 0), "10/11/2026");
        assert_eq!(list_time(EN, at(2026, 11, 9, 13, 59), n, 0), "09/11/2026");
    }

    #[test]
    fn arabic_forms_keep_western_digits() {
        let n = now();
        assert_eq!(
            head_time(AR, at(2026, 11, 17, 13, 59), n, 0),
            "اليوم في 13:59"
        );
        assert_eq!(
            head_time(AR, at(2026, 11, 16, 13, 59), n, 0),
            "أمس في 13:59"
        );
        assert_eq!(
            head_time(AR, at(2026, 11, 12, 21, 40), n, 0),
            "12/11/2026 21:40"
        );
        assert_eq!(
            full_time(AR, at(2026, 11, 17, 13, 59), 0),
            "الثلاثاء، 17 نوفمبر 2026 في 13:59"
        );
        assert_eq!(day_title(AR, n, 0), "الثلاثاء، 17 نوفمبر 2026");
        assert_eq!(list_time(AR, at(2026, 11, 16, 13, 59), n, 0), "أمس");
        assert_eq!(list_time(AR, at(2026, 11, 15, 13, 59), n, 0), "الأحد");
    }

    #[test]
    fn the_cache_starts_over_on_a_new_day_or_language() {
        let mut times = Times::default();
        let t = at(2026, 11, 17, 13, 59);
        assert_eq!(&*times.get(EN, Kind::Head, t, now(), 0), "Today at 13:59");
        // Midnight passes: the same message is now yesterday's.
        let tomorrow = at(2026, 11, 18, 0, 1);
        assert_eq!(
            &*times.get(EN, Kind::Head, t, tomorrow, 0),
            "Yesterday at 13:59"
        );
        assert_eq!(&*times.get(AR, Kind::Head, t, tomorrow, 0), "أمس في 13:59");
        assert_eq!(times.map.len(), 1);
    }
}
