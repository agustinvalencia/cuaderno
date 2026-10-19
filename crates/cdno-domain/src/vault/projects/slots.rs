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
/// against. Today every active map holds one, so the two are equal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SlotUsage {
    /// Maps with `status: active` (closed maps under `projects/_done/`
    /// are never read).
    pub active: usize,
    /// Maps that hold a slot: the number the cap is checked against.
    pub counted: usize,
    /// The configured cap (`max_active_projects`, default 5).
    pub cap: u8,
    /// Slugs of the maps counted, in index order — what
    /// [`DomainError::ProjectCapReached`] lists for the caller to pick
    /// one to park.
    pub counted_slugs: Vec<String>,
}

impl Vault {
    /// Count the active-project slots in use against the cap.
    ///
    /// Built on [`active_projects`](Self::active_projects), so a
    /// malformed project map fails the count rather than being skipped
    /// (a skipped map would let a sixth active project past the cap).
    pub fn slot_usage(&self) -> Result<SlotUsage, DomainError> {
        let active = self.active_projects()?;
        let counted_slugs: Vec<String> = active
            .iter()
            .map(|(path, _)| project_slug_from_path(path))
            .collect();
        Ok(SlotUsage {
            active: active.len(),
            counted: counted_slugs.len(),
            cap: self.config.vault.max_active_projects,
            counted_slugs,
        })
    }
}
