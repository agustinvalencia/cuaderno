//! How a closable thing ends: the one outcome type shared by every
//! closing verb (RFC 0004 §5.2).
//!
//! Actions (#559), and projects from RFC 0004 on, close one of two ways,
//! and the two are not opposites of one degree: they are different claims
//! about what happened. Each verb maps the outcome onto its own frontmatter
//! and log line; this module only names the outcome.

use crate::frontmatter::ActionStatus;

/// How a closable thing is being closed.
///
/// A named type rather than a bool because the two outcomes are different
/// claims, and #559 exists because the tooling could only make the first
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::vault) enum Closure {
    /// The work was performed.
    Completed,
    /// The work was abandoned, superseded or reprioritised.
    Dropped,
}

impl Closure {
    /// The `status` an action note is stamped with for this outcome.
    pub(in crate::vault) fn action_status(self) -> ActionStatus {
        match self {
            Closure::Completed => ActionStatus::Completed,
            Closure::Dropped => ActionStatus::Dropped,
        }
    }
}
