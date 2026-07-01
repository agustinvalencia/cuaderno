//! Tracking note frontmatter: typed view of `type: tracking` YAML
//! headers (design §5.7).
//!
//! A tracking note records one occurrence of an activity under a
//! stewardship — a gym session, a body-measurements snapshot, a swim.
//! The frontmatter is the time-series substrate (\"all gym sessions
//! between X and Y\"); the body holds the rich, activity-specific
//! detail (rep sheet, metrics table, prose notes) that's read on
//! demand rather than indexed.

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;

/// Parsed and validated frontmatter for a tracking note.
///
/// `stewardship` is the slug (matches the parent folder), `activity`
/// is the activity slug (`gym`, `body`, `swim`, or a user-defined
/// one), and `date` is when the activity happened.
///
/// `duration_min` and `routine` are optional activity-shape fields that a
/// template may include (e.g. the example gym/swim variants) or omit (the
/// generic default, a body-metrics variant). Optional here so a note that
/// omits them parses cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackingFrontmatter {
    pub stewardship: String,
    pub activity: String,
    pub date: NaiveDate,
    pub duration_min: Option<u32>,
    /// Raw wikilink string (e.g. `"[[stewardships/health/routines/upper-body-a]]"`)
    /// when the entry references a routine doc; `None` when absent.
    pub routine: Option<String>,
}

impl TryFrom<Frontmatter> for TrackingFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            stewardship: fm.require_field::<String>("stewardship")?,
            activity: fm.require_field::<String>("activity")?,
            date: fm.require_field::<NaiveDate>("date")?,
            duration_min: fm.optional_field::<u32>("duration_min")?,
            routine: fm.optional_field::<String>("routine")?,
        })
    }
}
