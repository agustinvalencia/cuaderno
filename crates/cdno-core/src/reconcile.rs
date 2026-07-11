//! Startup reconciliation.
//!
//! Reconciliation makes the index reflect the filesystem. It runs on
//! every `Vault::new` (and can be re-run on demand). The algorithm is
//! simple:
//!
//! 1. Walk every `.md` file in the vault, minus any matching the
//!    config `ignore` globs (passed in as a compiled [`IgnoreSet`]).
//! 2. For each file: take the fast path (skip on matching mtime + size,
//!    #94), else read, hash, and compare against the matching index row.
//!    Reindex if the hash differs or no row exists.
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

use crate::config::IgnoreSet;
use crate::error::IndexError;
use crate::frontmatter::Frontmatter;
use crate::hash::content_hash;
use crate::index::{DeadlineEntry, MilestoneEntry, NoteEntry, VaultIndex};
use crate::markdown::{MarkdownDocument, extract_hard_deadlines, extract_milestones_from_body};
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
    /// Markdown files excluded from this pass by the config `ignore`
    /// globs (#242). Surfaced so an over-broad pattern (a stray `**`)
    /// that drops notes from search/lint/backlinks is observable rather
    /// than a silent retrieval blackout — the files themselves are never
    /// touched, and clearing the glob then reindexing restores every row.
    pub ignored: usize,
    /// Notes present in the `notes` table but absent from the FTS index
    /// that were backfilled this pass. Non-zero on the first reconcile
    /// after the FTS migration (or after the index is dropped), when the
    /// per-file hash fast-path skips unchanged notes and so never
    /// repopulates their search rows.
    pub fts_built: usize,
    /// FTS rows with no surviving note that were dropped this pass.
    pub fts_removed: usize,
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
    ignore: &IgnoreSet,
) -> Result<ReconciliationReport, IndexError> {
    let mut report = ReconciliationReport::default();

    // Gather the filesystem's current `.md` paths. Non-markdown files
    // (PDFs, .ipynb attachments) are discoverable via the store but
    // never indexed — they have no frontmatter contract.
    let all_fs_paths = store
        .walk_dir(&VaultPath::root())
        .map_err(|e| IndexError::Query(format!("walk_dir failed during reconcile: {e}")))?;
    let candidate_md_paths: Vec<VaultPath> = all_fs_paths
        .into_iter()
        .filter(|p| p.as_path().extension() == Some(OsStr::new("md")))
        // `.cuaderno/` is the vault's meta directory — config,
        // templates, index database. Its contents are infrastructure,
        // not notes; indexing them would mean e.g. the dumped daily
        // template surfacing in "all daily notes" queries.
        .filter(|p| !p.as_path().starts_with(crate::paths::CUADERNO_DIR))
        .collect();
    // Config `ignore` globs (#242): user-declared non-vault docs (e.g.
    // CLAUDE.md, README.md) that live in the vault dir but aren't notes.
    // Excluding them here is the single enforcement point — a path absent
    // from the index is also absent from lint (index-driven) and search.
    // A file that was indexed before becoming ignored falls out via
    // Phase 2's orphan removal, since it's no longer in `fs_set`.
    //
    // Partition rather than filter so the excluded count is reported: an
    // over-broad pattern silently evicting notes from retrieval is the
    // feature's sharpest footgun, so the number must be observable. The
    // files are never touched; clearing the glob and reindexing restores
    // every row.
    let (ignored_paths, fs_md_paths): (Vec<VaultPath>, Vec<VaultPath>) = candidate_md_paths
        .into_iter()
        .partition(|p| ignore.is_match(p.as_path()));
    report.ignored = ignored_paths.len();
    let fs_set: HashSet<VaultPath> = fs_md_paths.iter().cloned().collect();

    // Phase 1: walk the filesystem, ensure every `.md` is correctly
    // reflected in the index. The full path set is threaded into
    // `reconcile_one` so wikilink resolution can do its
    // exact-then-basename lookup without extra index calls.
    for path in &fs_md_paths {
        report.scanned += 1;
        match reconcile_one(store, index, path, &fs_set) {
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
            // Orphan removal is index-only, but we still go through a
            // VaultTransaction for uniformity with the rest of the
            // reconciliation pipeline.
            //
            // Any error — whether IndexStale or (in principle) a file
            // op failure — means the note is *still* in the index, so
            // we record it as an error rather than marking it removed.
            // The next reconciliation pass will retry.
            let mut tx = VaultTransaction::new(store.clone(), index.clone()).map_err(|e| {
                IndexError::Update(format!("acquiring write lock during reconcile: {e}"))
            })?;
            tx.remove_note(path.clone());
            match tx.commit() {
                Ok(_) => report.removed += 1,
                Err(e) => report.errors.push(ReconciliationIssue {
                    path,
                    reason: format!("failed to remove orphan: {e}"),
                }),
            }
        }
    }

    // Phase 3: heal the FTS index by path-set diff against `notes`.
    //
    // Phase 1 only reindexes notes whose content changed; an unchanged
    // note hits the mtime/hash fast path and never re-enters the
    // body-parsing path. So a freshly-migrated (or dropped) FTS table would
    // stay empty for every note that hasn't been touched since. Reconcile it
    // here, independent of the per-file fast path: any note missing from FTS
    // is backfilled from its file;
    // any FTS row without a surviving note is dropped. Steady state is two
    // path-set queries and an empty diff.
    let note_paths: HashSet<VaultPath> = index.list_all_paths()?.into_iter().collect();
    let fts_paths: HashSet<VaultPath> = index.fts_indexed_paths()?.into_iter().collect();

    for path in note_paths.difference(&fts_paths) {
        // Only backfill notes that actually exist on disk. A path in the
        // index but absent from the filesystem walk is an orphan that
        // Phase 2 owns; trying to read its (missing) file here would just
        // surface a redundant error for a problem already reported.
        if !fs_set.contains(path) {
            continue;
        }
        match backfill_fts_one(store, index, path) {
            Ok(()) => report.fts_built += 1,
            Err(reason) => report.errors.push(ReconciliationIssue {
                path: path.clone(),
                reason,
            }),
        }
    }

    for path in fts_paths.difference(&note_paths) {
        // An FTS row whose note is gone. `remove_note` deletes both the
        // (already-absent) notes row and the FTS row, so it's the right
        // primitive even though only the FTS side has anything to drop.
        let mut tx = VaultTransaction::new(store.clone(), index.clone()).map_err(|e| {
            IndexError::Update(format!("acquiring write lock during reconcile: {e}"))
        })?;
        tx.remove_note(path.clone());
        match tx.commit() {
            Ok(_) => report.fts_removed += 1,
            Err(e) => report.errors.push(ReconciliationIssue {
                path: path.clone(),
                reason: format!("failed to remove orphan FTS row: {e}"),
            }),
        }
    }

    Ok(report)
}

/// Backfill the FTS row for a single note from its file on disk. Used by
/// the reconcile FTS-heal pass for notes present in `notes` but missing
/// from search (e.g. every note on the first open after the FTS
/// migration). Returns a string-reason error on any read/parse failure so
/// the caller records it without aborting the pass.
fn backfill_fts_one(
    store: &Arc<dyn VaultStore>,
    index: &Arc<dyn VaultIndex>,
    path: &VaultPath,
) -> Result<(), String> {
    let content = store
        .read_file(path)
        .map_err(|e| format!("read failed: {e}"))?;
    let (_frontmatter, body) =
        Frontmatter::parse(&content).map_err(|e| format!("frontmatter parse failed: {e}"))?;
    let title = crate::extractors::first_h1(body);

    let mut tx = VaultTransaction::new(store.clone(), index.clone())
        .map_err(|e| format!("acquiring write lock: {e}"))?;
    tx.replace_fts(path.clone(), title, body.to_owned());
    tx.commit().map_err(|e| format!("commit failed: {e}"))?;
    Ok(())
}

/// Refresh just the `mtime_ns`/`size` of an existing index row whose
/// content is unchanged, so a touched-but-identical file takes the
/// reconcile fast path next pass instead of being re-read every time.
/// Re-uses the row otherwise verbatim — no facet or FTS work. The upsert
/// has no paired file write, so `VaultTransaction::commit` leaves the
/// mtime we set here alone.
fn restamp_meta(
    store: &Arc<dyn VaultStore>,
    index: &Arc<dyn VaultIndex>,
    existing: &NoteEntry,
    mtime_ns: u64,
    size: u64,
) -> Result<(), String> {
    let mut updated = existing.clone();
    updated.mtime_ns = mtime_ns;
    updated.size = size;
    updated.indexed_at_ns = system_time_to_ns(SystemTime::now());

    let mut tx = VaultTransaction::new(store.clone(), index.clone())
        .map_err(|e| format!("acquiring write lock: {e}"))?;
    tx.upsert_note(updated);
    tx.commit()
        .map_err(|e| format!("mtime re-stamp failed: {e}"))?;
    Ok(())
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
    vault_paths: &HashSet<VaultPath>,
) -> Result<Outcome, String> {
    let existing = index
        .find_by_path(path)
        .map_err(|e| format!("index lookup failed: {e}"))?;
    let meta = store
        .metadata(path)
        .map_err(|e| format!("metadata failed: {e}"))?;

    // Fast path (#94): an already-indexed file whose mtime and size both
    // match is assumed unchanged — skip the read + hash entirely. mtime can
    // lie (preserved across a copy, drift across filesystems); a false
    // "unchanged" only leaves a stale row, which the next size- or
    // mtime-changing edit corrects, and the content hash on the slow path
    // below stays the source of truth. Note writes store the file's real
    // mtime (`VaultTransaction::commit`), so cdno's own notes hit this path
    // too — the steady-state win for CLI verbs that re-reconcile on every
    // invocation.
    if let Some(entry) = &existing
        && entry.mtime_ns == meta.mtime_ns()
        && entry.size == meta.size
    {
        return Ok(Outcome::Skipped);
    }

    // Slow path: read + hash. Either the fast path missed (mtime/size
    // drifted) or this is a new file. The hash decides whether content
    // really changed.
    let content = store
        .read_file(path)
        .map_err(|e| format!("read failed: {e}"))?;
    let hash = content_hash(&content);

    if let Some(entry) = &existing
        && entry.content_hash == hash
    {
        // mtime/size drifted but the bytes are identical — a `touch`, a git
        // checkout restoring the same content, an editor save-without-edit.
        // Re-stamp the row's mtime/size so the *next* pass takes the fast
        // path instead of re-reading this file on every reconcile forever.
        // Content is unchanged, so facets and FTS are left untouched.
        restamp_meta(store, index, entry, meta.mtime_ns(), meta.size)?;
        return Ok(Outcome::Skipped);
    }

    // Either a brand-new note or one whose content_hash drifted.
    // Parse, build the NoteEntry + facets, commit atomically.
    let mtime_ns = meta.mtime_ns();
    let indexed_at_ns = system_time_to_ns(SystemTime::now());

    let (frontmatter, body) =
        Frontmatter::parse(&content).map_err(|e| format!("frontmatter parse failed: {e}"))?;
    let note_type = frontmatter
        .require_field::<String>("type")
        .map_err(|e| format!("missing or invalid `type` field: {e}"))?;
    let title = frontmatter
        .optional_field::<String>("title")
        .map_err(|e| format!("invalid `title` field: {e}"))?;

    // Frontmatter `tags:` list, then merge with body-scanned inline
    // tags. The extractor returns an already-deduped sorted list; the
    // merge path here keeps the order stable across reconcile passes
    // so the `note_tags` table doesn't churn.
    let frontmatter_tags: Vec<String> = frontmatter
        .optional_field::<Vec<String>>("tags")
        .map_err(|e| format!("invalid `tags` field: {e}"))?
        .unwrap_or_default();
    let inline_tags = crate::extractors::extract_inline_tags(body);
    let mut tag_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    tag_set.extend(frontmatter_tags);
    tag_set.extend(inline_tags);
    let tags: Vec<String> = tag_set.into_iter().collect();

    // Body-scanned wikilinks, merged with frontmatter wikilinks (a
    // project's `core_question:`, a portfolio's `project:`, an evidence
    // note's `origin:`, …) so backlinks see frontmatter references too
    // (#395). Deduped by (target, label) so a link present in both the body
    // and frontmatter yields one edge, not two. Resolution staleness is
    // bounded by the next reconcile pass (every Vault::new) — see
    // `extractors::resolve_wikilinks` for the exact-then-basename policy.
    let mut raw_links = crate::extractors::extract_wikilinks(body);
    raw_links.extend(crate::extractors::extract_frontmatter_wikilinks(
        &frontmatter.as_json(),
    ));
    let mut seen = std::collections::HashSet::new();
    raw_links.retain(|l| seen.insert((l.target.clone(), l.label.clone())));
    let links = crate::extractors::resolve_wikilinks(raw_links, vault_paths);

    // Project-type notes contribute deadlines and milestones via
    // `## Milestones`. Other types skip both even if they happen to
    // have a section of the same name, to match the domain-level
    // semantics. Deadlines are the hard-only commitments funnel;
    // milestones are the full event timeline (#109).
    let is_project = note_type == "project";
    let deadlines = if is_project {
        collect_deadlines_from_body(&content, body).unwrap_or_default()
    } else {
        Vec::new()
    };
    let milestones = if is_project {
        collect_milestones_from_body(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    // The FTS row needs title + body. This path reindexes a note already
    // on disk, so there's no paired file write for the commit seam to
    // derive the body from — buffer the FTS replacement explicitly. The
    // FTS title is the body's H1 (where notes carry their title), so it
    // earns the bm25 title weight; `title` (frontmatter-derived) is the
    // `notes` row's own field and is a separate concern.
    let fts_title = crate::extractors::first_h1(body);
    let fts_body = body.to_owned();

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

    let mut tx = VaultTransaction::new(store.clone(), index.clone())
        .map_err(|e| format!("acquiring write lock: {e}"))?;
    tx.upsert_note(entry);
    tx.replace_deadlines(path.clone(), deadlines);
    tx.replace_milestones(path.clone(), milestones);
    tx.replace_tags(path.clone(), tags);
    tx.replace_links(path.clone(), links);
    tx.replace_fts(path.clone(), fts_title, fts_body);
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

/// Parse the `## Milestones` section into the full milestone timeline
/// (hard/soft, pending/completed, dated or not). Returns `None` only
/// when the document can't be parsed; an absent section yields an
/// empty list. Distinct from `collect_deadlines_from_body`, which
/// keeps only the hard-deadline subset for the commitments funnel.
fn collect_milestones_from_body(raw: &str) -> Option<Vec<MilestoneEntry>> {
    let doc = MarkdownDocument::parse(raw).ok()?;
    match doc.section("Milestones") {
        Ok(section) => Some(extract_milestones_from_body(section)),
        Err(_) => Some(Vec::new()),
    }
}

/// Convert a `SystemTime` to nanoseconds since the UNIX epoch.
/// Pre-epoch times (which shouldn't occur on a live filesystem) are
/// clamped to 0.
fn system_time_to_ns(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
