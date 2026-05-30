//! Stewardship dashboard frontmatter: typed view of `type: stewardship`
//! YAML headers (design §5.6).
//!
//! Stewardships represent perpetual responsibilities (finances,
//! health, household, …). The frontmatter is deliberately tiny — just
//! the life context — because the value lives in the structured body
//! sections (Current Status, Periodic Commitments, Active Habits,
//! Notes) and the per-stewardship tracking notes that link back via
//! the `stewardship:` field on a `tracking` note.

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;

use super::context::Context;

/// Parsed and validated frontmatter for a stewardship dashboard.
/// Holds for both the flat (`stewardships/<slug>.md`) and expanded
/// (`stewardships/<slug>/_index.md`) variants — the shape is the same;
/// only the on-disk layout differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StewardshipFrontmatter {
    pub context: Context,
}

impl TryFrom<Frontmatter> for StewardshipFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            context: fm.require_field::<Context>("context")?,
        })
    }
}
