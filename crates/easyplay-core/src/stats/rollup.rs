//! Splitting a session across local calendar days (STATISTICS §4.3), and the `YYYYMMDD`
//! day numbers used by `playtime_daily`.

use jiff::civil::Date;

use crate::clock::Timestamp;
use crate::storage::PlaytimePiece;

/// Milliseconds per day.
pub const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 → `(year, month, day)` (H. Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `(year, month, day)` → days since 1970-01-01 (`days_from_civil`).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYYMMDD` of a day number; years outside `0..=9999` are clamped.
pub fn day_key(days: i64) -> u32 {
    let (y, m, d) = civil_from_days(days);
    let y = y.clamp(0, 9999) as u32;
    y * 10_000 + m * 100 + d
}

/// Day number of a `YYYYMMDD` key (no validation beyond arithmetic).
pub fn key_days(key: u32) -> i64 {
    days_from_civil(i64::from(key / 10_000), (key / 100) % 100, key % 100)
}

/// `YYYYMMDD` of a calendar date.
pub fn date_key(d: Date) -> u32 {
    let y = u32::try_from(d.year()).unwrap_or(0);
    y * 10_000 + u32::from(d.month().unsigned_abs()) * 100 + u32::from(d.day().unsigned_abs())
}

/// The calendar date of a `YYYYMMDD` key, if valid.
pub fn key_date(key: u32) -> Option<Date> {
    let y = i16::try_from(key / 10_000).ok()?;
    let m = i8::try_from((key / 100) % 100).ok()?;
    let d = i8::try_from(key % 100).ok()?;
    Date::new(y, m, d).ok()
}

/// The local day key of a wall-clock instant at a fixed UTC offset.
pub fn local_day(at: Timestamp, tz_offset_min: i16) -> u32 {
    let local = at
        .as_millis()
        .saturating_add(i64::from(tz_offset_min) * 60_000);
    day_key(local.div_euclid(DAY_MS))
}

/// Lay `duration_ms` of play time on the local timeline from `started_at + offset` and cut it
/// at local midnights. Pieces are chronological, none is zero, and they sum to
/// `duration_ms` exactly; a zero duration yields no pieces. The first piece has
/// `starts_here`.
pub fn split(started_at: Timestamp, duration_ms: u64, tz_offset_min: i16) -> Vec<PlaytimePiece> {
    let mut out = Vec::new();
    let mut t = started_at
        .as_millis()
        .saturating_add(i64::from(tz_offset_min) * 60_000);
    let mut left = duration_ms;
    let mut first = true;
    while left > 0 {
        let day = t.div_euclid(DAY_MS);
        let to_midnight = u64::try_from((day + 1) * DAY_MS - t).unwrap_or(1).max(1);
        let chunk = left.min(to_midnight);
        out.push(PlaytimePiece {
            day: day_key(day),
            ms: chunk,
            starts_here: first,
        });
        first = false;
        left -= chunk;
        t = t.saturating_add(i64::try_from(chunk).unwrap_or(i64::MAX));
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn civil_round_trip_and_known_dates() {
        assert_eq!(day_key(0), 19_700_101);
        assert_eq!(day_key(-1), 19_691_231);
        assert_eq!(day_key(20_404), 20_251_112);
        assert_eq!(key_days(20_000_301) - key_days(20_000_228), 2);
        for d in -800_000..800_000i64 {
            if d % 997 == 0 {
                let (y, m, dd) = civil_from_days(d);
                assert_eq!(days_from_civil(y, m, dd), d);
            }
        }
        let date = key_date(20_261_112).unwrap();
        assert_eq!(date_key(date), 20_261_112);
        assert_eq!(key_date(20_260_230), None);
    }

    #[test]
    fn split_examples() {
        // 23:30 UTC + 60 min at offset +60 → local 00:30 next day: one piece.
        let t = Timestamp::from_millis(DAY_MS - 30 * 60_000);
        assert_eq!(
            split(t, 3_600_000, 60),
            vec![PlaytimePiece {
                day: 19_700_102,
                ms: 3_600_000,
                starts_here: true
            }]
        );
        // Same at offset 0: split 30 + 30 min.
        let p = split(t, 3_600_000, 0);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].day, p[0].ms), (19_700_101, 1_800_000));
        assert_eq!(
            (p[1].day, p[1].ms, p[1].starts_here),
            (19_700_102, 1_800_000, false)
        );
        // Negative offset moves the start back a day.
        assert_eq!(
            split(Timestamp::from_millis(60_000), 1000, -120)[0].day,
            19_691_231
        );
        assert!(split(t, 0, 0).is_empty());
        // Exactly at midnight.
        let m = Timestamp::from_millis(DAY_MS);
        assert_eq!(split(m, DAY_MS as u64, 0).len(), 1);
        assert_eq!(split(m, DAY_MS as u64 + 1, 0).len(), 2);
    }

    proptest! {
        #[test]
        fn split_properties(
            start in -10_000_000_000_000i64..10_000_000_000_000i64,
            dur in 0u64..(10 * DAY_MS as u64),
            off in -840i16..=840i16,
        ) {
            let pieces = split(Timestamp::from_millis(start), dur, off);
            prop_assert_eq!(pieces.iter().map(|p| p.ms).sum::<u64>(), dur);
            prop_assert!(pieces.iter().all(|p| p.ms > 0));
            prop_assert!(pieces.windows(2).all(|w| key_days(w[1].day) == key_days(w[0].day) + 1));
            prop_assert_eq!(pieces.iter().filter(|p| p.starts_here).count(), usize::from(dur > 0));
            if let Some(f) = pieces.first() {
                prop_assert!(f.starts_here);
                prop_assert_eq!(f.day, local_day(Timestamp::from_millis(start), off));
            }
            // Every piece but the first starts at local midnight, every piece but the last
            // ends at one.
            for p in pieces.iter().take(pieces.len().saturating_sub(1)).skip(1) {
                prop_assert_eq!(p.ms, DAY_MS as u64);
            }
        }
    }
}
