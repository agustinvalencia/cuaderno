//! Periodic recurrence patterns used by stewardship periodic
//! commitments (design §5.6).
//!
//! The wire format is the human-readable phrase that appears on a
//! `- Title — <recurrence> — next: YYYY-MM-DD` line: `daily`,
//! `weekly`, `monthly`, `every N months`, `yearly`. Two passes through
//! the same vocabulary: [`Recurrence::from_str`] parses the line back
//! into the typed enum, [`Recurrence::fmt`] writes it.
//!
//! The arithmetic for "next occurrence" is calendar-aware: `Monthly`
//! and `EveryNMonths` clamp the day to the target month's length so
//! "monthly" on the 31st rolls through 30/29/28-day months without
//! ever returning an invalid date. `Yearly` clamps Feb-29 to Feb-28
//! on non-leap years for the same reason.

use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Duration, NaiveDate};

/// A periodic recurrence pattern. Closed set: design §5.6 lists only
/// these; adding a sixth variant (`every N weeks`, `quarterly`, …)
/// should be a conscious change, not silently inferred from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly,
    Monthly,
    /// `every N months` for N >= 2. N == 1 normalises to [`Monthly`]
    /// on parse so two equivalent inputs round-trip to one canonical
    /// form.
    EveryNMonths(u32),
    Yearly,
}

/// Error returned when a string does not parse as a [`Recurrence`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseRecurrenceError {
    #[error(
        "unknown recurrence pattern: {0} (expected: daily, weekly, monthly, every N months, yearly)"
    )]
    Unknown(String),
    #[error("invalid month count in '{0}': N must be a positive integer of at least 2")]
    InvalidMonths(String),
}

impl FromStr for Recurrence {
    type Err = ParseRecurrenceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "daily" => Ok(Recurrence::Daily),
            "weekly" => Ok(Recurrence::Weekly),
            "monthly" => Ok(Recurrence::Monthly),
            "yearly" => Ok(Recurrence::Yearly),
            other => parse_every_n_months(other)
                .ok_or_else(|| ParseRecurrenceError::Unknown(s.to_owned())),
        }
    }
}

/// Attempt to parse `every N months`. Returns `None` for any other
/// shape so the caller can fall through to `Unknown`. Validates `N`
/// and folds `every 1 months` into the canonical `Monthly`.
fn parse_every_n_months(s: &str) -> Option<Recurrence> {
    let rest = s.strip_prefix("every ")?.strip_suffix(" months")?;
    let n: u32 = rest.parse().ok()?;
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(Recurrence::Monthly);
    }
    Some(Recurrence::EveryNMonths(n))
}

impl fmt::Display for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recurrence::Daily => f.write_str("daily"),
            Recurrence::Weekly => f.write_str("weekly"),
            Recurrence::Monthly => f.write_str("monthly"),
            Recurrence::EveryNMonths(n) => write!(f, "every {n} months"),
            Recurrence::Yearly => f.write_str("yearly"),
        }
    }
}

impl Recurrence {
    /// Compute the next occurrence strictly after `from`. The result
    /// is `from + one_cycle`; advancing across multiple missed cycles
    /// is the caller's responsibility (loop and re-call).
    ///
    /// Calendar-aware: month and year additions clamp the day to the
    /// target month's length, so e.g. monthly-on-the-31st never
    /// returns an invalid date.
    pub fn next_after(self, from: NaiveDate) -> NaiveDate {
        match self {
            Recurrence::Daily => from + Duration::days(1),
            Recurrence::Weekly => from + Duration::weeks(1),
            Recurrence::Monthly => add_months(from, 1),
            Recurrence::EveryNMonths(n) => add_months(from, n),
            Recurrence::Yearly => add_months(from, 12),
        }
    }
}

/// Add `months` months to `date`, clamping the day to the resulting
/// month's length. Chrono's calendar helpers don't have a direct
/// "add N months with day-clamping" so this is hand-rolled; the
/// algorithm is the standard one (zero-based month arithmetic via
/// `i32`, then convert back).
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    let total_months = date.year() as i64 * 12 + (date.month0() as i64) + months as i64;
    let new_year = (total_months.div_euclid(12)) as i32;
    let new_month0 = total_months.rem_euclid(12) as u32;
    let new_month = new_month0 + 1;
    let last_day = last_day_of_month(new_year, new_month);
    let day = date.day().min(last_day);
    // Both the year and month0 come from valid arithmetic and the day
    // is clamped to the month's length — `from_ymd_opt` cannot return
    // `None` here in practice.
    NaiveDate::from_ymd_opt(new_year, new_month, day)
        .expect("day clamped to month length; year/month derived from arithmetic")
}

/// Last calendar day of `(year, month)`. Walks one day back from the
/// first day of the next month — handles Feb in leap years correctly
/// without a separate leap-year branch.
fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .expect("year, month derived from valid arithmetic");
    (first_of_next - Duration::days(1)).day()
}
