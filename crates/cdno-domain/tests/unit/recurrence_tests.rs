//! Unit tests for `Recurrence` parsing, display, and arithmetic.

use cdno_domain::recurrence::{ParseRecurrenceError, Recurrence};
use chrono::NaiveDate;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

// ---------------------------------------------------------------------
// FromStr
// ---------------------------------------------------------------------

#[test]
fn parse_each_canonical_variant() {
    assert_eq!("daily".parse::<Recurrence>().unwrap(), Recurrence::Daily);
    assert_eq!("weekly".parse::<Recurrence>().unwrap(), Recurrence::Weekly);
    assert_eq!(
        "monthly".parse::<Recurrence>().unwrap(),
        Recurrence::Monthly
    );
    assert_eq!("yearly".parse::<Recurrence>().unwrap(), Recurrence::Yearly);
    assert_eq!(
        "every 6 months".parse::<Recurrence>().unwrap(),
        Recurrence::EveryNMonths(6)
    );
    assert_eq!(
        "every 3 months".parse::<Recurrence>().unwrap(),
        Recurrence::EveryNMonths(3)
    );
}

#[test]
fn parse_is_case_insensitive_and_trims_whitespace() {
    assert_eq!(
        "  WEEKLY  ".parse::<Recurrence>().unwrap(),
        Recurrence::Weekly
    );
    assert_eq!(
        "EVERY 2 MONTHS".parse::<Recurrence>().unwrap(),
        Recurrence::EveryNMonths(2)
    );
}

#[test]
fn parse_folds_every_1_months_into_monthly_canonical_form() {
    assert_eq!(
        "every 1 months".parse::<Recurrence>().unwrap(),
        Recurrence::Monthly
    );
}

#[test]
fn parse_rejects_unknown_phrases() {
    let err = "fortnightly".parse::<Recurrence>().unwrap_err();
    assert!(matches!(err, ParseRecurrenceError::Unknown(s) if s == "fortnightly"));
}

#[test]
fn parse_rejects_zero_or_garbage_n() {
    assert!(matches!(
        "every 0 months".parse::<Recurrence>().unwrap_err(),
        ParseRecurrenceError::Unknown(_)
    ));
    assert!(matches!(
        "every abc months".parse::<Recurrence>().unwrap_err(),
        ParseRecurrenceError::Unknown(_)
    ));
}

// ---------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------

#[test]
fn display_round_trips_canonical_phrases() {
    for r in [
        Recurrence::Daily,
        Recurrence::Weekly,
        Recurrence::Monthly,
        Recurrence::EveryNMonths(2),
        Recurrence::EveryNMonths(6),
        Recurrence::Yearly,
    ] {
        let s = r.to_string();
        let parsed: Recurrence = s.parse().unwrap();
        assert_eq!(parsed, r, "round-trip failed for {s}");
    }
}

#[test]
fn display_strings_match_wire_format() {
    assert_eq!(Recurrence::Daily.to_string(), "daily");
    assert_eq!(Recurrence::EveryNMonths(6).to_string(), "every 6 months");
}

// ---------------------------------------------------------------------
// next_after
// ---------------------------------------------------------------------

#[test]
fn next_after_daily_adds_one_day() {
    assert_eq!(Recurrence::Daily.next_after(d(2026, 5, 30)), d(2026, 5, 31));
    // crosses month boundary
    assert_eq!(Recurrence::Daily.next_after(d(2026, 5, 31)), d(2026, 6, 1));
}

#[test]
fn next_after_weekly_adds_seven_days() {
    assert_eq!(
        Recurrence::Weekly.next_after(d(2026, 1, 10)),
        d(2026, 1, 17)
    );
}

#[test]
fn next_after_monthly_basic() {
    assert_eq!(
        Recurrence::Monthly.next_after(d(2026, 1, 15)),
        d(2026, 2, 15)
    );
    // crosses year boundary
    assert_eq!(
        Recurrence::Monthly.next_after(d(2026, 12, 5)),
        d(2027, 1, 5)
    );
}

#[test]
fn next_after_monthly_clamps_day_to_short_month() {
    // 31 Jan -> 28 Feb (clamped); not invalid 31 Feb.
    assert_eq!(
        Recurrence::Monthly.next_after(d(2026, 1, 31)),
        d(2026, 2, 28)
    );
    // 31 Mar -> 30 Apr (clamped).
    assert_eq!(
        Recurrence::Monthly.next_after(d(2026, 3, 31)),
        d(2026, 4, 30)
    );
}

#[test]
fn next_after_every_n_months_spans_year() {
    assert_eq!(
        Recurrence::EveryNMonths(6).next_after(d(2026, 9, 10)),
        d(2027, 3, 10)
    );
    assert_eq!(
        Recurrence::EveryNMonths(3).next_after(d(2026, 11, 15)),
        d(2027, 2, 15)
    );
}

#[test]
fn next_after_yearly_basic_and_leap_clamp() {
    assert_eq!(Recurrence::Yearly.next_after(d(2026, 5, 1)), d(2027, 5, 1));
    // Feb 29 on a leap year (2028) -> Feb 28 in 2029 (non-leap).
    assert_eq!(
        Recurrence::Yearly.next_after(d(2028, 2, 29)),
        d(2029, 2, 28)
    );
}
