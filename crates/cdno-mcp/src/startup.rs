//! Startup observability helpers.
//!
//! The server timestamps every log line, daily entry and tracking entry
//! with the process's **local** time (`chrono::Local::now()`). On a host
//! with no zoneinfo database and no `TZ`, chrono silently falls back to
//! UTC — which surfaced as a production incident (GH #309): remote daily
//! logs landed hours behind the wall clock, and the only way to spot it
//! was `docker exec ... date`. Logging the resolved offset at startup
//! turns that into a signal an operator can read straight from the boot
//! logs. We report the offset factually and let the operator judge — a
//! host legitimately in UTC is valid and must not raise a false warning.

use std::fmt::Display;

use chrono::{DateTime, TimeZone};

/// The resolved local UTC offset and a sample local timestamp, formatted
/// for the startup log.
pub struct LocalTimeReport {
    /// The local UTC offset, e.g. `+02:00` (or `+00:00` for a UTC
    /// fallback). Rendered via the offset's `Display`.
    pub offset: String,
    /// A sample `Local::now()` in RFC 3339, so the wall-clock time the
    /// process believes it is shows alongside the offset.
    pub sample_now: String,
}

/// Build a [`LocalTimeReport`] from a concrete timestamp. Generic over
/// the time zone so it can be unit-tested with a `FixedOffset` instead of
/// depending on the host's `TZ`.
pub fn local_time_report<Tz>(now: &DateTime<Tz>) -> LocalTimeReport
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    LocalTimeReport {
        offset: now.offset().to_string(),
        sample_now: now.to_rfc3339(),
    }
}

/// Log the process's resolved local timezone offset and a sample
/// `Local::now()` at `INFO`, next to the other startup lines. Purely
/// observability — see the module docs for the incident it guards.
pub fn log_local_time() {
    let report = local_time_report(&chrono::Local::now());
    tracing::info!(
        local_offset = %report.offset,
        sample_local_now = %report.sample_now,
        "local time zone resolved (server timestamps use process-local time)"
    );
}
