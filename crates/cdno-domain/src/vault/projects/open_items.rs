//! What still hangs off a project when someone asks to close it (RFC 0004
//! §5.3).
//!
//! The report is computed from the project map itself: the open bullets
//! of `## Next Actions` and the open lines of `## Milestones`, parsed from
//! the [`MarkdownDocument`], plus each attached action note's status read
//! from disk. It never goes through `list_actions` (which only answers for
//! an active project) or `open_milestones` (which reads index rows that a
//! parked map may no longer have): an index-backed report on a parked map
//! would see no milestones, pass the refusal, and archive open lines
//! unlisted.
//!
//! The report is what the user is shown and what the cascade acts on, so
//! its [`OpenItemsHash`] identifies exactly that list: a caller that
//! confirmed one list cannot have a different one dropped.

use chrono::NaiveDate;
use serde::Serialize;

use cdno_core::error::ManipulationError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::hash::content_hash;
use cdno_core::markdown::{MarkdownDocument, extract_milestones_from_body};

use crate::error::DomainError;
use crate::frontmatter::{ActionFrontmatter, ActionStatus, CommitmentStatus};

use super::super::Vault;
use super::actions::{parse_attached_action_slug, parse_open_action_text};
use super::milestones::parse_open_milestone_title;
use super::{MILESTONES_SECTION, NEXT_ACTIONS_SECTION};

/// One open `- [ ]` bullet of `## Next Actions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenAction {
    /// The bullet text verbatim, energy suffix included.
    pub text: String,
    /// The attached action note's slug when the bullet is a
    /// `[[actions/<slug>]]` wikilink.
    pub note: Option<String>,
    /// The attached note's `status`, read from `actions/<slug>.md`.
    /// `None` when the bullet is plain or the note no longer exists.
    pub note_status: Option<ActionStatus>,
    /// The bullet's index in the section split on `'\n'`, the index the
    /// action verbs resolve, so a cascade removes exactly this line. Not
    /// part of the hash or the wire shape.
    #[serde(skip)]
    pub line: usize,
}

/// One open `- [ ]` line of `## Milestones`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenMilestone {
    /// The title as `drop_milestone` names it in its log line.
    pub title: String,
    /// `None` for an undated marker.
    pub date: Option<NaiveDate>,
    /// A `hard:` deadline, the only kind the commitments register reads,
    /// so the user can see which drops would retire a deadline.
    pub hard: bool,
    /// The line's index in the section split on `'\n'`, the index the
    /// milestone verbs resolve. Not part of the hash or the wire shape.
    #[serde(skip)]
    pub line: usize,
}

/// An active standalone commitment whose `project:` names the project.
/// Listed for information only: a promise to someone else does not end
/// with the project, so it is never cascaded and never blocks a close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedCommitment {
    pub slug: String,
    pub due: NaiveDate,
}

/// Everything still open on a project: actions and milestones in the
/// order they appear in the map, linked commitments by due date then path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenItemsReport {
    pub actions: Vec<OpenAction>,
    pub milestones: Vec<OpenMilestone>,
    pub untouched_commitments: Vec<LinkedCommitment>,
}

impl OpenItemsReport {
    /// Whether nothing blocks the close: no open action and no open
    /// milestone. Linked commitments do not count, since they are never
    /// cascaded and the project closes with them still open.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.milestones.is_empty()
    }

    /// How many items block the close (actions plus milestones).
    pub fn open_count(&self) -> usize {
        self.actions.len() + self.milestones.len()
    }

    /// A content hash of the whole report in its listed order. Two identical
    /// reports hash alike; adding, removing, editing or reordering any
    /// entry changes it. Commitments are part of the report, so a newly
    /// linked commitment changes the hash too.
    pub fn hash(&self) -> OpenItemsHash {
        // JSON of the derived shape is an unambiguous encoding: every
        // string is quoted and escaped, so no two reports share one.
        let encoded = serde_json::to_string(self).expect("the report serialises");
        OpenItemsHash(content_hash(&encoded))
    }
}

/// The hash a refusal hands back and a cascade must present, so the list
/// dropped is the list the caller was shown (RFC 0004 D10). Opaque: the
/// user never sees it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct OpenItemsHash(String);

impl OpenItemsHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for OpenItemsHash {
    /// Wrap a hash a caller echoed back, such as an MCP
    /// `expected_open_items` argument.
    fn from(hash: String) -> Self {
        OpenItemsHash(hash)
    }
}

impl std::fmt::Display for OpenItemsHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What `drop_project` does with open items. `complete_project` takes no
/// such choice: it refuses while any action or milestone is open (RFC 0004
/// D11).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OpenItems {
    /// Refuse with [`DomainError::ProjectHasOpenItems`]. The default on
    /// every surface.
    #[default]
    Refuse,
    /// Drop the open actions and milestones in the same transaction.
    /// `expected` is the hash of the list the caller was shown; `None`
    /// only where a human typed the request against their own vault.
    Drop { expected: Option<OpenItemsHash> },
}

impl Vault {
    /// The open items of the project map `doc` (whose slug is `slug`).
    ///
    /// An open line is picked with the same predicate the action and
    /// milestone verbs use, so the report lists exactly the lines a cascade
    /// would remove: an unnamed or indented `- [ ]` milestone is listed, not
    /// silently skipped. A milestone's `date` and `hard` come from the
    /// parser the index uses, since they describe what the register sees.
    ///
    /// A missing `## Next Actions` or `## Milestones` section contributes
    /// nothing; any other section error (a heading that appears twice)
    /// fails the call, because reading it as empty would let a map with
    /// open items close. An attached note that no longer exists reports
    /// `note_status: None`; one whose frontmatter does not parse fails the
    /// call, as does a malformed commitment anywhere in the vault, as they
    /// fail the other scans.
    pub fn open_items_report(
        &self,
        doc: &MarkdownDocument,
        slug: &str,
    ) -> Result<OpenItemsReport, DomainError> {
        let mut actions = Vec::new();
        for (index, line) in section_or_empty(doc, NEXT_ACTIONS_SECTION)?
            .split('\n')
            .enumerate()
        {
            let Some(text) = parse_open_action_text(line) else {
                continue;
            };
            let note = parse_attached_action_slug(text).map(str::to_owned);
            let note_status = match &note {
                Some(note_slug) => self.action_note_status(note_slug)?,
                None => None,
            };
            actions.push(OpenAction {
                text: text.to_owned(),
                note,
                note_status,
                line: index,
            });
        }

        let mut milestones = Vec::new();
        for (index, line) in section_or_empty(doc, MILESTONES_SECTION)?
            .split('\n')
            .enumerate()
        {
            let Some(title) = parse_open_milestone_title(line) else {
                continue;
            };
            // The index parser skips a line it cannot name; such a line is
            // still open, so it is listed, as undated and not hard.
            let indexed = extract_milestones_from_body(line).into_iter().next();
            milestones.push(OpenMilestone {
                title: title.to_owned(),
                date: indexed
                    .as_ref()
                    .and_then(|m| m.date.as_deref())
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
                hard: indexed.is_some_and(|m| m.is_hard),
                line: index,
            });
        }

        let untouched_commitments = self
            .commitments_for_project(slug)?
            .into_iter()
            .filter(|(_, c)| c.status == CommitmentStatus::Active)
            .map(|(path, c)| LinkedCommitment {
                slug: path
                    .as_path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                due: c.due,
            })
            .collect();

        Ok(OpenItemsReport {
            actions,
            milestones,
            untouched_commitments,
        })
    }

    /// The `status` of `actions/<slug>.md`, or `None` when it does not
    /// exist.
    fn action_note_status(&self, slug: &str) -> Result<Option<ActionStatus>, DomainError> {
        let path = Self::active_action_path(slug)?;
        if !self.store.exists(&path)? {
            return Ok(None);
        }
        let raw = self.store.read_file(&path)?;
        let (fm, _body) = Frontmatter::parse(&raw)?;
        Ok(Some(ActionFrontmatter::try_from(fm)?.status))
    }
}

/// The section under `heading`, or `""` when the map has no such section.
/// Every other error propagates: an ambiguous heading read as empty would
/// report nothing open on a map that has open items.
fn section_or_empty<'a>(doc: &'a MarkdownDocument, heading: &str) -> Result<&'a str, DomainError> {
    match doc.section(heading) {
        Ok(section) => Ok(section),
        Err(ManipulationError::SectionNotFound(_)) => Ok(""),
        Err(e) => Err(e.into()),
    }
}
