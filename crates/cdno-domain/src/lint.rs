//! Vault-wide validation report.
//!
//! [`Vault::lint_all_notes`] walks every indexed note and produces a
//! [`LintReport`] summarising frontmatter problems. Lint is read-only:
//! it never mutates the vault. The CLI surfaces the report through
//! `cdno lint`; the MCP server and Tauri app expose the same report
//! through their respective handlers.

use cdno_core::path::VaultPath;

/// Result of a lint pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LintReport {
    pub issues: Vec<LintIssue>,
}

impl LintReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// A single problem found at a given note path.
///
/// Severity is intentionally absent for now — every issue we currently
/// emit is a hard validation failure (unknown note type, required
/// field missing). When the lint surface grows warnings (e.g. broken
/// wikilinks once #84 lands), add a `LintSeverity` enum here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub path: VaultPath,
    pub message: String,
}
