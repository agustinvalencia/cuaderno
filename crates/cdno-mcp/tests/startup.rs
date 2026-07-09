//! Unit tests for the startup local-time report (GH #309).

use chrono::{FixedOffset, TimeZone};

use cdno_mcp::startup::local_time_report;

#[test]
fn reports_the_offset_and_a_sample_timestamp() {
    // CEST — the offset the incident vault should have resolved to.
    let tz = FixedOffset::east_opt(2 * 3600).unwrap();
    let now = tz.with_ymd_and_hms(2026, 7, 6, 14, 30, 0).unwrap();

    let report = local_time_report(&now);

    assert_eq!(report.offset, "+02:00");
    assert_eq!(report.sample_now, "2026-07-06T14:30:00+02:00");
}

#[test]
fn utc_fallback_reports_a_zero_offset() {
    // The silent fallback the log is meant to make visible: +00:00.
    let tz = FixedOffset::east_opt(0).unwrap();
    let now = tz.with_ymd_and_hms(2026, 7, 6, 12, 30, 0).unwrap();

    let report = local_time_report(&now);

    assert_eq!(report.offset, "+00:00");
    assert_eq!(report.sample_now, "2026-07-06T12:30:00+00:00");
}
