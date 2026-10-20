//! Reading a project map's `held_by:` (RFC 0006 §7.2, §8.2).
//!
//! `held_by` names whoever else is moving the map; absent means the person.
//! The rule is that the verb refuses and the parser never does: a value that
//! is not a plain name, or that names the person or nobody, reads as no
//! holder, so the map counts toward the cap and nothing that reads project
//! maps fails over it.
//!
//! [`classify_holder_text`] is the one deny-list function. It serves the
//! frontmatter reader (through [`holder_reading_from_yaml`]), lint (through
//! [`holder_reading_from_json`], over the index's
//! [`Frontmatter::as_json`](cdno_core::frontmatter::Frontmatter::as_json)
//! blob) and the hand-off verb, all reading the same configured list,
//! `[projects] holder_sentinels`.
//!
//! The two adapters agree on every value `as_json` can render. The one known
//! disagreement: a YAML mapping keyed by a sequence has no JSON form, so
//! `as_json` writes `null` for the present key, which the JSON adapter reads
//! as `Absent` where the YAML adapter reads `NotAString`. The map counts
//! either way; only lint's error for that value is lost.

use cdno_core::config::VaultConfig;
use chrono::{DateTime, NaiveDate, NaiveDateTime};

/// What a map's `held_by:` says, once read.
///
/// Only [`Named`](Self::Named) takes the map out of the count; every other
/// reading means the map is the person's. Match it exhaustively: a new
/// reading must be placed on one side of that line on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderReading {
    /// No `held_by:` key, or `held_by: null`.
    Absent,
    /// A plain name of someone else, trimmed.
    Named(String),
    /// A value that is not a plain string: an unquoted `[[people/ana]]`
    /// (a nested YAML sequence), a number, a bool, a mapping, or a date.
    NotAString,
    /// A value that names you or nobody: one on the configured list (`me`,
    /// `tbd`, `?`, …), or a blank string under any list, as written.
    Sentinel(String),
}

/// The configured `[projects] holder_sentinels`, normalised for comparison:
/// trimmed and lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolderSentinels {
    entries: Vec<String>,
}

impl HolderSentinels {
    /// A list from raw entries, as config spells them.
    pub fn new<I, S>(entries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            entries: entries.into_iter().map(|e| normalise(e.as_ref())).collect(),
        }
    }

    /// The vault's configured list, `[projects] holder_sentinels`.
    pub fn from_config(config: &VaultConfig) -> Self {
        Self::new(&config.projects.holder_sentinels)
    }

    fn contains(&self, normalised: &str) -> bool {
        self.entries.iter().any(|e| e == normalised)
    }
}

fn normalise(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Classify `held_by` text against the configured list.
///
/// - A value on the list, or a blank value under any list (a blank names
///   nobody), reads as [`Sentinel`](HolderReading::Sentinel): it names you or
///   nobody.
/// - A date or timestamp (`2026-10-20`, quoted or not) reads as
///   [`NotAString`](HolderReading::NotAString): YAML would write it
///   unquoted, the parser hands it over as text either way, and it names no
///   one.
/// - Anything else is [`Named`](HolderReading::Named), trimmed.
pub fn classify_holder_text(text: &str, sentinels: &HolderSentinels) -> HolderReading {
    let trimmed = text.trim();
    if is_yaml_timestamp(trimmed) {
        return HolderReading::NotAString;
    }
    if trimmed.is_empty() || sentinels.contains(&normalise(trimmed)) {
        return HolderReading::Sentinel(text.to_owned());
    }
    HolderReading::Named(trimmed.to_owned())
}

/// Read `held_by` from the parsed YAML frontmatter value (`None` when the
/// key is absent).
pub fn holder_reading_from_yaml(
    value: Option<&serde_yaml::Value>,
    sentinels: &HolderSentinels,
) -> HolderReading {
    match value {
        None | Some(serde_yaml::Value::Null) => HolderReading::Absent,
        Some(serde_yaml::Value::String(text)) => classify_holder_text(text, sentinels),
        Some(_) => HolderReading::NotAString,
    }
}

/// Read `held_by` from the index's JSON frontmatter (`None` when the key is
/// absent). Agrees with [`holder_reading_from_yaml`] on every value
/// [`Frontmatter::as_json`](cdno_core::frontmatter::Frontmatter::as_json)
/// can render (see the module docs for the one it cannot).
pub fn holder_reading_from_json(
    value: Option<&serde_json::Value>,
    sentinels: &HolderSentinels,
) -> HolderReading {
    match value {
        None | Some(serde_json::Value::Null) => HolderReading::Absent,
        Some(serde_json::Value::String(text)) => classify_holder_text(text, sentinels),
        Some(_) => HolderReading::NotAString,
    }
}

/// Whether `text` is a YAML 1.1 date or timestamp, which YAML hands over as
/// text and which names no one.
fn is_yaml_timestamp(text: &str) -> bool {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
        || DateTime::parse_from_rfc3339(text).is_ok()
        || NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
        || NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f").is_ok()
}
