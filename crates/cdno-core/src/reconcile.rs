//! Startup reconciliation.
//!
//! Reconciliation makes the index reflect the filesystem. It runs on
//! every `Vault::new` (and can be re-run on demand). The algorithm is
//! simple:
//!
//! 1. Walk every `.md` file in the vault.
//! 2. For each file: read, hash, compare against the matching index
//!    row. Reindex if the hash differs or no row exists.
//! 3. Any index row whose path isn't in the walk is an orphan — remove
//!    it. Cascading FKs drop its deadlines, links, and tags.
//!
//! Per-note transactions keep one corrupted note from blocking the
//! others: a parse error is recorded in the report and reconciliation
//! continues. The vault is source of truth — the index is always
//! rebuildable from the filesystem.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::IndexError;
use crate::frontmatter::Frontmatter;
use crate::hash::content_hash;
use crate::index::{DeadlineEntry, NoteEntry, VaultIndex};
use crate::markdown::{MarkdownDocument, extract_hard_deadlines};
use crate::path::VaultPath;
use crate::store::VaultStore;
use crate::transaction::VaultTransaction;

/// Summary of a reconciliation pass. Fields are incremented in place
/// as files are processed; no assumption that `scanned == added +
/// updated + skipped`, because errors also count toward `scanned`.
#[derive(Debug, Default)]
pub struct ReconciliationReport {
    /// Total `.md` files walked in the filesystem.
    pub scanned: usize,
    /// Files not previously in the index that were added this pass.
    pub added: usize,
    /// Files whose content_hash differed from the index row and were
    /// reindexed.
    pub updated: usize,
    /// Index rows that had no corresponding filesystem file and were
    /// dropped (cascades facets).
    pub removed: usize,
    /// Per-file failures — typically parse errors on a corrupted note.
    /// Reconciliation continues past these; the offending file stays
    /// unindexed until fixed.
    pub errors: Vec<ReconciliationIssue>,
}

/// One per-file failure encountered during reconciliation.
#[derive(Debug)]
pub struct ReconciliationIssue {
    pub path: VaultPath,
    pub reason: String,
}

/// Reconcile the index against the filesystem. See module docs for the
/// algorithm. Returns `Err` only for catastrophic failures (e.g. the
/// vault walk itself fails); per-file errors accumulate into the
/// report so a single broken note doesn't abort the whole pass.
pub fn reconcile(
    store: &Arc<dyn VaultStore>,
    index: &Arc<dyn VaultIndex>,
) -> Result<ReconciliationReport, IndexError> {
    let mut report = ReconciliationReport::default();

    // Gather the filesystem's current `.md` paths. Non-markdown files
    // (PDFs, .ipynb attachments) are discoverable via the store but
    // never indexed — they have no frontmatter contract.
    let all_fs_paths = store
        .walk_dir(&VaultPath::root())
        .map_err(|e| IndexError::Query(format!("walk_dir failed during reconcile: {e}")))?;
    let fs_md_paths: Vec<VaultPath> = all_fs_paths
        .into_iter()
        .filter(|p| p.as_path().extension() == Some(OsStr::new("md")))
        .collect();
    let fs_set: HashSet<VaultPath> = fs_md_paths.iter().cloned().collect();

    // Phase 1: walk the filesystem, ensure every `.md` is correctly
    // reflected in the index.
    for path in &fs_md_paths {
        report.scanned += 1;
        match reconcile_one(store, index, path) {
            Ok(Outcome::Added) => report.added += 1,
            Ok(Outcome::Updated) => report.updated += 1,
            Ok(Outcome::Skipped) => {}
            Err(reason) => report.errors.push(ReconciliationIssue {
                path: path.clone(),
                reason,
            }),
        }
    }

    // Phase 2: remove orphans. Any path in the index but not in the
    // filesystem walk is dropped. Cascading FKs (and MemoryIndex's
    // manual cascade) clean up deadlines, links, and tags.
    let index_paths = index.list_all_paths()?;
    for path in index_paths {
        if !fs_set.contains(&path) {
            // Use a transaction so the remove_note is uniform with
            // the rest of the reconciliation pipeline. The index-only
            // commit can't fail the file phase.
            let mut tx = VaultTransaction::new(store.clone(), index.clone());
            tx.remove_note(path.clone());
            match tx.commit() {
                Ok(()) | Err(crate::error::TransactionError::IndexStale(_)) => {
                    // Best-effort: if the index call failed here, the
                    // next reconciliation will retry — the row is
                    // still orphaned, still detected.
                    report.removed += 1;
                }
                Err(other) => {
                    report.errors.push(ReconciliationIssue {
                        path,
                        reason: format!("failed to remove orphan: {other}"),
                    });
                }
            }
        }
    }

    Ok(report)
}

/// Per-file outcome reported back to the caller counters.
enum Outcome {
    Added,
    Updated,
    Skipped,
}

/// Reindex a single note if it's missing from the index or its hash
/// has drifted. Returns a string-reason error on any parse failure
/// so the caller can record it without aborting the pass.
fn reconcile_one(
    store: &Arc<dyn VaultStore>,
    index: &Arc<dyn VaultIndex>,
    path: &VaultPath,
) -> Result<Outcome, String> {
    // Read content up-front: we need it to hash, and we'll also need
    // it to parse for added/updated notes. A single read is cheaper
    // than the mtime-check-then-read dance for vault-scale data.
    let content = store
        .read_file(path)
        .map_err(|e| format!("read failed: {e}"))?;
    let hash = content_hash(&content);

    let existing = index
        .find_by_path(path)
        .map_err(|e| format!("index lookup failed: {e}"))?;

    if let Some(entry) = &existing
        && entry.content_hash == hash
    {
        return Ok(Outcome::Skipped);
    }

    // Either a brand-new note or one whose content_hash drifted.
    // Parse, build the NoteEntry + facets, commit atomically.
    let meta = store
        .metadata(path)
        .map_err(|e| format!("metadata failed: {e}"))?;
    let mtime_ns = system_time_to_ns(meta.mtime);
    let indexed_at_ns = system_time_to_ns(SystemTime::now());

    let (frontmatter, body) =
        Frontmatter::parse(&content).map_err(|e| format!("frontmatter parse failed: {e}"))?;
    let note_type = frontmatter
        .require_field::<String>("type")
        .map_err(|e| format!("missing or invalid `type` field: {e}"))?;
    let title = frontmatter
        .optional_field::<String>("title")
        .map_err(|e| format!("invalid `title` field: {e}"))?;

    // Frontmatter `tags:` list. Inline body-scanned `#tag` and
    // wikilink extraction are deferred to a follow-up task.
    let tags: Vec<String> = frontmatter
        .optional_field::<Vec<String>>("tags")
        .map_err(|e| format!("invalid `tags` field: {e}"))?
        .unwrap_or_default();

    // Project-type notes contribute deadlines via `## Milestones`.
    // Other types skip this even if they happen to have a section of
    // the same name, to match the domain-level semantics.
    let deadlines = if note_type == "project" {
        collect_deadlines_from_body(&content, body).unwrap_or_default()
    } else {
        Vec::new()
    };

    let entry = NoteEntry {
        path: path.clone(),
        note_type,
        title,
        content_hash: hash,
        mtime_ns,
        size: meta.size,
        frontmatter: frontmatter.as_json(),
        indexed_at_ns,
    };

    let outcome = if existing.is_some() {
        Outcome::Updated
    } else {
        Outcome::Added
    };

    let mut tx = VaultTransaction::new(store.clone(), index.clone());
    tx.upsert_note(entry);
    tx.replace_deadlines(path.clone(), deadlines);
    tx.replace_tags(path.clone(), tags);
    tx.commit().map_err(|e| format!("commit failed: {e}"))?;

    Ok(outcome)
}

/// Parse the raw document as a [`MarkdownDocument`] to find the
/// `## Milestones` section, then run `extract_hard_deadlines` on it.
/// Returns an empty list if the section is absent — a project is
/// allowed to have no active hard deadlines.
fn collect_deadlines_from_body(raw: &str, _body_slice: &str) -> Option<Vec<DeadlineEntry>> {
    let doc = MarkdownDocument::parse(raw).ok()?;
    let section = doc.section("Milestones").ok()?;
    let deadlines = extract_hard_deadlines(section)
        .into_iter()
        .map(|(title, due_date)| DeadlineEntry {
            source: "project_milestone".to_owned(),
            title,
            due_date,
            is_hard: true,
            // Context derivation needs the frontmatter `context` field;
            // not threaded here yet, but the index column accepts NULL
            // and the commitments query can filter client-side until
            // it becomes load-bearing.
            context: None,
        })
        .collect();
    Some(deadlines)
}

/// Convert a `SystemTime` to nanoseconds since the UNIX epoch.
/// Pre-epoch times (which shouldn't occur on a live filesystem) are
/// clamped to 0.
fn system_time_to_ns(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
