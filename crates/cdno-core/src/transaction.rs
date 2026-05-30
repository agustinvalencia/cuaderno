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

use std::sync::Arc;

use crate::error::{IndexError, StoreError, TransactionError};
use crate::index::{
    ArchivalSnapshot, DeadlineEntry, LinkEntry, MilestoneEntry, NoteEntry, VaultIndex,
};
use crate::path::VaultPath;
use crate::store::VaultStore;

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
}

/// File operations the transaction can apply. Each variant owns the
/// data it needs so the transaction is self-contained once buffered.
enum FileOp {
    Write { path: VaultPath, content: String },
    Append { path: VaultPath, content: String },
    Move { src: VaultPath, dest: VaultPath },
    Delete { path: VaultPath },
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
    /// Create an empty transaction bound to the given store and index.
    pub fn new(store: Arc<dyn VaultStore>, index: Arc<dyn VaultIndex>) -> Self {
        Self {
            store,
            index,
            file_ops: Vec::new(),
            index_ops: Vec::new(),
        }
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

    // ---- commit -----------------------------------------------------

    /// Apply every buffered operation. File ops run first, in
    /// enqueue order; on failure, previously-applied ops are rolled
    /// back best-effort. Index ops run only if every file op
    /// succeeded; failures collect into a single `IndexStale` error
    /// so the caller can log and move on.
    pub fn commit(self) -> Result<(), TransactionError> {
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

        // Phase 2: index ops. A failure here leaves files correct;
        // collect every error so the caller sees the full picture.
        let mut index_errors: Vec<IndexError> = Vec::new();
        for op in &self.index_ops {
            if let Err(e) = apply_index_op(&*self.index, op) {
                index_errors.push(e);
            }
        }

        if index_errors.is_empty() {
            Ok(())
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
    }
}
