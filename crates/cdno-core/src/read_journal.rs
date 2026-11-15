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
//! store records the first thing it observed of every path it read — the
//! content's fingerprint, or that the file was absent — into the
//! transaction's [`ReadJournal`]. The commit then re-checks each file it is
//! about to replace, append to or delete against that observation and refuses
//! the whole commit if any differs
//! ([`TransactionError::Conflict`](crate::error::TransactionError::Conflict)).
//!
//! # Why the store records, not the domain
//!
//! Domain operations read through `Vault::store` directly and stage writes
//! on the transaction; there is no read call on the transaction for them to
//! forget. Recording in the store means every read made while the
//! transaction is open is covered without any verb opting in. A path the
//! domain staged without reading (a blind write) gets its observation when
//! the write is staged, and failing that at commit, so no staged path is
//! ever left unchecked.
//!
//! # Why the first observation wins
//!
//! A store is shared by every thread of a process, and while a transaction
//! is open a read-only call on another thread records into it too. Keeping
//! the *first* observation makes that harmless: the domain's own read comes
//! after the transaction opened, so the recorded fingerprint is either the
//! one the domain read or an older one. An older one can only make the
//! check fire when it need not (a spurious conflict, which a retry clears),
//! never let a foreign write through.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

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

/// The first observation of each path made while one transaction is open.
#[derive(Debug, Default)]
pub struct ReadJournal {
    seen: Mutex<HashMap<VaultPath, Observed>>,
}

impl ReadJournal {
    /// Record `observed` for `path` unless something was recorded already.
    pub fn record(&self, path: &VaultPath, observed: Observed) {
        let mut seen = self.seen.lock().expect("poisoned mutex");
        seen.entry(path.clone()).or_insert(observed);
    }

    /// What was first observed at `path`, if anything.
    pub fn get(&self, path: &VaultPath) -> Option<Observed> {
        self.seen.lock().expect("poisoned mutex").get(path).copied()
    }

    /// Every recorded observation, leaving the journal empty.
    pub fn take(&self) -> HashMap<VaultPath, Observed> {
        std::mem::take(&mut *self.seen.lock().expect("poisoned mutex"))
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

    /// Open a journal that records every read through this store until it
    /// is dropped.
    pub fn open(&self) -> Arc<ReadJournal> {
        let journal = Arc::new(ReadJournal::default());
        let mut open = self.open.lock().expect("poisoned mutex");
        open.retain(|j| j.strong_count() > 0);
        open.push(Arc::downgrade(&journal));
        journal
    }

    /// Record what a read of `path` returned into every open journal.
    /// The fingerprint is computed only when a journal is open.
    pub fn record<T: AsRef<[u8]>>(&self, path: &VaultPath, result: &Result<T, StoreError>) {
        self.record_with(path, || Observed::of_read(result));
    }

    /// Record that `path` is absent (an `exists` that answered false).
    pub fn record_absent(&self, path: &VaultPath) {
        self.record_with(path, || Some(Observed::Absent));
    }

    fn record_with(&self, path: &VaultPath, observe: impl FnOnce() -> Option<Observed>) {
        let open = self.open.lock().expect("poisoned mutex");
        let live: Vec<Arc<ReadJournal>> = open.iter().filter_map(Weak::upgrade).collect();
        drop(open);
        if live.is_empty() {
            return;
        }
        if let Some(observed) = observe() {
            for journal in live {
                journal.record(path, observed);
            }
        }
    }
}
