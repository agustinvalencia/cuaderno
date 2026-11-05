//! The startup backfill of the masked snapshot hash (RFC 0007 T16).
//!
//! Index migration 004 added `masked_hash`, `mask_version` and
//! `masked_size` to the archival snapshots, `NULL` on every row archived
//! before it. Until a row carries them, the append-only lint can verify it
//! only through the plain hash, which a rename's reference rewrite breaks.
//! This pass fills them, once, from the frozen prefix the plain hash still
//! vouches for.

use std::sync::Arc;

use cdno_core::hash::{MASK_VERSION, content_hash, mask_references};
use cdno_core::index::ArchivalSnapshot;
use cdno_core::path::VaultPath;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;

use super::Vault;
use super::actions::SNAPSHOT_REFERENCE_KEYS;

impl Vault {
    /// Stamp the masked hash on every snapshot row that lacks one and whose
    /// plain hash still verifies against the current file, in one
    /// transaction. Runs inside [`Vault::new`], after reconciliation.
    ///
    /// The masked hash is computed over the file's first `frozen_size`
    /// bytes, the prefix the plain hash has just vouched for, with the same
    /// keys the archival stamp uses, so a backfilled row is exactly the row
    /// archiving that file today would have written. A row whose plain hash
    /// fails is left `NULL`: masking an edited prefix would bless the edit,
    /// and lint keeps reporting it as before. A row that already carries a
    /// masked hash, at whatever `mask_version`, is never touched, which makes
    /// the pass idempotent and keeps an older binary from overwriting a row
    /// a newer one stamped.
    ///
    /// The rows that would be filled are found before the lock is taken,
    /// and the lock is taken only when there is at least one; under it they
    /// are re-listed and re-verified. So a vault whose `NULL` rows can never
    /// be filled (a hand-edited prefix, or a rename that reached this clone
    /// before its first open on this build) opens without the lock, as one
    /// with nothing to backfill does.
    ///
    /// Best-effort, like reconciliation's link repair: the pass heals a
    /// cache and writes no file, so a lock timeout or a store or index error
    /// (an `IndexStale` from the commit included) is logged and the vault
    /// still opens. The rows stay as they were, every one already stamped
    /// verifies, lint still has the plain path, and the next open retries.
    /// Like reconciliation the pass is not gated on the method version.
    pub(in crate::vault) fn backfill_masked_snapshots(&self) {
        if let Err(error) = self.try_backfill_masked_snapshots() {
            tracing::warn!(
                %error,
                "could not backfill the masked hash on archival snapshots; \
                 the rows stay unstamped and the next open retries"
            );
        }
    }

    fn try_backfill_masked_snapshots(&self) -> Result<(), DomainError> {
        if self.fillable_snapshots()?.is_empty() {
            return Ok(());
        }
        let mut tx = VaultTransaction::new(Arc::clone(&self.store), Arc::clone(&self.index))?;
        let fillable = self.fillable_snapshots()?;
        if fillable.is_empty() {
            return Ok(());
        }
        let filled = fillable.len();
        for (path, stamped) in fillable {
            tx.record_archival_snapshot(path, stamped);
        }
        tx.commit()?;
        tracing::debug!(filled, "backfilled masked archival snapshot hashes");
        Ok(())
    }

    /// Every `NULL` row whose plain hash verifies, with its masked fields
    /// filled.
    fn fillable_snapshots(&self) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, DomainError> {
        Ok(self
            .index
            .list_archival_snapshots()?
            .into_iter()
            .filter(|(_, snap)| snap.masked_hash.is_none())
            .filter_map(|(path, snap)| {
                let stamped = self.stamp_masked(&path, &snap)?;
                Some((path, stamped))
            })
            .collect())
    }

    /// `snap` with its masked fields filled, or `None` when the file cannot
    /// be read or its first `frozen_size` bytes no longer match the plain
    /// hash. A file that cannot be read is skipped rather than failing the
    /// vault open: lint reports it, and the next open retries.
    fn stamp_masked(&self, path: &VaultPath, snap: &ArchivalSnapshot) -> Option<ArchivalSnapshot> {
        let content = self.store.read_file(path).ok()?;
        let prefix = content.get(..usize::try_from(snap.frozen_size).ok()?)?;
        if content_hash(prefix) != snap.frozen_hash {
            return None;
        }
        let masked = mask_references(prefix, SNAPSHOT_REFERENCE_KEYS);
        Some(ArchivalSnapshot {
            masked_hash: Some(content_hash(&masked)),
            mask_version: Some(MASK_VERSION),
            masked_size: Some(masked.len() as u64),
            ..snap.clone()
        })
    }
}
