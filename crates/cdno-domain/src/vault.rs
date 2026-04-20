//! [`Vault`] — the domain-layer entry point that stitches a
//! [`VaultStore`], a [`VaultIndex`], and a [`VaultConfig`] into a
//! single object downstream crates depend on.
//!
//! The store and index are held as `Arc<dyn _>` trait objects. One
//! reason over monomorphisation: uniformity with `VaultTransaction`,
//! which already uses `Arc<dyn>`. See
//! `Projects/cuaderno/resources/decision-vault-generics-vs-dyn.md`
//! for the full rationale.
//!
//! Startup reconciliation runs inside [`Vault::new`] so any domain
//! method can assume the index reflects the filesystem on entry.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::hash::content_hash;
use cdno_core::index::{NoteEntry, VaultIndex};
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::reconcile::{ReconciliationReport, reconcile};
use cdno_core::store::VaultStore;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;

/// The heading used for the log subsection in a daily note.
const DAILY_LOGS_SECTION: &str = "Logs";

/// Domain entry point. Own the store, index, and config; hand out
/// transactions; expose high-level operations (`log_to_daily_note`,
/// and later `create_project`, `update_project_state`, …).
pub struct Vault {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    config: VaultConfig,
}

impl Vault {
    /// Construct a vault and run startup reconciliation. The returned
    /// [`ReconciliationReport`] lets callers surface scan counts or
    /// per-file issues without re-running the pass.
    pub fn new(
        store: Arc<dyn VaultStore>,
        index: Arc<dyn VaultIndex>,
        config: VaultConfig,
    ) -> Result<(Self, ReconciliationReport), DomainError> {
        let report = reconcile(&store, &index)?;
        Ok((
            Self {
                store,
                index,
                config,
            },
            report,
        ))
    }

    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Start an uncommitted transaction bound to this vault's store
    /// and index. Callers enqueue ops and commit via
    /// `VaultTransaction::commit`.
    fn transaction(&self) -> VaultTransaction {
        VaultTransaction::new(Arc::clone(&self.store), Arc::clone(&self.index))
    }

    /// Append a log entry to the daily note for the given moment.
    ///
    /// Creates the note with a minimal scaffold if it doesn't exist.
    /// For existing notes, inserts the line at the end of the `## Logs`
    /// section — so later manual additions under other headings stay
    /// where the author put them.
    ///
    /// Returns the vault-relative path of the daily note touched.
    pub fn log_to_daily_note(
        &self,
        at: NaiveDateTime,
        entry: &str,
    ) -> Result<VaultPath, DomainError> {
        let path = daily_note_path(at.date())?;
        let line = format_log_line(at.time(), entry);

        let new_content = if self.store.exists(&path)? {
            // File exists: parse, append into the Logs section, re-render.
            // Going through `MarkdownDocument` means a missing Logs
            // section surfaces as `ManipulationError::SectionNotFound`
            // rather than silently appending in the wrong place.
            let current = self.store.read_file(&path)?;
            let mut doc = MarkdownDocument::parse(current)?;
            doc.append_to_section(DAILY_LOGS_SECTION, &line)?;
            doc.render().to_owned()
        } else {
            // Fresh daily note: compose the scaffold with the first
            // log line already inside `## Logs`.
            scaffold_daily_note(at.date(), &line)
        };

        // Rebuild the index row from the new content so the committed
        // transaction leaves file + index in sync.
        let entry_meta = build_daily_note_entry(&path, &new_content)?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }
}

/// Vault-relative path for a daily note of the given date —
/// `journal/daily/YYYY-MM-DD.md`.
fn daily_note_path(date: NaiveDate) -> Result<VaultPath, DomainError> {
    let rel = format!("journal/daily/{}.md", date.format("%Y-%m-%d"));
    Ok(VaultPath::new(rel)?)
}

/// Render one log line in the canonical `- **HH:MM**: text` form.
/// Trailing newline means subsequent `append_to_section` calls stack
/// cleanly without introducing blank lines between entries.
fn format_log_line(time: NaiveTime, entry: &str) -> String {
    format!("- **{:02}:{:02}**: {}\n", time.hour(), time.minute(), entry,)
}

/// Minimal scaffold for a brand-new daily note, pre-populated with the
/// first log line inside `## Logs`. Format chosen to satisfy the
/// reconciliation contract (valid frontmatter with `type: daily`) and
/// give the `## Logs` section a stable home.
fn scaffold_daily_note(date: NaiveDate, first_log_line: &str) -> String {
    format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {heading}\n\n## {section}\n{first_log_line}",
        date = date.format("%Y-%m-%d"),
        heading = date.format("%A, %-d %B %Y"),
        section = DAILY_LOGS_SECTION,
    )
}

/// Build the [`NoteEntry`] row that should go into the index for a
/// freshly-written daily note. Timestamps use `SystemTime::now()` —
/// close enough to the post-write filesystem mtime for reconciliation
/// to treat the row as up-to-date on the next pass.
fn build_daily_note_entry(path: &VaultPath, content: &str) -> Result<NoteEntry, DomainError> {
    let (fm, _body) = Frontmatter::parse(content)?;
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    Ok(NoteEntry {
        path: path.clone(),
        note_type: "daily".to_owned(),
        title: None,
        content_hash: content_hash(content),
        mtime_ns: now_ns,
        size: content.len() as u64,
        frontmatter: fm.as_json(),
        indexed_at_ns: now_ns,
    })
}
