//! The ship's calendar: turning "seconds since the game began" into a date
//! and time (decision D32).
//!
//! The game clock ([`crate::World::seconds`]) counts seconds from 0. Every
//! game begins at [`SHIP_START`], 08:42:00 on 25 November 2631, so the
//! date and time on screen is that moment plus the seconds that have passed.
//!
//! Dates use the Gregorian calendar (the one in everyday use), including its
//! leap-year rule: every 4th year, except every 100th, except every 400th.
//! The conversions are Howard Hinnant's well-known `days_from_civil` and
//! `civil_from_days` algorithms: pure whole-number maths, so they're exact,
//! fast and the same on every machine.

use std::fmt;

/// A moment on the ship's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    /// 1 = January … 12 = December.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// The moment every game begins.
pub const SHIP_START: DateTime = DateTime {
    year: 2631,
    month: 11,
    day: 25,
    hour: 8,
    minute: 42,
    second: 0,
};

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl DateTime {
    /// This moment `seconds` later.
    pub fn plus_seconds(self, seconds: u64) -> DateTime {
        let seconds = i64::try_from(seconds).expect("game time fits in i64 seconds");
        let start = days_from_civil(self.year, self.month, self.day) * SECONDS_PER_DAY
            + i64::from(self.hour * 3600 + self.minute * 60 + self.second);
        let total = start + seconds;
        let (days, second_of_day) = (
            total.div_euclid(SECONDS_PER_DAY),
            total.rem_euclid(SECONDS_PER_DAY),
        );
        let (year, month, day) = civil_from_days(days);
        // `second_of_day` is 0..86400, so these all fit in a u32.
        let s = u32::try_from(second_of_day).expect("a second of the day fits in u32");
        DateTime {
            year,
            month,
            day,
            hour: s / 3600,
            minute: s / 60 % 60,
            second: s % 60,
        }
    }

    /// The date, like `25 Nov 2631`.
    pub fn date_text(self) -> String {
        format!(
            "{} {} {}",
            self.day,
            MONTH_NAMES[(self.month - 1) as usize],
            self.year
        )
    }

    /// The time of day on a 24-hour clock, like `08:42:00`.
    pub fn time_text(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.date_text(), self.time_text())
    }
}

/// Days from 1 January 1970 to the given date (negative before it).
///
/// The trick: treat March as the first month of the year, so the leap day
/// (29 February) falls at the very end, then count whole 400-year "eras",
/// which always hold exactly 146,097 days.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400; // 0..=399
    let month_from_march = (month + 9) % 12; // March = 0 … February = 11
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1; // 0..=365
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468 // 719,468 days from 0000-03-01 to 1970-01-01
}

/// The date that is `days` days after 1 January 1970: the exact reverse of
/// [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097; // 0..=146096
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // `day` is 1..=31 and `month` 1..=12 by construction.
    (
        year,
        u32::try_from(month).expect("month 1..=12"),
        u32::try_from(day).expect("day 1..=31"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime {
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn the_game_begins_at_0842_on_25_november_2631() {
        assert_eq!(SHIP_START.to_string(), "25 Nov 2631 08:42:00");
        assert_eq!(SHIP_START.plus_seconds(0), SHIP_START);
    }

    #[test]
    fn seconds_minutes_and_hours_roll_over() {
        assert_eq!(SHIP_START.plus_seconds(59), at(2631, 11, 25, 8, 42, 59));
        assert_eq!(SHIP_START.plus_seconds(60), at(2631, 11, 25, 8, 43, 0));
        assert_eq!(SHIP_START.plus_seconds(18 * 60), at(2631, 11, 25, 9, 0, 0));
    }

    #[test]
    fn midnight_starts_the_next_day() {
        // 15 h 18 min after 08:42 is midnight.
        let midnight = SHIP_START.plus_seconds(15 * 3600 + 18 * 60);
        assert_eq!(midnight, at(2631, 11, 26, 0, 0, 0));
        assert_eq!(midnight.to_string(), "26 Nov 2631 00:00:00");
    }

    #[test]
    fn months_and_years_roll_over() {
        let day = 86_400;
        let nov_30 = at(2631, 11, 30, 12, 0, 0);
        assert_eq!(nov_30.plus_seconds(day), at(2631, 12, 1, 12, 0, 0));
        let dec_31 = at(2631, 12, 31, 23, 59, 59);
        assert_eq!(dec_31.plus_seconds(1), at(2632, 1, 1, 0, 0, 0));
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        let day = 86_400;
        // 2632 is divisible by 4: a leap year, so 29 Feb exists.
        assert_eq!(
            at(2632, 2, 28, 0, 0, 0).plus_seconds(day),
            at(2632, 2, 29, 0, 0, 0)
        );
        // 2700 is divisible by 100 but not 400: not a leap year.
        assert_eq!(
            at(2700, 2, 28, 0, 0, 0).plus_seconds(day),
            at(2700, 3, 1, 0, 0, 0)
        );
        // 2800 is divisible by 400: a leap year again.
        assert_eq!(
            at(2800, 2, 28, 0, 0, 0).plus_seconds(day),
            at(2800, 2, 29, 0, 0, 0)
        );
    }

    #[test]
    fn day_counts_match_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn day_counts_and_dates_convert_back_and_forth() {
        // Every day across four centuries, including the ship's era.
        let start = days_from_civil(2500, 1, 1);
        let end = days_from_civil(2900, 1, 1);
        for days in start..end {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn date_and_time_text() {
        let t = at(2631, 3, 7, 5, 4, 3);
        assert_eq!(t.date_text(), "7 Mar 2631");
        assert_eq!(t.time_text(), "05:04:03");
    }

    #[test]
    fn a_long_game_still_counts_correctly() {
        // A year of game time later (2632 is a leap year: 366 days).
        let year = 366 * 86_400;
        assert_eq!(SHIP_START.plus_seconds(year), at(2632, 11, 25, 8, 42, 0));
    }
}
