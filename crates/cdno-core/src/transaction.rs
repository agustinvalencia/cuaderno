//! `VaultTransaction` — atomicity primitive over [`VaultStore`] and
//! [`VaultIndex`].
//!
//! Every write operation at the domain layer produces a
//! [`VaultTransaction`]. Callers buffer file and index operations,
//! then call [`commit`](VaultTransaction::commit) to apply them as a
//! unit. File writes are best-effort atomic: if a later write fails,
//! the previously-applied writes are rolled back in reverse order.
//! Index updates happen after all file writes succeed; if an index
//! call fails, files are left correct on disk and the index error is
//! surfaced as [`TransactionError::IndexStale`] — reconciliation on
//! next startup heals the divergence.
//!
//! **Scope.** This primitive provides *file-level consistency within a
//! single process*. It is not crash-safe: a power cut mid-commit can
//! leave the vault in a transient inconsistent state, which the
//! startup reconciliation pass detects and fixes.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{IndexError, StoreError, TransactionError};
use crate::extractors::AliasMap;
use crate::index::{
    ArchivalSnapshot, DeadlineEntry, LinkEntry, MilestoneEntry, NoteEntry, VaultIndex,
};
use crate::path::VaultPath;
use crate::paths::note_format;
use crate::store::{VaultStore, VaultWriteLock};

/// A buffered set of file and index operations that are applied
/// together by [`commit`](Self::commit).
///
/// Use `&mut self` buffer methods to enqueue ops, then consume the
/// transaction via `commit`. Consumption prevents accidental reuse
/// and makes the commit-or-drop decision explicit.
pub struct VaultTransaction {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    file_ops: Vec<FileOp>,
    index_ops: Vec<IndexOp>,
    /// The vault write lock, held from construction until this
    /// transaction drops (after `commit`) so the whole read-modify-write
    /// of the owning operation serialises against other processes (#196).
    /// Held, never read — its `Drop` does the work.
    _lock: VaultWriteLock,
}

/// File operations the transaction can apply. Each variant owns the
/// data it needs so the transaction is self-contained once buffered.
enum FileOp {
    Write {
        path: VaultPath,
        content: String,
    },
    Append {
        path: VaultPath,
        content: String,
    },
    Move {
        src: VaultPath,
        dest: VaultPath,
    },
    Delete {
        path: VaultPath,
    },
    /// Copy an external file into the vault (non-markdown attachment,
    /// #154). `src` is a real filesystem path; `dest` is vault-relative.
    Import {
        src: PathBuf,
        dest: VaultPath,
    },
}

/// Index operations the transaction can apply. Mirrors the
/// [`VaultIndex`] method set.
enum IndexOp {
    UpsertNote(NoteEntry),
    RemoveNote(VaultPath),
    ReplaceDeadlines(VaultPath, Vec<DeadlineEntry>),
    ReplaceLinks(VaultPath, Vec<LinkEntry>),
    ReplaceTags(VaultPath, Vec<String>),
    ReplaceMilestones(VaultPath, Vec<MilestoneEntry>),
    RecordArchivalSnapshot(VaultPath, ArchivalSnapshot),
    /// Replace the FTS row for a note: `(path, title, body)`. Buffered
    /// explicitly only by reconciliation; ordinary writes get their FTS
    /// row (and their tags and links facets) derived from the paired file
    /// write at commit (see `commit`).
    ReplaceFts(VaultPath, Option<String>, String),
}

/// Recorded information needed to undo one successfully-applied file
/// op. Captured at apply-time (not buffer-time) so the snapshot
/// reflects the actual pre-op state, including changes introduced by
/// prior ops in the same transaction.
enum Undo {
    /// Restore `path` to `content` (it existed before the op).
    Restore { path: VaultPath, content: String },
    /// Remove `path` — it was created by the op and must not linger.
    DeleteCreated { path: VaultPath },
    /// Reverse a move: current location is `dest`, put it back at `src`.
    MoveBack { src: VaultPath, dest: VaultPath },
}

impl VaultTransaction {
    /// Create an empty transaction bound to the given store and index,
    /// acquiring the vault write lock up front (#196).
    ///
    /// The lock is taken here, not at `commit`, deliberately: the lost
    /// update is born at the *read* (two writers read the same base, both
    /// rewrite it), so the lock must cover the operation's read-modify-write
    /// — which means creating the transaction *before* reading. Acquiring
    /// can fail or time out, hence the `Result`.
    pub fn new(store: Arc<dyn VaultStore>, index: Arc<dyn VaultIndex>) -> Result<Self, StoreError> {
        let lock = store.acquire_write_lock()?;
        Ok(Self {
            store,
            index,
            file_ops: Vec::new(),
            index_ops: Vec::new(),
            _lock: lock,
        })
    }

    // ---- file operation buffer --------------------------------------

    pub fn write_file(&mut self, path: VaultPath, content: impl Into<String>) {
        self.file_ops.push(FileOp::Write {
            path,
            content: content.into(),
        });
    }

    pub fn append_to_file(&mut self, path: VaultPath, content: impl Into<String>) {
        self.file_ops.push(FileOp::Append {
            path,
            content: content.into(),
        });
    }

    pub fn move_file(&mut self, src: VaultPath, dest: VaultPath) {
        self.file_ops.push(FileOp::Move { src, dest });
    }

    pub fn delete_file(&mut self, path: VaultPath) {
        self.file_ops.push(FileOp::Delete { path });
    }

    /// Buffer an import of the external file `src` into the vault at
    /// `dest` (an attachment, #154). Committed alongside the stub write,
    /// so a failed stub rolls the imported file back out.
    pub fn import_external(&mut self, src: PathBuf, dest: VaultPath) {
        self.file_ops.push(FileOp::Import { src, dest });
    }

    // ---- index operation buffer -------------------------------------

    pub fn upsert_note(&mut self, entry: NoteEntry) {
        self.index_ops.push(IndexOp::UpsertNote(entry));
    }

    pub fn remove_note(&mut self, path: VaultPath) {
        self.index_ops.push(IndexOp::RemoveNote(path));
    }

    pub fn replace_deadlines(&mut self, path: VaultPath, deadlines: Vec<DeadlineEntry>) {
        self.index_ops
            .push(IndexOp::ReplaceDeadlines(path, deadlines));
    }

    pub fn replace_links(&mut self, path: VaultPath, links: Vec<LinkEntry>) {
        self.index_ops.push(IndexOp::ReplaceLinks(path, links));
    }

    pub fn replace_tags(&mut self, path: VaultPath, tags: Vec<String>) {
        self.index_ops.push(IndexOp::ReplaceTags(path, tags));
    }

    pub fn replace_milestones(&mut self, path: VaultPath, milestones: Vec<MilestoneEntry>) {
        self.index_ops
            .push(IndexOp::ReplaceMilestones(path, milestones));
    }

    pub fn record_archival_snapshot(&mut self, path: VaultPath, snapshot: ArchivalSnapshot) {
        self.index_ops
            .push(IndexOp::RecordArchivalSnapshot(path, snapshot));
    }

    /// Explicitly buffer a full-text-index replacement for `path`.
    ///
    /// Ordinary write paths do **not** call this: their FTS row is
    /// derived automatically at commit from the paired `write_file`
    /// content (see [`commit`](Self::commit)). Reconciliation uses it
    /// because it reindexes a note that already exists on disk — there is
    /// no file write in that transaction to derive the body from.
    pub fn replace_fts(&mut self, path: VaultPath, title: Option<String>, body: String) {
        self.index_ops.push(IndexOp::ReplaceFts(path, title, body));
    }

    /// Every distinct vault path this transaction's *file* ops touch,
    /// in first-staged order.
    ///
    /// A move counts both endpoints — the source disappears and the
    /// destination appears, so a watcher (and the desktop echo journal,
    /// #315) sees a change at each. Index-only ops are excluded: they
    /// leave no file on disk for a watcher to observe.
    ///
    /// Deduplicated because a single logical write can stage the same
    /// path more than once (e.g. a note written then re-written in the
    /// same batch); the journal only needs each path once.
    ///
    /// Crate-private: [`commit`](Self::commit) is the only consumer and
    /// returns this set on success, so a caller never needs to snapshot it
    /// separately (and snapshotting before commit would report paths even
    /// on a rolled-back write).
    pub(crate) fn touched_paths(&self) -> Vec<VaultPath> {
        let mut paths: Vec<VaultPath> = Vec::with_capacity(self.file_ops.len());
        let mut push = |p: &VaultPath| {
            if !paths.contains(p) {
                paths.push(p.clone());
            }
        };
        for op in &self.file_ops {
            match op {
                FileOp::Write { path, .. }
                | FileOp::Append { path, .. }
                | FileOp::Delete { path } => push(path),
                FileOp::Move { src, dest } => {
                    push(src);
                    push(dest);
                }
                FileOp::Import { dest, .. } => push(dest),
            }
        }
        paths
    }

    /// Whether the index may hold a note row at `path` right now. Only a
    /// clean `Ok(None)` says it does not: a failed query keeps the path
    /// trigger, so the re-resolution pass still runs.
    fn indexed(&self, path: &VaultPath) -> bool {
        !matches!(self.index.find_by_path(path), Ok(None))
    }

    /// The notes this transaction would add and remove, derived from its
    /// buffered ops without applying them, and the one derivation `commit`
    /// uses. A path is *added* when `write_file` creates it and an
    /// `UpsertNote` names it or it has a note extension (a note rename
    /// writes even when it stages no row), or when a move lands on a path an
    /// `UpsertNote` names. It is *removed* when a file op deletes it or
    /// moves from it, no `UpsertNote` names it, and a `RemoveNote` names it
    /// or the index holds a row. An import or a bare move of an attachment
    /// is never a note. Whether a write creates its path is read from the
    /// store.
    pub fn note_delta(&self) -> (Vec<VaultPath>, HashSet<VaultPath>) {
        let mut present: HashMap<VaultPath, bool> = HashMap::new();
        let exists = |p: &VaultPath, present: &mut HashMap<VaultPath, bool>| {
            *present
                .entry(p.clone())
                .or_insert_with(|| self.store.exists(p).unwrap_or(false))
        };
        let mut effects: Vec<FileEffect> = Vec::new();
        for op in &self.file_ops {
            match op {
                FileOp::Write { path, .. } | FileOp::Append { path, .. } => {
                    if !exists(path, &mut present) {
                        effects.push(FileEffect::Written(path.clone()));
                    }
                    present.insert(path.clone(), true);
                }
                FileOp::Import { dest, .. } => {
                    effects.push(FileEffect::Imported);
                    present.insert(dest.clone(), true);
                }
                FileOp::Move { src, dest } => {
                    effects.push(FileEffect::Moved {
                        src: src.clone(),
                        dest: dest.clone(),
                    });
                    present.insert(src.clone(), false);
                    present.insert(dest.clone(), true);
                }
                FileOp::Delete { path } => {
                    effects.push(FileEffect::Deleted(path.clone()));
                    present.insert(path.clone(), false);
                }
            }
        }
        note_delta_from(&effects, &self.index_ops, &|p| self.indexed(p))
    }

    // ---- commit -----------------------------------------------------

    /// Apply every buffered operation. File ops run first, in
    /// enqueue order; on failure, previously-applied ops are rolled
    /// back best-effort. Index ops run only if every file op
    /// succeeded; failures collect into a single `IndexStale` error
    /// so the caller can log and move on.
    ///
    /// On success returns the transaction's touched file paths (see
    /// [`touched_paths`](Self::touched_paths)) so a caller can report
    /// exactly what changed — the desktop layer journals these for echo
    /// suppression (#315). Callers that don't need them discard the
    /// returned `Vec`.
    pub fn commit(mut self) -> Result<Vec<VaultPath>, TransactionError> {
        // Capture the touched set before consuming the op buffers below,
        // so a successful commit can hand it back to the caller.
        let touched = self.touched_paths();

        let delta = self.note_delta();

        // Phase 1: file ops with undo capture.
        let mut applied: Vec<Undo> = Vec::with_capacity(self.file_ops.len());
        for op in &self.file_ops {
            match apply_file_op(&*self.store, op) {
                Ok(undo) => applied.push(undo),
                Err(source) => {
                    let rollback_failures = rollback(&*self.store, applied);
                    return Err(TransactionError::FileWrite {
                        source,
                        rollback_failures,
                    });
                }
            }
        }

        // Only a change to these sets can change the answer for a link held
        // by some *other* note, so only then does the re-resolution below
        // run (#640). Read before any file op lands, so the index and the
        // store are in their pre-commit state.
        let (added_paths, removed_paths) = delta;

        // The files are now on disk, so each note write's real mtime is
        // knowable. The domain stamps `NoteEntry::mtime_ns` with the
        // entry-build instant (`build_index_entry_for` uses `now()`), which
        // is close to but never equals the file's mtime — leaving the index
        // row's mtime a small lie. Correct it (and `size`, which reconcile
        // compares alongside mtime) from the file just written so the
        // reconcile fast-path (#94) can trust the row and skip re-reading
        // unchanged notes. Reconcile's own upserts have no paired file write
        // and already carry the real mtime, so they're untouched.
        for op in &mut self.index_ops {
            if let IndexOp::UpsertNote(entry) = op
                && latest_write_content(&self.file_ops, &entry.path).is_some()
                && let Ok(meta) = self.store.metadata(&entry.path)
            {
                entry.mtime_ns = meta.mtime_ns();
                entry.size = meta.size;
            }
        }

        // Phase 2: index ops. A failure here leaves files correct;
        // collect every error so the caller sees the full picture.
        let mut index_errors: Vec<IndexError> = Vec::new();

        // The aliases each note will declare once this commit's index ops
        // land (RFC 0007 §5.5): the last `UpsertNote` or `RemoveNote` for a
        // path decides it. Read from the entries themselves, through the
        // parser `list_aliases` uses, so it cannot disagree with the index.
        let pending_aliases = pending_note_aliases(&self.index_ops);
        // The aliases the index holds, before the ops below, for every note
        // this commit both wrote (or moved, or deleted) on disk and re-rows.
        // Compared after the loop with `pending_aliases` to find the alias
        // names whose holders this commit changed. Reconcile's own
        // index-only commits stage no file op, so they never land here:
        // their path set is mid-walk, and Phase 2b re-resolves against the
        // whole pass instead.
        let mut aliases_before: Vec<(VaultPath, BTreeSet<String>)> = Vec::new();
        for path in &touched {
            if !pending_aliases.contains_key(path) {
                continue;
            }
            match self.index.find_by_path(path) {
                Ok(entry) => aliases_before.push((
                    path.clone(),
                    entry
                        .map(|e| crate::index::frontmatter_aliases(&e.frontmatter))
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                )),
                // The comparison cannot be made, so no alias name is treated
                // as changed; the path trigger below still runs.
                Err(e) => {
                    index_errors.push(e);
                    aliases_before.clear();
                    break;
                }
            }
        }

        // The note paths wikilinks resolve against, for the facet seam
        // below. Built lazily, once per commit, and only when some upsert
        // has a paired write.
        let mut link_targets: Option<HashSet<VaultPath>> = None;
        // The alias map those links resolve against, likewise lazy.
        let mut link_aliases: Option<AliasMap> = None;
        // Set when the path or alias query fails, so the re-resolution
        // below does not repeat it and report the same failure twice.
        let mut link_inputs_failed = false;
        for op in &self.index_ops {
            let op_ok = match apply_index_op(&*self.index, op) {
                Ok(()) => true,
                Err(e) => {
                    index_errors.push(e);
                    false
                }
            };

            // Keep the FTS projection in sync with note writes without
            // threading the body through every write call site. `NoteEntry`
            // carries no body, but every write that upserts a note also
            // buffers a `write_file` of that note's full content in this
            // same transaction — so the body is already here. Derive it and
            // refresh the note's FTS row. This single seam covers all
            // current and future write paths; reconciliation (which has no
            // paired write) uses the explicit `ReplaceFts` op instead.
            //
            // Only mirror an upsert that actually landed: there's no point
            // FTS-indexing a note whose metadata row failed to write. Body
            // is the content past the frontmatter; if the content has none
            // (malformed, or a non-note write), index it whole rather than
            // failing the commit — FTS is best-effort and reconcile re-heals.
            // The FTS title earns the bm25 title weight: the body's H1 for
            // Markdown (not `entry.title`), the block's `title` for Typst
            // (see `note::fts_title`).
            if op_ok
                && let IndexOp::UpsertNote(entry) = op
                && let Some(content) = latest_write_content(&self.file_ops, &entry.path)
            {
                let parsed = crate::frontmatter::parse_note(&entry.path, content).ok();
                let body = parsed.as_ref().map_or(content, |(_, body)| *body);
                let title =
                    crate::note::fts_title(&entry.path, parsed.as_ref().map(|(fm, _)| fm), body);
                if let Err(e) = self.index.replace_fts(&entry.path, title.as_deref(), body) {
                    index_errors.push(e);
                }

                // Same seam for the `tags` and `links` facets (#646). The
                // commit stamps the file's real mtime and size on the row
                // above, so reconcile's fast path skips this note from now
                // on and would never re-derive them — an edge or tag added
                // by an in-tool write has to land here or not at all. The
                // extraction is the one reconcile uses
                // (`extractors::extract_note_facets`), so the two agree.
                //
                // Both facets are always replaced, so nothing from the
                // note's previous content survives the fresh stamp. An
                // invalid `tags:` value (e.g. `tags: linalg`, a string
                // rather than a list) affects only the tags facet: the links
                // are staged as usual, and the tags fall back to the body's
                // inline `#tags` alone, the part that still parses.
                // Unparseable frontmatter leaves both facets as they were
                // (FTS above is equally best-effort); a failed path query
                // does too, and is reported as `IndexStale`.
                if let Some((fm, body)) = &parsed {
                    let inputs = link_targets_for(
                        &mut link_targets,
                        &*self.index,
                        &self.file_ops,
                        &self.index_ops,
                    )
                    .and_then(|targets| {
                        link_aliases_for(&mut link_aliases, &*self.index, &pending_aliases)
                            .map(|aliases| (targets, aliases))
                    });
                    match inputs {
                        Err(e) => {
                            link_inputs_failed = true;
                            index_errors.push(e);
                        }
                        Ok((targets, aliases)) => {
                            let facets = crate::extractors::extract_note_facets(
                                &entry.path,
                                fm,
                                &fm.as_json(),
                                body,
                                targets,
                                aliases,
                            );
                            let tags = facets.tags.unwrap_or_else(|_| {
                                crate::extractors::note_inline_tags(&entry.path, body)
                            });
                            if let Err(e) = self.index.replace_tags(&entry.path, &tags) {
                                index_errors.push(e);
                            }
                            if let Err(e) = self.index.replace_links(&entry.path, &facets.links) {
                                index_errors.push(e);
                            }
                        }
                    }
                }
            }
        }

        // A note created (or moved) here may be the target an earlier note
        // linked to before it existed; that note's row is not rewritten by
        // this commit, and reconcile's fast path would skip it too. Heal the
        // affected rows now, with the reconcile helper and the same path set
        // the facet seam above resolved against, so the new note's
        // backlinks are complete from the moment it is written (#640).
        //
        // Scoped to this commit's own changes. A resolved row is
        // reconsidered only if it points at a path this commit removed:
        // the path set is the index's, which omits notes present on disk
        // but unparseable, so "absent from the set" would wrongly unresolve
        // rows reconcile resolved to such a note. An unresolved row is
        // reconsidered only if its target could name a path this commit
        // added or removed, which keeps a routine note creation from
        // re-resolving every long-standing dangling link in the vault.
        //
        // The alias map is resolution's other input (RFC 0007 §5.5), so a
        // commit that changed the aliases a note declares re-resolves too,
        // even with no path added or removed: `[[old]]` written before
        // `aliases: [old]` arrived gains its edge now, and loses it when the
        // alias goes. The changed names are the symmetric difference of each
        // written note's aliases before and after. An unresolved row is
        // reconsidered when its target could name one of them (or an added
        // or removed path, as before); a row resolved through an alias also
        // when its holder's aliases changed, or when its target could name a
        // path added here (a stem outranks an alias). The map is read from
        // the index after this commit's upserts.
        let mut changed_alias_names: HashSet<String> = HashSet::new();
        let mut alias_changed_paths: HashSet<VaultPath> = HashSet::new();
        for (path, before) in &aliases_before {
            let after: BTreeSet<String> = pending_aliases
                .get(path)
                .map(|a| a.iter().cloned().collect())
                .unwrap_or_default();
            if after != *before {
                alias_changed_paths.insert(path.clone());
                changed_alias_names.extend(before.symmetric_difference(&after).cloned());
            }
        }
        // Known limitation (#892): when the link inputs fail here — the path
        // or alias query at the facet seam (`link_inputs_failed`), or the
        // `list_aliases` below — the commit reports `IndexStale` and the
        // affected rows stay stale. The same holds for the `aliases_before`
        // `find_by_path` read above (a failure there drops the alias trigger
        // while the path trigger still runs) and for the
        // `set_link_resolutions` write inside `reresolve_stale_links`. The next reconcile does not heal them:
        // the upsert already landed with the file's real mtime, so the note
        // takes the fast path, and Phase 2b runs only when that pass itself
        // adds or removes a note or changes the alias map. The path trigger
        // has had the same gap since #640.
        if (!added_paths.is_empty() || !removed_paths.is_empty() || !changed_alias_names.is_empty())
            && !link_inputs_failed
        {
            let names: HashSet<&str> = added_paths
                .iter()
                .chain(removed_paths.iter())
                .flat_map(crate::extractors::note_link_names)
                .chain(changed_alias_names.iter().map(String::as_str))
                .collect();
            let healed = link_targets_for(
                &mut link_targets,
                &*self.index,
                &self.file_ops,
                &self.index_ops,
            )
            .and_then(|targets| {
                let aliases = AliasMap::from_rows(self.index.list_aliases()?);
                crate::reconcile::reresolve_stale_links(
                    &*self.index,
                    targets,
                    &aliases,
                    |p, row| {
                        removed_paths.contains(p)
                                || (row.via_alias
                                    && (alias_changed_paths.contains(p)
                                        || crate::extractors::target_may_name(
                                            &row.target_raw,
                                            &names,
                                        )))
                                // A bare link resolved to a folder note by
                                // its folder name ranks below every stem
                                // and alias (#538), so it follows a path or
                                // alias that could take it.
                                || (crate::extractors::is_bare_folder_name_link(
                                    p,
                                    &row.target_raw,
                                ) && crate::extractors::target_may_name(
                                    &row.target_raw,
                                    &names,
                                ))
                    },
                    |t| crate::extractors::target_may_name(t, &names),
                )
            });
            if let Err(e) = healed {
                index_errors.push(e);
            }
        }

        if index_errors.is_empty() {
            Ok(touched)
        } else {
            Err(TransactionError::IndexStale(index_errors))
        }
    }
}

impl std::fmt::Debug for VaultTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The store/index trait objects don't have a useful Debug; show
        // just the buffered op counts so test output stays readable.
        f.debug_struct("VaultTransaction")
            .field("file_ops", &self.file_ops.len())
            .field("index_ops", &self.index_ops.len())
            .finish_non_exhaustive()
    }
}

/// Content of the last `Write` op targeting `path` in this transaction,
/// if any. Lets the commit FTS seam recover a note's body from the file
/// write it's paired with, instead of every write call site passing the
/// body explicitly. The last write wins (matches what lands on disk).
fn latest_write_content<'a>(file_ops: &'a [FileOp], path: &VaultPath) -> Option<&'a str> {
    file_ops.iter().rev().find_map(|op| match op {
        FileOp::Write { path: p, content } if p == path => Some(content.as_str()),
        _ => None,
    })
}

/// What one buffered file op does to the tree, for [`note_delta_from`].
enum FileEffect {
    /// `write_file` or `append_to_file` created a file.
    Written(VaultPath),
    /// An external file was imported: bytes, never a note.
    Imported,
    Moved {
        src: VaultPath,
        dest: VaultPath,
    },
    Deleted(VaultPath),
}

fn upserted_paths(index_ops: &[IndexOp]) -> HashSet<&VaultPath> {
    index_ops
        .iter()
        .filter_map(|op| match op {
            IndexOp::UpsertNote(entry) => Some(&entry.path),
            _ => None,
        })
        .collect()
}

/// Whether a file written at `path` is a note: an `UpsertNote` names it, or
/// it has a note extension (a rename writes a note it stages no row for).
fn is_note_write(path: &VaultPath, upserted: &HashSet<&VaultPath>) -> bool {
    upserted.contains(path) || note_format(path).is_some()
}

/// The notes a commit added and removed; see [`VaultTransaction::note_delta`].
/// A `RemoveNote` with no file op beside it (reconcile's orphan removal) is
/// not a delta: reconcile's own Phase 2b re-resolves against the whole pass.
fn note_delta_from(
    effects: &[FileEffect],
    index_ops: &[IndexOp],
    indexed: &dyn Fn(&VaultPath) -> bool,
) -> (Vec<VaultPath>, HashSet<VaultPath>) {
    let upserted = upserted_paths(index_ops);
    let named_removed: HashSet<&VaultPath> = index_ops
        .iter()
        .filter_map(|op| match op {
            IndexOp::RemoveNote(path) => Some(path),
            _ => None,
        })
        .collect();
    let gone = |path: &VaultPath| {
        !upserted.contains(path) && (named_removed.contains(path) || indexed(path))
    };
    let mut added: Vec<VaultPath> = Vec::new();
    let mut removed: HashSet<VaultPath> = HashSet::new();
    for effect in effects {
        match effect {
            FileEffect::Written(path) if is_note_write(path, &upserted) => {
                added.push(path.clone());
            }
            FileEffect::Moved { src, dest } => {
                if gone(src) {
                    removed.insert(src.clone());
                }
                if upserted.contains(dest) {
                    added.push(dest.clone());
                }
            }
            FileEffect::Deleted(path) if gone(path) => {
                removed.insert(path.clone());
            }
            _ => {}
        }
    }
    (added, removed)
}

/// The note paths a wikilink written in this transaction resolves
/// against: every note the index holds, adjusted by this transaction's
/// file ops so a note created (or deleted) alongside the linking note is
/// already accounted for. After startup reconciliation the index's path
/// set is reconcile's own `fs_set` less any file it could not parse, so an
/// in-tool write resolves as a reindex would. Computed once per commit and
/// cached in
/// `cache`; the index query error, if any, is returned, and the caller
/// skips the facets and reports it as `IndexStale`.
fn link_targets_for<'a>(
    cache: &'a mut Option<HashSet<VaultPath>>,
    index: &dyn VaultIndex,
    file_ops: &[FileOp],
    index_ops: &[IndexOp],
) -> Result<&'a HashSet<VaultPath>, IndexError> {
    if cache.is_none() {
        let mut paths: HashSet<VaultPath> = index.list_all_paths()?.into_iter().collect();
        let upserted = upserted_paths(index_ops);
        for op in file_ops {
            match op {
                FileOp::Write { path, .. } | FileOp::Append { path, .. }
                    if is_note_write(path, &upserted) =>
                {
                    paths.insert(path.clone());
                }
                FileOp::Move { src, dest } => {
                    paths.remove(src);
                    if upserted.contains(dest) {
                        paths.insert(dest.clone());
                    }
                }
                FileOp::Delete { path } => {
                    paths.remove(path);
                }
                _ => {}
            }
        }
        *cache = Some(paths);
    }
    Ok(cache.as_ref().expect("populated above"))
}

/// The aliases each note upserted or removed by `index_ops` declares once
/// they are applied: the last op for a path wins, an `UpsertNote` declaring
/// its entry's `aliases:` (read as [`VaultIndex::list_aliases`] reads them)
/// and a `RemoveNote` none.
fn pending_note_aliases(index_ops: &[IndexOp]) -> HashMap<VaultPath, Vec<String>> {
    let mut pending = HashMap::new();
    for op in index_ops {
        match op {
            IndexOp::UpsertNote(entry) => {
                pending.insert(
                    entry.path.clone(),
                    crate::index::frontmatter_aliases(&entry.frontmatter),
                );
            }
            IndexOp::RemoveNote(path) => {
                pending.insert(path.clone(), Vec::new());
            }
            _ => {}
        }
    }
    pending
}

/// The alias map a wikilink written in this transaction resolves against:
/// the index's aliases with every note this transaction re-rows replaced
/// by what it will declare (`pending`), so the map is the one the index
/// holds after the commit's upserts even while they are being applied. A
/// shared alias stays unresolvable ([`AliasMap::from_rows`]). Computed once
/// per commit and cached in `cache`, like [`link_targets_for`].
fn link_aliases_for<'a>(
    cache: &'a mut Option<AliasMap>,
    index: &dyn VaultIndex,
    pending: &HashMap<VaultPath, Vec<String>>,
) -> Result<&'a AliasMap, IndexError> {
    if cache.is_none() {
        let mut rows: Vec<(String, VaultPath)> = index
            .list_aliases()?
            .into_iter()
            .filter(|(_, path)| !pending.contains_key(path))
            .collect();
        for (path, aliases) in pending {
            rows.extend(aliases.iter().map(|a| (a.clone(), path.clone())));
        }
        *cache = Some(AliasMap::from_rows(rows));
    }
    Ok(cache.as_ref().expect("populated above"))
}

/// Apply a single file op, returning the undo information needed to
/// reverse it if a later op fails.
fn apply_file_op(store: &dyn VaultStore, op: &FileOp) -> Result<Undo, StoreError> {
    match op {
        FileOp::Write { path, content } => {
            // Capture the pre-write content (or note that the file
            // didn't exist) so rollback can restore exactly that state.
            let snapshot = read_if_exists(store, path)?;
            store.write_file(path, content)?;
            Ok(match snapshot {
                Some(original) => Undo::Restore {
                    path: path.clone(),
                    content: original,
                },
                None => Undo::DeleteCreated { path: path.clone() },
            })
        }
        FileOp::Append { path, content } => {
            // Same snapshot strategy as Write — an append that creates
            // a previously-absent file rolls back via delete; appends
            // onto an existing file roll back by restoring the
            // pre-append content.
            let snapshot = read_if_exists(store, path)?;
            store.append_to_file(path, content)?;
            Ok(match snapshot {
                Some(original) => Undo::Restore {
                    path: path.clone(),
                    content: original,
                },
                None => Undo::DeleteCreated { path: path.clone() },
            })
        }
        FileOp::Move { src, dest } => {
            store.move_file(src, dest)?;
            Ok(Undo::MoveBack {
                src: src.clone(),
                dest: dest.clone(),
            })
        }
        FileOp::Delete { path } => {
            // Snapshot before deletion so rollback can restore the
            // file with its exact content.
            let content = store.read_file(path)?;
            store.delete_file(path)?;
            Ok(Undo::Restore {
                path: path.clone(),
                content,
            })
        }
        FileOp::Import { src, dest } => {
            // The domain checks `dest` is unoccupied before buffering this,
            // so the import always creates a new file — rollback deletes
            // it. (Binary content isn't snapshotted for restore; an import
            // never overwrites.)
            store.import_external(src, dest)?;
            Ok(Undo::DeleteCreated { path: dest.clone() })
        }
    }
}

/// Read `path` into memory if it exists, or return `None` if it
/// doesn't. Any other error is surfaced so a broken store bails
/// before the op is attempted.
fn read_if_exists(store: &dyn VaultStore, path: &VaultPath) -> Result<Option<String>, StoreError> {
    match store.read_file(path) {
        Ok(content) => Ok(Some(content)),
        Err(StoreError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Attempt to reverse every successfully-applied op, in reverse
/// order. Collects any undo failures into a `Vec<StoreError>` so the
/// caller can surface them alongside the triggering error.
fn rollback(store: &dyn VaultStore, applied: Vec<Undo>) -> Vec<StoreError> {
    let mut failures = Vec::new();
    for undo in applied.into_iter().rev() {
        if let Err(e) = apply_undo(store, undo) {
            failures.push(e);
        }
    }
    failures
}

fn apply_undo(store: &dyn VaultStore, undo: Undo) -> Result<(), StoreError> {
    match undo {
        Undo::Restore { path, content } => store.write_file(&path, &content),
        Undo::DeleteCreated { path } => store.delete_file(&path),
        Undo::MoveBack { src, dest } => store.move_file(&dest, &src),
    }
}

fn apply_index_op(index: &dyn VaultIndex, op: &IndexOp) -> Result<(), IndexError> {
    match op {
        IndexOp::UpsertNote(entry) => index.upsert_note(entry),
        IndexOp::RemoveNote(path) => index.remove_note(path),
        IndexOp::ReplaceDeadlines(path, deadlines) => index.replace_deadlines(path, deadlines),
        IndexOp::ReplaceLinks(path, links) => index.replace_links(path, links),
        IndexOp::ReplaceTags(path, tags) => index.replace_tags(path, tags),
        IndexOp::ReplaceMilestones(path, milestones) => index.replace_milestones(path, milestones),
        IndexOp::RecordArchivalSnapshot(path, snapshot) => {
            index.record_archival_snapshot(path, snapshot)
        }
        IndexOp::ReplaceFts(path, title, body) => index.replace_fts(path, title.as_deref(), body),
    }
}
