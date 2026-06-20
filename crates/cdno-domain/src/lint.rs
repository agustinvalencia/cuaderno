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

/// How serious a lint issue is.
///
/// `Error` is a hard validation failure that downstream code can trip
/// over (unknown note type, missing required field, a frozen archived
/// note that was edited). `Warning` is a non-fatal problem worth
/// surfacing but not structurally breaking — a dangling wikilink is
/// the canonical case: the note parses fine, a link just points
/// nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LintSeverity {
    #[default]
    Error,
    Warning,
}

impl LintSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            LintSeverity::Error => "error",
            LintSeverity::Warning => "warning",
        }
    }
}

/// A single problem found at a given note path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub path: VaultPath,
    pub severity: LintSeverity,
    pub message: String,
}

impl LintIssue {
    /// A hard validation failure.
    pub fn error(path: VaultPath, message: impl Into<String>) -> Self {
        Self {
            path,
            severity: LintSeverity::Error,
            message: message.into(),
        }
    }

    /// A non-fatal problem (e.g. a dangling wikilink).
    pub fn warning(path: VaultPath, message: impl Into<String>) -> Self {
        Self {
            path,
            severity: LintSeverity::Warning,
            message: message.into(),
        }
    }
}
