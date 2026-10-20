//! `slot_usage`: the active-project cap count, in one place.
//!
//! Creation routing, the activation refusal, the CLI's closure message,
//! the monthly context, `list_projects` and the orientation snapshot all
//! read the cap through here, so the rule for what holds a slot has one
//! home (RFC 0006 §8.0).

use crate::error::DomainError;

use super::super::Vault;
use super::project_slug_from_path;

/// Active-project slot usage against the configured cap.
///
/// `active` is the number of maps with `status: active`; `counted` is
/// the number that hold a slot, which is what the cap is checked
/// against: the active maps that do not name someone else in `held_by:`
/// (RFC 0006 §7.2, [`ProjectFrontmatter::counts_toward_cap`]).
///
/// [`ProjectFrontmatter::counts_toward_cap`]: crate::frontmatter::ProjectFrontmatter::counts_toward_cap
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SlotUsage {
    /// Maps with `status: active` (closed maps under `projects/_done/`
    /// are never read).
    pub active: usize,
    /// Maps that hold a slot: the active maps naming no other holder, the
    /// number the cap is checked against.
    pub counted: usize,
    /// The configured cap (`max_active_projects`, default 5).
    pub cap: u8,
    /// Slugs of the maps counted, in index order — what
    /// [`DomainError::ProjectCapReached`] lists for the caller to pick
    /// one to park.
    pub counted_slugs: Vec<String>,
}

impl SlotUsage {
    /// The cap rule, in one place (RFC 0006 §7.2): the cap is checked
    /// whenever a map that did not count begins to count.
    ///
    /// `counts_before` and `counts_after` say whether the map being
    /// changed holds a slot before and after the change. Refuses with
    /// [`DomainError::ProjectCapReached`], listing the counted maps,
    /// exactly when the map did not count before, counts after, and the
    /// counted maps are already at the cap; every other case is allowed.
    pub fn ensure_slot(&self, counts_before: bool, counts_after: bool) -> Result<(), DomainError> {
        if counts_before || !counts_after {
            return Ok(());
        }
        if self.counted >= usize::from(self.cap) {
            return Err(DomainError::ProjectCapReached {
                current: self.counted,
                max: usize::from(self.cap),
                active_projects: self.counted_slugs.clone(),
            });
        }
        Ok(())
    }
}

impl Vault {
    /// Count the active-project slots in use against the cap.
    ///
    /// Built on [`active_projects`](Self::active_projects), so a
    /// malformed project map fails the count rather than being skipped
    /// (a skipped map would let a sixth active project past the cap).
    pub fn slot_usage(&self) -> Result<SlotUsage, DomainError> {
        let active = self.active_projects()?;
        let sentinels = &self.holder_sentinels;
        let counted_slugs: Vec<String> = active
            .iter()
            .filter(|(_, project)| project.counts_toward_cap(sentinels))
            .map(|(path, _)| project_slug_from_path(path))
            .collect();
        Ok(SlotUsage {
            active: active.len(),
            counted: counted_slugs.len(),
            cap: self.config.vault.max_active_projects,
            counted_slugs,
        })
    }

    /// The cap rule at a write site: [`SlotUsage::ensure_slot`] over the
    /// current count. Call it under the write lock, before staging
    /// anything, with whether the map being changed holds a slot before
    /// and after the change.
    pub(in crate::vault) fn ensure_slot(
        &self,
        counts_before: bool,
        counts_after: bool,
    ) -> Result<(), DomainError> {
        self.slot_usage()?.ensure_slot(counts_before, counts_after)
    }
}
