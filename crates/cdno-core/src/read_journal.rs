//! What a transaction saw of each file before it rewrote it (#1001).
//!
//! The vault write lock (`.cuaderno/.lock`, an `flock`) serialises writers
//! inside one kernel only. When one vault folder is written from two
//! kernels (a host and a container bind-mounting it, a network share that
//! does not forward locks), both writers hold "the" lock at once, and two
//! read-modify-writes can interleave: the second rename discards the first
//! writer's change. The store's atomic rename keeps each file well formed,
//! so nothing looks wrong afterwards.
//!
//! The defence is compare-before-rename. While a
//! [`VaultTransaction`](crate::transaction::VaultTransaction) is open, the
//! store records what it observed of every path it read — the content's
//! fingerprint, or that the file was absent — into the transaction's
//! [`ReadJournal`]. The commit then re-checks each file it is about to
//! replace, append to or delete against that observation and refuses the
//! whole commit if any differs
//! ([`TransactionError::Conflict`](crate::error::TransactionError::Conflict)).
//! This narrows the race; it does not close it (`docs/concurrent-writers.md`).
//!
//! # Why the store records, not the domain
//!
//! Domain operations read through `Vault::store` directly and stage writes
//! on the transaction; there is no read call on the transaction for them to
//! forget. Recording in the store means every read made while the
//! transaction is open is covered without any verb opting in.
//!
//! # Whose reads are recorded
//!
//! Only reads made on the thread that opened the transaction. A domain
//! operation runs on one thread from opening its transaction to committing
//! it (the MCP server runs each call on one blocking thread), while a store
//! is shared by every thread of a process: a read-only call on another
//! thread must not supply the baseline, because it may have read a newer or
//! an older version than the operation did.
//!
//! # What the baseline guarantees
//!
//! The baseline of a path is the first observation the operation made of
//! it. Every later observation it makes must agree; one that does not marks
//! the path as having changed during the operation, and the commit refuses
//! it. So a commit that passes replaced a file that held, at its last check,
//! exactly the bytes every read of the operation returned: the content it
//! composed from. A file changed and changed back to those bytes in between
//! is indistinguishable, and harmless, since what was composed from is what
//! is there.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, ThreadId};

use xxhash_rust::xxh3::xxh3_64;

use crate::error::StoreError;
use crate::path::VaultPath;

/// What a store observed at a path: absent, or present with this content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// No file at the path.
    Absent,
    /// A file whose bytes have this length and xxh3 fingerprint.
    Content { len: u64, hash: u64 },
}

impl Observed {
    /// The observation of a file holding `bytes`.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Observed::Content {
            len: bytes.len() as u64,
            hash: xxh3_64(bytes),
        }
    }

    /// The observation a read result implies: its content, or absent on
    /// [`StoreError::NotFound`]. Any other error observes nothing.
    pub fn of_read<T: AsRef<[u8]>>(result: &Result<T, StoreError>) -> Option<Self> {
        match result {
            Ok(bytes) => Some(Self::of_bytes(bytes.as_ref())),
            Err(StoreError::NotFound(_)) => Some(Observed::Absent),
            Err(_) => None,
        }
    }
}

/// What one transaction observed, path by path.
#[derive(Debug, Default)]
struct Seen {
    first: HashMap<VaultPath, Observed>,
    /// Paths a later observation disagreed with the first about.
    changed: HashSet<VaultPath>,
}

/// The observations one transaction's own thread made while it was open.
#[derive(Debug)]
pub struct ReadJournal {
    thread: ThreadId,
    seen: Mutex<Seen>,
}

impl Default for ReadJournal {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything a journal recorded, handed to the commit.
#[derive(Debug, Default)]
pub struct Recorded {
    /// The first observation of each path.
    pub first: HashMap<VaultPath, Observed>,
    /// Paths observed differently by two reads of the operation: changed
    /// underneath it while it ran.
    pub changed: HashSet<VaultPath>,
}

impl ReadJournal {
    /// A journal belonging to the calling thread.
    pub fn new() -> Self {
        Self {
            thread: thread::current().id(),
            seen: Mutex::new(Seen::default()),
        }
    }

    /// Record `observed` for `path`: the first observation is kept, and a
    /// later one that differs marks the path as changed.
    pub fn record(&self, path: &VaultPath, observed: Observed) {
        let mut seen = self.seen.lock().expect("poisoned mutex");
        match seen.first.get(path) {
            None => {
                seen.first.insert(path.clone(), observed);
            }
            Some(first) if *first != observed => {
                seen.changed.insert(path.clone());
            }
            Some(_) => {}
        }
    }

    /// Set the baseline of `path` to `observed` whatever was recorded:
    /// for a create, which states it expects the path absent.
    pub fn expect(&self, path: &VaultPath, observed: Observed) {
        let mut seen = self.seen.lock().expect("poisoned mutex");
        seen.first.insert(path.clone(), observed);
    }

    /// What was first observed at `path`, if anything.
    pub fn get(&self, path: &VaultPath) -> Option<Observed> {
        self.seen
            .lock()
            .expect("poisoned mutex")
            .first
            .get(path)
            .copied()
    }

    /// Every recorded observation, leaving the journal empty.
    pub fn take(&self) -> Recorded {
        let seen = std::mem::take(&mut *self.seen.lock().expect("poisoned mutex"));
        Recorded {
            first: seen.first,
            changed: seen.changed,
        }
    }
}

/// The journals open on one store. A store that records reads holds one
/// of these and calls [`record`](Self::record) from its read methods; a
/// transaction registers its journal for as long as it is open.
///
/// Journals are held weakly, so a transaction that drops without
/// committing stops recording with no unregister step.
#[derive(Debug, Default)]
pub struct ReadJournals {
    open: Mutex<Vec<Weak<ReadJournal>>>,
}

impl ReadJournals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a journal, owned by the calling thread, that records every
    /// read that thread makes through this store until it is dropped.
    pub fn open(&self) -> Arc<ReadJournal> {
        let journal = Arc::new(ReadJournal::new());
        let mut open = self.open.lock().expect("poisoned mutex");
        open.retain(|j| j.strong_count() > 0);
        open.push(Arc::downgrade(&journal));
        journal
    }

    /// Record what a read of `path` returned into the open journals of the
    /// calling thread. The fingerprint is computed only when there is one.
    pub fn record<T: AsRef<[u8]>>(&self, path: &VaultPath, result: &Result<T, StoreError>) {
        self.record_with(path, || Observed::of_read(result));
    }

    /// Record that `path` is absent (an `exists` that answered false).
    pub fn record_absent(&self, path: &VaultPath) {
        self.record_with(path, || Some(Observed::Absent));
    }

    fn record_with(&self, path: &VaultPath, observe: impl FnOnce() -> Option<Observed>) {
        let me = thread::current().id();
        let open = self.open.lock().expect("poisoned mutex");
        let mine: Vec<Arc<ReadJournal>> = open
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|j| j.thread == me)
            .collect();
        drop(open);
        if mine.is_empty() {
            return;
        }
        if let Some(observed) = observe() {
            for journal in mine {
                journal.record(path, observed);
            }
        }
    }
}
