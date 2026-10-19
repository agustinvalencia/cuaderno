//! The one-time read-through of parked deadlines (RFC 0006 §7.1, §7.4,
//! §8.1; plan T12).
//!
//! RLM 1.1 keeps a parked map's hard dates in the commitments register
//! (T11), so dates that had dropped out of it come back, some of them long
//! overdue. The method asks for one pass over them on adopting 1.1: each
//! stale date is dropped, with its entry in the log, or re-dated.
//!
//! cdno supports that pass twice over. `cdno migrate` (dry run and real
//! run) lists every hard date on a parked map, overdue first, through
//! [`Vault::parked_live_dates`]. `cdno lint` warns on the **overdue** ones
//! only, and only while the vault's method version is below
//! [`READ_THROUGH_BELOW`]: once the vault records RLM 1.1 the read-through
//! is done and the rows are gone for good, because a permanent lint row on
//! parked deadlines would be the guilt engine RFC 0006 §6 P1 forbids. The
//! comparison is against the vault's version as
//! [`Vault::method_status`] reads it (unstamped reads as 1.0), never the
//! binary's: the behaviour ships with the register change whatever version
//! this cdno stamps. Nothing here reaches orientation or any MCP tool but
//! `lint`.

use chrono::NaiveDate;

use cdno_core::config::MethodVersion;
use cdno_core::error::ConfigError;
use cdno_core::path::VaultPath;

use super::Vault;
use super::commitments::{CommitmentEntry, CommitmentSource, ProjectDateCache};
use crate::error::DomainError;
use crate::frontmatter::ProjectStatus;
use crate::lint::LintIssue;
use crate::method_version::UNSTAMPED_READS_AS;

/// The method version at which the parked read-through is done: lint warns
/// on overdue parked dates only while the vault's effective version is
/// below it, and `cdno migrate` lists the parked dates only for a run that
/// starts below it.
pub const READ_THROUGH_BELOW: MethodVersion = MethodVersion { major: 1, minor: 1 };

/// The widest window the index's milestone query takes: every date.
const EVERY_DATE: (&str, &str) = ("0000-01-01", "9999-12-31");

fn is_parked(status: ProjectStatus) -> bool {
    status == ProjectStatus::Parked
}

impl Vault {
    /// Every hard date on a parked map that the register now carries:
    /// sources 1 (hard, uncompleted milestones) and 4 (an active action
    /// note's `due:` with no milestone pin) of [`Vault::commitments`],
    /// restricted to parked maps and with no date window. Overdue dates
    /// come first, then the rest, each group by date.
    ///
    /// Active and closed maps contribute nothing: an active map is read
    /// daily already, and a closed map's dates are not in the register.
    pub fn parked_live_dates(&self, today: NaiveDate) -> Result<Vec<CommitmentEntry>, DomainError> {
        Ok(self
            .parked_live_dates_with_paths(today)?
            .into_iter()
            .map(|(_path, entry)| entry)
            .collect())
    }

    /// [`Vault::parked_live_dates`], each entry with the path of the note
    /// that carries the date: the parked map for a milestone, the action
    /// note for a `due:`. Read through the register's own source readers
    /// and their cache, so the two never disagree on what a parked date is.
    pub(in crate::vault) fn parked_live_dates_with_paths(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<(VaultPath, CommitmentEntry)>, DomainError> {
        let mut cache = ProjectDateCache::default();
        let (from, to) = EVERY_DATE;
        let mut dates = self.milestone_dates(today, from, to, is_parked, &mut cache)?;
        dates.extend(self.action_due_dates(today, None, is_parked, &mut cache)?);
        dates.sort_by_key(|(_path, entry)| (!entry.is_overdue, entry.date));
        Ok(dates)
    }

    /// Lint's read-through rows: one `Warning` per overdue date on a parked
    /// map, while the vault's effective method version is below
    /// [`READ_THROUGH_BELOW`]. A version that cannot be read gives no rows
    /// here: the method-version rule reports it as an error already.
    pub(in crate::vault) fn read_through_issues(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<LintIssue>, DomainError> {
        let vault = match self.method_status() {
            Ok(status) => status.vault,
            Err(DomainError::Config(ConfigError::MethodVersion { .. })) => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        if vault.unwrap_or(UNSTAMPED_READS_AS) >= READ_THROUGH_BELOW {
            return Ok(Vec::new());
        }
        Ok(self
            .parked_live_dates_with_paths(today)?
            .into_iter()
            .filter(|(_path, entry)| entry.is_overdue)
            .map(|(path, entry)| LintIssue::warning(path, read_through_message(&entry)))
            .collect())
    }
}

/// The read-through row for one overdue parked date: what it is, that it is
/// in the register again, the fix for its source, and that the row ends
/// once the vault records RLM 1.1. It names no command for that last step.
fn read_through_message(entry: &CommitmentEntry) -> String {
    let (what, project, fix) = match &entry.source {
        CommitmentSource::ActionNote(project) => (
            format!("action `{}` (due {})", entry.title, entry.date),
            project,
            "re-date its `due:` in the note, or drop the action with `cdno action drop`",
        ),
        CommitmentSource::ProjectMilestone(project) => (
            format!("hard milestone `{}` ({})", entry.title, entry.date),
            project,
            "drop it with `cdno project milestone drop`, or re-date it on the map",
        ),
        // Sources 2 and 3 never come from a parked map.
        CommitmentSource::Stewardship(slug) | CommitmentSource::StandaloneCommitment(slug) => (
            format!("`{}` ({})", entry.title, entry.date),
            slug,
            "drop it or re-date it",
        ),
    };
    format!(
        "{what} on parked project [[{project}]] is overdue and in the commitments register again \
         (a parked map's hard dates are promises under RLM {READ_THROUGH_BELOW}): {fix}; this \
         read-through row disappears once the vault records RLM {READ_THROUGH_BELOW}"
    )
}
