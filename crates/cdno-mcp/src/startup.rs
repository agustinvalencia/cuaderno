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
//!
//! [`log_method_notice`] does the same for a vault on a newer minor method
//! version than this binary (RFC 0006 §8.4): writes proceed, so the start-up
//! log is where an operator learns that cdno is behind the vault.

use std::fmt::Display;

use cdno_domain::Vault;
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

/// Log the vault's newer-minor method-version notice at `WARN`, once per
/// process ([`Vault::take_method_notice`]). Through `tracing`, so to
/// stderr: stdout is the JSON-RPC channel. Nothing is logged when the vault
/// is on this binary's version, an older one, or unstamped.
pub fn log_method_notice(vault: &Vault) {
    if let Some(status) = vault.take_method_notice() {
        let vault_version = status
            .vault
            .map_or_else(|| "unstamped".to_owned(), |v| v.to_string());
        tracing::warn!(
            vault_method_version = %vault_version,
            binary_method_version = %status.binary,
            "this vault follows a newer minor version of the Research Logbook Method than this \
             cdno implements; writes proceed, upgrade cdno for the practices it adds"
        );
    }
}
