//! Evidence frontmatter: typed view of `type: evidence` YAML headers.
//!
//! Evidence is the raw material a portfolio accumulates — a paper,
//! an experiment result, a conversation note (design §5.5). The
//! frontmatter stays minimal; the body is free-form prose. Two fields
//! are non-obvious:
//!
//! - `portfolio` is technically redundant with the file path (every
//!   evidence note lives inside `portfolios/<slug>/`) but kept so
//!   orphan detection works if a note gets moved out of its folder.
//! - `origin` is **required from Phase 3 onward**. It points to
//!   whatever produced the evidence — a project, an action note, a
//!   stewardship. The forward link gives provenance ("which work
//!   produced this?"); the backlink falls out of the wikilink index,
//!   so actions and projects can list their evidence without
//!   duplicating any structural data. Baked in from day one of the
//!   knowledge layer so the action layer (§5.11) can lean on it
//!   without a migration.

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;

/// Parsed and validated frontmatter for an evidence note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceFrontmatter {
    pub created: NaiveDate,
    /// Free-form: citation, experiment id, conversation reference, or
    /// `"personal observation"`. Used as the basis of the filename slug.
    pub source: String,
    /// Slug of the parent portfolio (the immediate-parent directory).
    /// Redundant with the path but kept for orphan detection.
    pub portfolio: String,
    /// Raw wikilink string pointing at whatever produced this
    /// evidence — a project, action note, stewardship, etc. Required.
    pub origin: String,
    /// Media kind for an attachment stub (`pdf`/`image`/`video`/
    /// `typst`/…, #154). `None` for a plain prose evidence note — the
    /// field is absent unless the note references a non-markdown
    /// artefact in a sibling folder.
    pub kind: Option<String>,
}

impl TryFrom<Frontmatter> for EvidenceFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            created: fm.require_field::<NaiveDate>("created")?,
            source: fm.require_field::<String>("source")?,
            portfolio: fm.require_field::<String>("portfolio")?,
            origin: fm.require_field::<String>("origin")?,
            kind: fm.optional_field::<String>("kind")?,
        })
    }
}
