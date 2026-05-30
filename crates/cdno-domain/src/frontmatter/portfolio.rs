//! Portfolio frontmatter: typed view of `type: portfolio` YAML headers.
//!
//! Portfolios are the index of an evidence folder — per-question
//! dossiers that accumulate evidence notes over time (design §5.4).
//! The frontmatter is small on purpose: the body is the user's
//! synthesis, the evidence notes in the folder are the real content,
//! and most facets (note count, last-updated) are computed by the
//! index rather than stored here.

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;

/// Parsed and validated frontmatter for a portfolio `_index.md`.
///
/// `question` is the unifying question the portfolio collects evidence
/// against — the text shown in the body heading and on listings.
/// `project` is an optional wikilink string back to the project that
/// gave rise to this question (some portfolios outlive their birthing
/// project, hence optional).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioFrontmatter {
    pub question: String,
    pub created: NaiveDate,
    /// Raw wikilink string (e.g. `"[[projects/surrogate-model]]"`)
    /// when the portfolio was spawned by a project; `None` for
    /// portfolios that stand alone.
    pub project: Option<String>,
}

impl TryFrom<Frontmatter> for PortfolioFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            question: fm.require_field::<String>("question")?,
            created: fm.require_field::<NaiveDate>("created")?,
            project: fm.optional_field::<String>("project")?,
        })
    }
}
