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

use std::str::FromStr;
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
use crate::lint::{LintIssue, LintReport};
use crate::note_type::NoteType;

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

    /// Validate every indexed note and return a structured report.
    ///
    /// The pass is read-only and skips any file that's not in the
    /// index — non-markdown attachments (PDFs, notebooks) and any
    /// file under `.cuaderno/` are by definition not present, so the
    /// scope of `lint` is exactly "the notes the index knows about".
    ///
    /// Today's checks:
    /// - the entry's `type` field parses as a known [`NoteType`];
    /// - every field listed in the type's `[schemas.<type>]
    ///   extra_required` config section is present in the
    ///   frontmatter.
    ///
    /// Per-type structural checks (e.g. `ProjectFrontmatter` invariants)
    /// land alongside their domain code in Phase 2/3.
    pub fn lint_all_notes(&self) -> Result<LintReport, DomainError> {
        let mut issues: Vec<LintIssue> = Vec::new();

        for path in self.index.list_all_paths()? {
            // A concurrent writer could remove a note between the
            // listing and the lookup. Treat that as "nothing to lint
            // here" rather than a hard error — the next pass will see
            // a coherent state.
            let Some(entry) = self.index.find_by_path(&path)? else {
                continue;
            };

            // The reconciler enforces that every indexed note has a
            // `type` field, but it does not constrain the value to a
            // known variant. An unknown type means downstream code
            // can't pick a schema, so don't bother checking
            // `extra_required` — the report would just compound a
            // problem the user already needs to fix.
            if NoteType::from_str(&entry.note_type).is_err() {
                issues.push(LintIssue {
                    path,
                    message: format!("unknown note type: `{}`", entry.note_type),
                });
                continue;
            }

            for required in self.config.extra_required_fields(&entry.note_type) {
                let present = entry
                    .frontmatter
                    .as_object()
                    .and_then(|obj| obj.get(required))
                    .is_some_and(|v| !v.is_null());
                if !present {
                    issues.push(LintIssue {
                        path: path.clone(),
                        message: format!(
                            "missing required field `{required}` for note type `{}`",
                            entry.note_type
                        ),
                    });
                }
            }
        }

        Ok(LintReport { issues })
    }

    /// Capture a quick note into `inbox/`. Returns the vault-relative
    /// path of the new file.
    ///
    /// Filename layout: `inbox/<YYYY-MM-DD>-<slug>.md`, where the slug
    /// is derived from the first ~6 words of the text. If the slug
    /// would be empty (the text is whitespace or punctuation only), it
    /// falls back to `untitled`. Filename collisions on the same day
    /// — same date plus same leading words — get a `-N` counter
    /// suffix; so `2026-04-26-buy-groceries.md`,
    /// `2026-04-26-buy-groceries-2.md`, and so on.
    ///
    /// The body of the file is the captured text trimmed of leading
    /// and trailing whitespace, with minimal frontmatter
    /// (`type: inbox`, `created: <ISO>`).
    pub fn capture_to_inbox(
        &self,
        at: NaiveDateTime,
        text: &str,
    ) -> Result<VaultPath, DomainError> {
        let path = self.next_inbox_path(at.date(), text)?;
        let content = scaffold_inbox_note(at, text);

        let entry_meta = build_index_entry_for(&path, &content, "inbox")?;
        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        tx.commit()?;
        Ok(path)
    }

    /// Resolve an unused inbox filename for `(date, text)`. Walks
    /// `-2`, `-3`, ... suffixes if needed, capped at a safety limit
    /// to avoid an infinite loop on a misbehaving store.
    fn next_inbox_path(&self, date: NaiveDate, text: &str) -> Result<VaultPath, DomainError> {
        let slug = slugify(text);
        let base = format!(
            "{}/{}-{}",
            cdno_core::paths::INBOX,
            date.format("%Y-%m-%d"),
            slug
        );
        let first = VaultPath::new(format!("{base}.md"))?;
        if !self.store.exists(&first)? {
            return Ok(first);
        }
        for n in 2..1000 {
            let candidate = VaultPath::new(format!("{base}-{n}.md"))?;
            if !self.store.exists(&candidate)? {
                return Ok(candidate);
            }
        }
        Err(DomainError::Store(
            cdno_core::error::StoreError::AlreadyExists(base),
        ))
    }
}

/// Vault-relative path for a daily note of the given date —
/// `journal/<year>/daily/YYYY-MM-DD.md`.
fn daily_note_path(date: NaiveDate) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(cdno_core::paths::daily_note_relpath(date))?)
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
    build_index_entry_for(path, content, "daily")
}

/// Generic version of [`build_daily_note_entry`] for note types that
/// don't carry a parsed title. Reads the frontmatter for the JSON
/// projection but doesn't require any field beyond what the caller
/// supplies; the `note_type` argument wins over whatever's in the
/// document so callers can be sure of the indexed type.
fn build_index_entry_for(
    path: &VaultPath,
    content: &str,
    note_type: &str,
) -> Result<NoteEntry, DomainError> {
    let (fm, _body) = Frontmatter::parse(content)?;
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    Ok(NoteEntry {
        path: path.clone(),
        note_type: note_type.to_owned(),
        title: None,
        content_hash: content_hash(content),
        mtime_ns: now_ns,
        size: content.len() as u64,
        frontmatter: fm.as_json(),
        indexed_at_ns: now_ns,
    })
}

/// Render the canonical inbox note for `at` carrying `text`.
fn scaffold_inbox_note(at: NaiveDateTime, text: &str) -> String {
    format!(
        "---\ntype: inbox\ncreated: {created}\n---\n\n{body}\n",
        created = at.format("%Y-%m-%dT%H:%M:%S"),
        body = text.trim(),
    )
}

/// Build a slug from the first words of `text`:
/// lowercase, alphanumerics joined by `-`, capped to a sensible
/// length so the filename stays manageable. Returns `"untitled"` if
/// the text contains no alphanumerics.
fn slugify(text: &str) -> String {
    /// Maximum number of words to keep; six is enough to be
    /// recognisable without producing absurdly long filenames.
    const MAX_WORDS: usize = 6;
    /// Hard char cap. A single very long word still gets truncated
    /// so a pathological input can't blow filesystem name limits.
    const MAX_CHARS: usize = 50;

    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().take(MAX_WORDS).collect();
    if words.is_empty() {
        return "untitled".to_owned();
    }
    let mut slug = words.join("-");
    if slug.chars().count() > MAX_CHARS {
        // Char-aware truncate, then trim any trailing partial-word
        // dashes so the slug never ends in a stray separator.
        let cut = slug
            .char_indices()
            .nth(MAX_CHARS)
            .map(|(i, _)| i)
            .unwrap_or(slug.len());
        slug.truncate(cut);
        slug = slug.trim_end_matches('-').to_owned();
    }
    slug
}
