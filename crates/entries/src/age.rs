//! Compact relative-age labels for sidebar rows ("now", "5m", "3h", "5d",
//! "2w", "8mo", "1y").
//!
//! Everything here is a pure function of `(touched_at, now)` in unix
//! milliseconds, so tests can pin both sides. The view computes labels
//! exactly once per `set_entries` — the sidebar never runs timers to
//! refresh itself.

const SECOND: i64 = 1000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;
/// A calendar-ish month for label purposes.
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;

/// The age label for an entry touched at `touched_at`, as of `now` (both
/// unix milliseconds). Future or corrupt timestamps clamp to "now" — a bad
/// `touched_at` must never take the sidebar down.
pub(crate) fn age_label(touched_at: i64, now: i64) -> String {
    let elapsed = now.saturating_sub(touched_at).max(0);
    if elapsed < MINUTE {
        "now".to_owned()
    } else if elapsed < HOUR {
        format!("{}m", elapsed / MINUTE)
    } else if elapsed < DAY {
        format!("{}h", elapsed / HOUR)
    } else if elapsed < WEEK {
        format!("{}d", elapsed / DAY)
    } else if elapsed < MONTH {
        format!("{}w", elapsed / WEEK)
    } else if elapsed < YEAR {
        format!("{}mo", elapsed / MONTH)
    } else {
        format!("{}y", elapsed / YEAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed "now" for every case: 2026-06-10 12:00:00 UTC.
    const NOW: i64 = 1_781_179_200_000;

    #[test]
    fn under_a_minute_is_now() {
        assert_eq!(age_label(NOW, NOW), "now");
        assert_eq!(age_label(NOW - 59 * SECOND, NOW), "now");
    }

    #[test]
    fn minutes_under_an_hour() {
        assert_eq!(age_label(NOW - MINUTE, NOW), "1m");
        assert_eq!(age_label(NOW - 5 * MINUTE - 30 * SECOND, NOW), "5m");
        assert_eq!(age_label(NOW - 59 * MINUTE, NOW), "59m");
    }

    #[test]
    fn hours_under_a_day() {
        assert_eq!(age_label(NOW - HOUR, NOW), "1h");
        assert_eq!(age_label(NOW - 23 * HOUR + MINUTE, NOW), "22h");
        assert_eq!(age_label(NOW - 23 * HOUR, NOW), "23h");
    }

    #[test]
    fn days_under_a_week() {
        assert_eq!(age_label(NOW - DAY, NOW), "1d");
        assert_eq!(age_label(NOW - 5 * DAY, NOW), "5d");
        assert_eq!(age_label(NOW - 6 * DAY - 23 * HOUR, NOW), "6d");
    }

    #[test]
    fn weeks_under_a_month() {
        assert_eq!(age_label(NOW - WEEK, NOW), "1w");
        assert_eq!(age_label(NOW - 29 * DAY, NOW), "4w");
    }

    #[test]
    fn months_under_a_year() {
        assert_eq!(age_label(NOW - MONTH, NOW), "1mo");
        assert_eq!(age_label(NOW - 8 * MONTH, NOW), "8mo");
        assert_eq!(age_label(NOW - 364 * DAY, NOW), "12mo");
    }

    #[test]
    fn years_beyond() {
        assert_eq!(age_label(NOW - YEAR, NOW), "1y");
        assert_eq!(age_label(NOW - 3 * YEAR - 100 * DAY, NOW), "3y");
    }

    #[test]
    fn future_and_extreme_timestamps_clamp_to_now() {
        assert_eq!(age_label(NOW + HOUR, NOW), "now");
        assert_eq!(age_label(i64::MAX, NOW), "now");
        // i64::MIN must not overflow the subtraction; the elapsed time
        // saturates and still formats as (an absurd number of) years.
        assert_eq!(age_label(i64::MIN, NOW), format!("{}y", i64::MAX / YEAR));
    }

    #[test]
    fn boundaries_roll_to_the_next_unit() {
        assert_eq!(age_label(NOW - 60 * SECOND, NOW), "1m");
        assert_eq!(age_label(NOW - 60 * MINUTE, NOW), "1h");
        assert_eq!(age_label(NOW - 24 * HOUR, NOW), "1d");
        assert_eq!(age_label(NOW - 7 * DAY, NOW), "1w");
        assert_eq!(age_label(NOW - 30 * DAY, NOW), "1mo");
        assert_eq!(age_label(NOW - 365 * DAY, NOW), "1y");
    }
}
