//! `Vault::rename_note` and its pure planner (RFC 0007 §5.2–§5.6, §6.2).
//!
//! A rename changes a note's slug and follows every reference to it: the
//! wikilink addresses that resolve to the note, the §3.5 frontmatter fields
//! that name it by bare slug, an `aliases:` entry on the note itself and one
//! `renamed` line in today's daily log. [`Vault::plan_rename`] turns a
//! [`VaultSnapshot`] into a [`RenamePlan`] and writes nothing;
//! [`Vault::rename_preview`] plans against the vault as it is, for the
//! person to see; [`Vault::rename_note`] plans again under the write lock
//! and writes exactly that plan, or nothing.
//!
//! **The snapshot comes from the store, not the index** (§5.4): the
//! long-running server does not reconcile between requests, and the index
//! has no edge for a bare-field reference or an ambiguous stem. Resolution
//! and referrers come from the one snapshot, so the plan cannot resolve
//! against one vault and rewrite another.
//!
//! **Exactly one [`PlannedEdit`] per path**, whatever touches it: the
//! address rewrites, the field rewrites, the alias and the log line are all
//! expressed in `before`'s byte positions and merged in ascending order, so
//! `after` is `before` with [`PlannedEdit::edits`] applied
//! ([`differs_only_by`](cdno_core::extractors::differs_only_by) holds).
//!
//! **A relink is the same planner in a mode where the note does not move**
//! (RFC 0007 §8 S8, T30): [`Vault::plan_relink`] rewrites every reference
//! that reaches the note only through a former name (one of its
//! `aliases:`), links and §3.5 fields alike, to the current slug, with no move, no alias
//! edit and a `relinked` line instead of a `renamed` one. One planner, one
//! hash, one preview and one write path serve both.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::OsStr;
use std::ops::Range;

use chrono::NaiveDateTime;

use cdno_core::error::TransactionError;
use cdno_core::extractors::{
    AddressEdit, AliasMap, WikilinkRaw, first_h1, note_link_names, resolve_wikilinks,
    rewrite_note_wikilink_addresses, target_may_name,
};
use cdno_core::frontmatter::{Frontmatter, parse_note, split_frontmatter};
use cdno_core::frontmatter_edit::{
    append_frontmatter_list_value, frontmatter_changed_only_at, frontmatter_scalar_spans,
    remove_frontmatter_list_value, rewrite_frontmatter_scalar,
};
use cdno_core::hash::content_hash;
use cdno_core::index::{ArchivalSnapshot, frontmatter_aliases};
use cdno_core::path::VaultPath;
use cdno_core::paths::{self, NoteFormat, note_format};
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::lint::frozen_prefix_verifies;
use super::log::{
    append_log_line, daily_note_path, format_log_line, format_relinked_log_entry,
    format_renamed_pair_log_entry,
};
use super::note_ref::{NoteRef, PeriodRef, RefResolution, ResolvedNote, candidate_slug};
use super::projects::stage_project_index_rows;
use super::slug::{TakenBy, taken_names_in, validate_explicit_slug};

/// The frontmatter key the renamed note records its former slugs under
/// (RFC 0007 §5.5).
const ALIASES_KEY: &str = "aliases";

/// The vault as one rename plans against, read from the store
/// ([`Vault::rename_snapshot`]).
///
/// `paths` is the note set reconciliation would index (every `.md` outside
/// `.cuaderno/`, attachment artefacts and the config `ignore` globs), and
/// what wikilinks resolve against. `aliases` holds the slug-shaped
/// `aliases:` entries those notes declare, one row per occurrence in path
/// order, as [`list_aliases`](cdno_core::index::VaultIndex::list_aliases)
/// returns them (rows rather than a map, so a shared alias stays visible to
/// the taken check while resolving to nothing). `contents` holds every
/// note's text; [`Vault::plan_rename`] parses only those that contain the
/// old slug or one of the note's aliases. `frozen` holds the archival
/// snapshot of every archived note in `paths` that has one. `ignored`
/// holds the markdown files the walk saw that the config `ignore` globs
/// exclude: no link resolves to them and none is planned, but they are
/// files on disk, so the names they hold are taken. `files` holds every
/// file the walk saw outside `.cuaderno/`, of any kind, by path only: a
/// folder note's rename (a portfolio, §5.3) moves every one of them under
/// its folder, and the non-note ones are moved, never read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultSnapshot {
    pub paths: HashSet<VaultPath>,
    pub ignored: HashSet<VaultPath>,
    pub files: BTreeSet<VaultPath>,
    pub aliases: Vec<(String, VaultPath)>,
    pub contents: BTreeMap<VaultPath, String>,
    pub frozen: BTreeMap<VaultPath, ArchivalSnapshot>,
}

/// What a rename would do, before it does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    /// The note being renamed, as resolved in the snapshot. `slug` is its
    /// current (old) slug.
    pub target: ResolvedNote,
    pub new_slug: String,
    /// The target's own move(s): `(from, to)`. The target's
    /// [`PlannedEdit`] is keyed by `from`; its `after` is written at `to`.
    pub moves: Vec<(VaultPath, VaultPath)>,
    /// One per touched path, in path order.
    pub edits: Vec<PlannedEdit>,
    pub warnings: Vec<RenameWarning>,
    /// The `renamed` entry the plan appends to today's daily note, without
    /// its `- **HH:MM**:` stamp: the head line, then the indented `was:`,
    /// `reason:` and `portfolio:` continuation lines. `None` on an empty
    /// plan. What a preview shows for the log, instead of the daily note's
    /// byte edit, which may be the whole scaffolded note.
    pub log_line: Option<String>,
    /// The plan is a relink ([`Vault::plan_relink`]): `moves` is empty,
    /// `new_slug` is the note's current slug, and the log line is a
    /// `relinked` one.
    pub relink: bool,
    /// [`content_hash`] over the moves and every edit's `after`, in path
    /// order, with the log line's time stamp left out (§6.2). The write
    /// refuses a plan whose hash moved.
    pub hash: String,
}

impl RenamePlan {
    /// References rewritten (links plus fields) and the notes holding them:
    /// the totals the preview's last line and the tools' `references` /
    /// `notes` report, computed here so every interface says the same.
    pub fn reference_totals(&self) -> (usize, usize) {
        let mut references = 0;
        let mut notes = 0;
        for edit in &self.edits {
            let here: usize = edit
                .changes
                .iter()
                .map(|c| match c {
                    Change::Address { count } => *count,
                    Change::Field { .. } => 1,
                    Change::Alias | Change::LogLine => 0,
                })
                .sum();
            references += here;
            notes += usize::from(here > 0);
        }
        (references, notes)
    }

    /// Whether the plan writes nothing (a rename to the current slug, or a
    /// relink with no straggler to rewrite).
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty() && self.edits.is_empty()
    }
}

/// The one rewrite of one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEdit {
    pub path: VaultPath,
    /// The file as the snapshot holds it; empty for today's daily note when
    /// it does not exist yet.
    pub before: String,
    pub after: String,
    /// What kinds of reference this edit touches, in the order address,
    /// field, alias, log line.
    pub changes: Vec<Change>,
    /// Every byte edit, ranges into `before`, ascending and non-overlapping:
    /// `after` is exactly `before` with these applied.
    pub edits: Vec<AddressEdit>,
}

/// One kind of change in a [`PlannedEdit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// `count` wikilink addresses rewritten.
    Address { count: usize },
    /// A §3.5 field under `key` rewritten.
    Field { key: String },
    /// The old slug appended to the target's `aliases:` (and the new slug
    /// dropped from it, when the note takes back a former name).
    Alias,
    /// The `renamed` (or `relinked`) line appended to today's daily log.
    LogLine,
}

/// What [`Vault::rename_note`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOutcome {
    /// The plan written: the one built under the lock, whose `hash` equals
    /// the `expected_plan` the write was given.
    pub plan: RenamePlan,
    /// Every path written, the moved note's at its new path first, then
    /// (for a portfolio) every other file of its folder at its new path in
    /// path order, then the plan's other edits in path order. The old
    /// paths, deleted or moved away from, are the `from`s of `plan.moves`.
    /// Empty for a no-op plan.
    pub written: Vec<VaultPath>,
    /// The files are written but an index update failed: the index is
    /// stale for some of them until the next open reconciles it. The
    /// rename itself succeeded.
    pub index_stale: bool,
}

/// Something the person should know before writing the plan, that does not
/// stop it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameWarning {
    /// An archived referrer's frozen prefix already failed its archival
    /// snapshot (neither hash verifies) before this rename: it is still
    /// rewritten, and the rename neither repairs nor hides it (§5.4).
    FrozenPrefixAlreadyDiverged { path: VaultPath },
    /// The old slug is already an alias of `holder`, another note: once the
    /// renamed note lists it too, the alias is shared and resolves to
    /// nothing, so the old links it was meant to keep working (in an ignored
    /// file, on another clone, typed later) dangle.
    OldSlugAlreadyAnAlias { holder: VaultPath },
}

impl std::fmt::Display for RenameWarning {
    /// The sentence every interface shows for the warning.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenameWarning::FrozenPrefixAlreadyDiverged { path } => write!(
                f,
                "{path} already failed its archival check before this rename; it is rewritten anyway and stays flagged"
            ),
            RenameWarning::OldSlugAlreadyAnAlias { holder } => write!(
                f,
                "the old slug is already an alias of {holder}; once this note lists it too the alias is shared and resolves to nothing"
            ),
        }
    }
}

/// What a call of the rename verb asks of the note's slug: a new one, or
/// none, to relink the stragglers to the current one (T30). The two are
/// exclusive: a relink keeps the slug, so a relink request carries no new
/// slug, and [`RenameRequest::from_args`] refuses one that does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameRequest {
    /// Rename to this slug ([`Vault::rename_preview`], [`Vault::rename_note`]).
    To(String),
    /// Keep the slug and rewrite the stragglers ([`Vault::relink_preview`],
    /// [`Vault::relink_note`]).
    Relink,
}

impl RenameRequest {
    /// The request an interface's two arguments make: `to` (the new slug)
    /// and the `relink` switch. Both given is
    /// [`DomainError::RelinkTakesNoSlug`]; neither is `Ok(None)`, for the
    /// interface to prompt for or demand `to` as it does.
    pub fn from_args(to: Option<String>, relink: bool) -> Result<Option<Self>, DomainError> {
        match (to, relink) {
            (Some(to), true) => Err(DomainError::RelinkTakesNoSlug { to }),
            (None, true) => Ok(Some(Self::Relink)),
            (Some(to), false) => Ok(Some(Self::To(to))),
            (None, false) => Ok(None),
        }
    }
}

/// The planner's mode: a rename to a new slug, or a relink.
#[derive(Debug, Clone, Copy)]
enum Mode<'a> {
    Rename(&'a str),
    Relink,
}

impl Vault {
    /// Resolve the `reference` a rename names, as
    /// [`resolve_note_ref`](Self::resolve_note_ref) does, with one addition:
    /// an ambiguity whose hits include exactly one the rename accepts (see
    /// [`is_renamable`](Self::is_renamable)) resolves to that one. A question
    /// and its same-slug portfolio share a stem; the portfolio is renamed
    /// through the question, so the question is the answer. The flag is true
    /// when that narrowing chose for the caller, which an interactive
    /// interface confirms as it would a pick. Every other outcome, including
    /// an ambiguity with none or several renamable hits, is returned as
    /// `resolve_note_ref` gave it.
    pub fn resolve_rename_target(
        &self,
        reference: &str,
        today: chrono::NaiveDate,
    ) -> Result<(RefResolution, bool), DomainError> {
        match self.resolve_note_ref(reference, today)? {
            RefResolution::Ambiguous(hits) => {
                let kept: Vec<_> = hits
                    .iter()
                    .filter(|c| self.is_renamable(&c.path, &c.note_type))
                    .collect();
                if let [only] = kept.as_slice() {
                    let note = ResolvedNote::direct(only.path.clone());
                    return Ok((RefResolution::Resolved(note), true));
                }
                Ok((RefResolution::Ambiguous(hits), false))
            }
            other => Ok((other, false)),
        }
    }

    /// Whether the note at `path`, of index type `note_type`, is one the
    /// rename verb accepts (RFC 0007 §5.2): the same check the planner makes
    /// first, so an interface offering notes to rename (a picker) offers
    /// exactly the ones [`plan_rename`](Self::plan_rename) does not refuse
    /// as [`DomainError::NoteNotRenamable`]. A portfolio is renamable only
    /// when no question shares its slug (it is renamed through the
    /// question), which this checks against the store.
    pub fn is_renamable(&self, path: &VaultPath, note_type: &str) -> bool {
        let mut snapshot = VaultSnapshot::default();
        if note_type == "portfolio"
            && let Some(slug) = candidate_slug(path)
        {
            for dir in [paths::QUESTIONS_RESEARCH, paths::QUESTIONS_LIFE] {
                // Markdown on purpose: questions are machine-written Markdown.
                if let Ok(question) = VaultPath::new(format!("{dir}/{slug}.md"))
                    && self.store.exists(&question).unwrap_or(false)
                {
                    snapshot.files.insert(question);
                }
            }
        }
        renamable_type(self, path, Some(note_type), &snapshot, false).is_ok()
    }

    /// Read the vault into a [`VaultSnapshot`] for [`plan_rename`](Self::plan_rename):
    /// every note file from the store (§5.4: the index is not trusted for
    /// planning), the aliases they declare, and the archival snapshots of
    /// the archived ones.
    ///
    /// The note set is reconciliation's (the classifier's): notes outside `.cuaderno/`,
    /// minus attachment artefacts and the config `ignore` globs, so a link
    /// resolves here exactly as the index would resolve it. Every note is
    /// read (the substring prefilter needs the text); only notes whose text
    /// mentions `aliases` have their frontmatter parsed here.
    ///
    /// The caller decides when: the preview takes it without the lock, the
    /// write under it (T20).
    pub fn rename_snapshot(&self) -> Result<VaultSnapshot, DomainError> {
        let classified = cdno_core::classify::classify(&self.store, &self.note_scope()?)?;

        let mut snapshot = VaultSnapshot {
            ignored: classified
                .skipped
                .iter()
                .filter(|s| s.reason == cdno_core::classify::SkipReason::Ignored)
                .map(|s| s.path.clone())
                .collect(),
            files: classified.files,
            ..VaultSnapshot::default()
        };
        for path in classified.notes {
            let content = self.store.read_file(&path)?;
            if content.contains(ALIASES_KEY)
                && let Some(frontmatter) = frontmatter_json(&path, &content)
            {
                for alias in frontmatter_aliases(&frontmatter) {
                    snapshot.aliases.push((alias, path.clone()));
                }
            }
            snapshot.paths.insert(path.clone());
            snapshot.contents.insert(path, content);
        }
        for (path, row) in self.index.list_archival_snapshots()? {
            if snapshot.paths.contains(&path) {
                snapshot.frozen.insert(path, row);
            }
        }
        Ok(snapshot)
    }

    /// Plan renaming the note `target` names to `new_slug`, against
    /// `snapshot` (RFC 0007 §5.2–§5.4). Writes nothing.
    ///
    /// `target` is a note reference as `cdno open` takes one (a path, a
    /// bare slug, `type:slug`, a former slug), resolved in the snapshot.
    /// Refusals come in this order, each before any content is planned:
    ///
    /// 1. [`DomainError::NoteNotRenamable`] — a type outside the v1 set of
    ///    §5.2 (`type <name>`), an archived action note (`archived`), a
    ///    folder note other than a portfolio's `portfolios/<slug>/_index.md`
    ///    (`folder`), a portfolio whose slug a question shares (`rename the
    ///    question`), or an `aliases:` value the old slug cannot be appended
    ///    to;
    /// 2. [`DomainError::InvalidSlug`] — `new_slug` breaks the slug rule;
    /// 3. [`DomainError::SlugTaken`] — another note's stem, folder or
    ///    alias, in the snapshot (one of the note's own aliases is not
    ///    taken: the rename takes it back and drops it from the list);
    /// 4. [`DomainError::StemShared`] — another note has the same file stem
    ///    (a folder note named like it does not: it is addressed by path);
    ///    for a portfolio, another folder note has the same folder name (a
    ///    flat note named like the folder does not);
    /// 5. then a rename to the current slug is `Ok` with an empty plan.
    ///
    /// Two refusals come from the referrers themselves, while planning and
    /// before any plan is returned: [`DomainError::ReferenceUnaddressable`]
    /// and [`DomainError::ReferenceDrift`]. [`DomainError::RenameEditOverlap`]
    /// is an internal invariant, not a refusal.
    ///
    /// Then every note mentioning the old slug (or one of the note's
    /// aliases) is planned: each wikilink whose full target (anchor
    /// included) resolves to the note in this snapshot has its address's
    /// last segment rewritten; each §3.5 field whose target type is the
    /// note's has a value equal to the old slug rewritten, in its own
    /// quoting ([`DomainError::ReferenceUnaddressable`] when the parser
    /// reads the old slug under such a key but the text rewrite finds no
    /// value to rewrite); and the frontmatter after the field rewrite must
    /// differ from the frontmatter after the address rewrite only at those
    /// keys ([`DomainError::ReferenceDrift`] otherwise). The note gains the
    /// old slug in `aliases:`, and today's daily note the `renamed` line
    /// (last in its log, `reason` on a continuation when given).
    ///
    /// **A portfolio moves its whole folder** (§5.3). `moves` holds the
    /// `_index.md` first and then every other file under `portfolios/<old>/`
    /// in path order, each to the same place under `portfolios/<new>/`,
    /// hidden files included (a hidden note is a note; Finder's `.DS_Store`
    /// describes the folder that is moving). Every wikilink
    /// whose address is `portfolios/<old>` or begins `portfolios/<old>/` has
    /// that folder segment rewritten, whether it names a note or an
    /// attachment, wherever it sits (inside the folder too); a link that
    /// reaches the `_index.md` some other way (an alias) is rewritten to
    /// `portfolios/<new>`, the address a folder note answers to. The
    /// evidence notes' `portfolio:` fields are §3.5 fields of the
    /// portfolio's own type and follow as for any other rename. The hash
    /// covers each move by its paths and by whether it is a note's (written
    /// and deleted) or a file's (moved): a non-note file's bytes are never
    /// read.
    ///
    /// **A question moves its portfolio with it** (§5.3) when a portfolio
    /// whose folder note `portfolios/<slug>/_index.md` shares its slug exists:
    /// `moves` holds the question's flat move, then the portfolio's as above,
    /// all under the one `hash`, and the `renamed` line gains an indented
    /// `portfolio: [[portfolios/<new>]]` continuation after `was:` and `reason:`.
    /// The evidence notes' `portfolio:` fields follow; without the pair a
    /// `portfolio:` value equal to the question's slug names nothing that
    /// moves and is left alone.
    ///
    pub fn plan_rename(
        &self,
        snapshot: &VaultSnapshot,
        target: &str,
        new_slug: &str,
        reason: Option<&str>,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        self.plan(snapshot, target, Mode::Rename(new_slug), reason, at)
    }

    /// Plan relinking the note `target` names (RFC 0007 §8 S8, T30):
    /// [`plan_rename`](Self::plan_rename) in the mode where the note does
    /// not move. Writes nothing.
    ///
    /// The plan is built from the note's **aliases** instead of its old
    /// slug: it rewrites every reference that reaches the note only through
    /// a former name, links and §3.5 fields alike. Every wikilink that
    /// resolves to the note **only through one of its aliases** (by the
    /// index's own resolver, the stragglers `cdno lint` warns about) has its
    /// address's last segment rewritten to
    /// the current slug (a portfolio's to its folder form,
    /// `portfolios/<slug>`), and only when the rewritten address resolves to
    /// the note too, so a reference can only ever move to where it already
    /// points. A link the stem rule or a path resolves is left alone,
    /// wherever it points; so is one whose own folder form would resolve
    /// elsewhere (a folder-qualified straggler the alias tier reaches across
    /// folders, #913).
    ///
    /// A §3.5 field of the note's own type whose value is one of the note's
    /// aliases (never one of its current names), and that alias resolves to
    /// the note, is rewritten to the current slug; lint does not report
    /// these, since a field is not a link. This is the one mode where that is right: a rename
    /// leaves such a field alone, since a field is read by its bare value
    /// and a value equal to an alias was already detached from the note
    /// before the rename; a relink is the repair that reattaches it.
    ///
    /// No move, no `aliases:` edit (D5: the aliases stay listed, so an old
    /// link elsewhere keeps working), and a `relinked [[<new>]] — <title>`
    /// line with an indented `references: <n>` continuation and no `was:`.
    /// With no straggler the plan is empty and logs nothing. The refusals
    /// are the rename's that apply to a note that keeps its slug, in its
    /// order: [`DomainError::NoteNotRenamable`] (the same types and the same
    /// reasons but `archived`: an archived action note may be relinked,
    /// since nothing on it changes),
    /// [`DomainError::StemShared`] (a bare `[[<slug>]]` would resolve to
    /// neither note), then the referrers' own.
    pub fn plan_relink(
        &self,
        snapshot: &VaultSnapshot,
        target: &str,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        self.plan(snapshot, target, Mode::Relink, None, at)
    }

    /// The one planner behind [`plan_rename`](Self::plan_rename) and
    /// [`plan_relink`](Self::plan_relink).
    fn plan(
        &self,
        snapshot: &VaultSnapshot,
        target: &str,
        mode: Mode<'_>,
        reason: Option<&str>,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        let alias_map = AliasMap::from_rows(snapshot.aliases.iter().cloned());
        let resolved = self.resolve_in_snapshot(snapshot, target, at)?;
        let target_path = resolved.path.clone();
        let old_slug = resolved.slug.clone();
        let relink = matches!(mode, Mode::Relink);
        // A relink "renames" the note to the slug it already has.
        let new_slug_owned = match mode {
            Mode::Rename(slug) => slug.to_owned(),
            Mode::Relink => old_slug.clone(),
        };
        let new_slug = new_slug_owned.as_str();
        let target_before = snapshot
            .contents
            .get(&target_path)
            .ok_or_else(|| cdno_core::error::StoreError::NotFound(target_path.to_string()))?;
        let target_type = note_type_name(target_before);

        // 1. Not renamable. A relink edits no alias, so it cannot be
        // refused for its `aliases:` shape, and it changes nothing on the
        // target, so an archived action note may be relinked.
        let builtin = renamable_type(self, &target_path, target_type.as_deref(), snapshot, relink)?;
        let alias_edit = if relink {
            None
        } else {
            alias_edit(target_before, &old_slug, new_slug)
                .map_err(|reason| not_renamable(&target_path, reason))?
        };

        // 2. Invalid slug. A relink keeps the note's slug, whatever rule it
        // was made under.
        if !relink {
            validate_explicit_slug(new_slug).map_err(|why| DomainError::InvalidSlug {
                slug: new_slug.to_owned(),
                why,
            })?;
        }

        // 3. Taken: against the snapshot's stems, folder names and the
        // aliases of every **other** note (D5: a name is reserved while it
        // is listed). The note's own aliases are not taken: taking a former
        // name back moves no referent, since `[[old]]` resolved to this
        // note through the alias and resolves to it through the stem
        // afterwards; the alias edit drops it from the list. The current
        // slug is the note's own name, not a taken one: a rename to it is
        // the shared-stem check and then the no-op below.
        //
        // Ignored markdown files count too: no link reaches them, but the
        // name is about to become a file beside them. Names compare
        // exactly, as resolution and `unique_slug` compare them. The case
        // risk (a filesystem that cannot tell `Other.md` from `other.md`)
        // is the store's to answer, and `ensure_destinations_free` asks it.
        let mut sorted: Vec<VaultPath> = snapshot.paths.iter().cloned().collect();
        sorted.sort();
        let mut on_disk: Vec<VaultPath> = sorted
            .iter()
            .cloned()
            .chain(snapshot.ignored.iter().cloned())
            .collect();
        on_disk.sort();
        let taken = taken_names_in(
            on_disk.iter().cloned(),
            snapshot
                .aliases
                .iter()
                .filter(|(_, holder)| *holder != target_path)
                .cloned(),
        );
        if new_slug != old_slug
            && let Some((holder, kind)) = taken.get(new_slug)
        {
            return Err(DomainError::SlugTaken {
                slug: new_slug.to_owned(),
                holder: holder.clone(),
                kind: *kind,
            });
        }

        // 4. Shared stem: another note with the same **file stem**, the only
        // name the bare-link rule matches on, so the only way a link to the
        // note can resolve to neither. A folder note named like it is
        // addressed by its folder path and is not a shared stem. This check
        // is about links, so it mirrors resolution: exact stems, over the
        // indexed paths only. Resolution neither folds case nor sees an
        // ignored file, so a case variant or an ignored file elsewhere
        // cannot make a link to the note resolve to neither.
        //
        // A folder note's file stem is `_index`, which every folder note
        // shares, and its name is its folder's: so for a portfolio the
        // comparison is among folder notes, by folder name, and a flat note
        // named like the folder is the taken check's business. For a flat
        // note it is among flat notes, by file stem. The two never mix.
        let old_folder = portfolio_folder(&target_path);
        let target_stem = target_path.as_path().file_stem();
        let others: Vec<VaultPath> = sorted
            .iter()
            .filter(|p| **p != target_path)
            .filter(|p| match &old_folder {
                Some(_) => is_folder_note(p) && candidate_slug(p).as_deref() == Some(&old_slug),
                None => !is_folder_note(p) && p.as_path().file_stem() == target_stem,
            })
            .cloned()
            .collect();
        if !others.is_empty() {
            return Err(DomainError::StemShared {
                slug: old_slug,
                path: target_path,
                others,
            });
        }

        // 5. No-op. (A relink is never one here: its new slug is its old
        // one by construction, and whether it is empty depends on the
        // stragglers.)
        let empty_plan = |resolved: ResolvedNote| RenamePlan {
            target: resolved,
            new_slug: new_slug.to_owned(),
            moves: Vec::new(),
            edits: Vec::new(),
            warnings: Vec::new(),
            log_line: None,
            relink,
            hash: content_hash(""),
        };
        if !relink && new_slug == old_slug {
            return Ok(empty_plan(resolved));
        }

        // The portfolio folder that moves with the note: its own, for a
        // portfolio; for a question, the same-slug portfolio's (§5.3), only
        // when that folder note exists. The question's flat move comes first,
        // then the portfolio's `_index.md`, then the folder's other files.
        // Nothing moves in a relink.
        let pair_folder: Option<VaultPath> = (!relink && builtin == Some(NoteType::Question))
            .then(|| question_portfolio_index(&target_path, snapshot))
            .flatten()
            .and_then(|index| portfolio_folder(&index));
        let moved_folder: Option<VaultPath> = if relink {
            None
        } else {
            old_folder.clone().or(pair_folder.clone())
        };

        // The folder segment a portfolio's links carry, before and after.
        let folder_segments: Option<(String, String)> = moved_folder
            .as_ref()
            .map(|old| (old.to_string(), format!("{}/{new_slug}", paths::PORTFOLIOS)));
        let new_path = match &old_folder {
            _ if relink => target_path.clone(),
            Some(old_dir) => rebase(
                &target_path,
                old_dir,
                &VaultPath::new(format!("{}/{new_slug}", paths::PORTFOLIOS))?,
            )?,
            None => sibling_path(&target_path, new_slug)?,
        };
        let mut moves = if relink {
            Vec::new()
        } else {
            vec![(target_path.clone(), new_path.clone())]
        };
        if let Some(old_dir) = &moved_folder {
            let new_dir = VaultPath::new(format!("{}/{new_slug}", paths::PORTFOLIOS))?;
            // Markdown on purpose: a portfolio's folder note is `_index.md`.
            let index = old_dir.as_path().join("_index.md");
            if pair_folder.is_some() {
                let index = VaultPath::new(index)?;
                moves.push((index.clone(), rebase(&index, old_dir, &new_dir)?));
            }
            for file in &snapshot.files {
                if moves.iter().any(|(from, _)| from == file)
                    || !file.as_path().starts_with(old_dir.as_path())
                {
                    continue;
                }
                moves.push((file.clone(), rebase(file, old_dir, &new_dir)?));
            }
        }

        // The old slug already an alias of another note: once the note lists
        // it too, the alias is shared and resolves to nothing, so it rescues
        // no straggler. Worth saying; not worth refusing (lint reports the
        // shared alias). A relink adds no alias, so it has nothing to say.
        let mut warnings = Vec::new();
        let mut alias_holders: Vec<&VaultPath> = Vec::new();
        for (alias, holder) in &snapshot.aliases {
            if !relink
                && *alias == old_slug
                && *holder != target_path
                && !alias_holders.contains(&holder)
            {
                alias_holders.push(holder);
            }
        }
        warnings.extend(alias_holders.into_iter().map(|holder| {
            RenameWarning::OldSlugAlreadyAnAlias {
                holder: holder.clone(),
            }
        }));

        // The names a link to the note can end on: its stem, its folder,
        // and every alias it declares. A cheap gate before resolution, and
        // the prefilter's needles.
        let own_aliases: Vec<&str> = snapshot
            .aliases
            .iter()
            .filter(|(_, holder)| holder == &target_path)
            .map(|(alias, _)| alias.as_str())
            .collect();
        let own_names = note_link_names(&target_path);
        let mut names: HashSet<&str> = own_names.iter().copied().collect();
        names.extend(own_aliases.iter().copied());
        // The aliases a relink follows: the note's own that the resolver
        // reaches it through (no stem shadows it, no other note shares it).
        // Links are judged by the resolver alone, so a bare `[[new]]` to a
        // portfolio listing its own folder name `new` (lint's straggler, the
        // stem being `_index`) is rewritten to its folder form, a real
        // change. A field is read by its text, so the note's current names
        // are never field values to rewrite: `portfolio: new` already names
        // the portfolio, and rewriting `new` to `new` is no change.
        let alias_names: HashSet<&str> = own_aliases.iter().copied().collect();
        let effective_aliases: Vec<&str> = own_aliases
            .iter()
            .copied()
            .filter(|alias| *alias != new_slug && !own_names.contains(alias))
            .filter(|alias| {
                resolve_with_alias(alias, snapshot, &alias_map) == Some((target_path.clone(), true))
            })
            .collect();
        // The address a folder note answers to: its folder path, never a
        // bare name, since the folder form is unambiguous and `[[new]]`
        // can be taken by a note with that file name or an alias. After a rename it is the new
        // folder; in a relink, the folder the note already has.
        let folder_form: Option<String> = match (&old_folder, &folder_segments) {
            (Some(folder), _) if relink => Some(folder.to_string()),
            (Some(_), Some((_, new))) => Some(new.clone()),
            _ => None,
        };
        // For a portfolio, an address under its folder (`portfolios/old`,
        // `portfolios/old/2026-10-12-baseline`, an attachment's
        // `portfolios/old/<stem>/<file>`) with only that segment moved. None
        // for any other address, and for every address of a flat rename.
        let under_folder = |address: &str| -> Option<String> {
            let (old, new) = folder_segments.as_ref()?;
            let rest = address.strip_prefix(old.as_str())?;
            (rest.is_empty() || rest.starts_with('/')).then(|| format!("{new}{rest}"))
        };
        let rewrite = |address: &str| {
            if let Some(moved) = under_folder(address) {
                return moved;
            }
            // A link reaching the portfolio another way (through an alias, or
            // a bare link by its folder name).
            if let Some(folder) = &folder_form {
                return folder.clone();
            }
            match address.rsplit_once('/') {
                Some((folder, _)) => format!("{folder}/{new_slug}"),
                None => new_slug.to_owned(),
            }
        };
        let names_target = |raw: &str| {
            let address = raw.split_once('#').map_or(raw, |(address, _)| address);
            if relink {
                // A straggler: resolves to the note through one of its
                // aliases (the index's rule, so lint's straggler set), and
                // its rewrite resolves to the note too. A link that reaches
                // the note by path or stem is not one, and neither is a
                // link the stem rule sends elsewhere. Load-bearing: the
                // resolution to the target (without it `[[older]]`, another
                // note's stem, would move onto this note) and the rewrite
                // check (#913). `via_alias` adds little beside the alias-name
                // gate, but it is lint's definition, so it stays.
                return target_may_name(raw, &alias_names)
                    && resolve_with_alias(raw, snapshot, &alias_map)
                        == Some((target_path.clone(), true))
                    && resolve_in(&rewrite(address.trim_end()), snapshot, &alias_map).as_ref()
                        == Some(&target_path);
            }
            under_folder(address.trim_end()).is_some()
                || (target_may_name(raw, &names)
                    && resolve_in(raw, snapshot, &alias_map).as_ref() == Some(&target_path))
        };
        let field_keys = field_keys_for(builtin, pair_folder.is_some());
        // The values a §3.5 field may hold to be rewritten: the old slug in
        // a rename; in a relink, each alias the note is reached through.
        let field_values: Vec<&str> = if relink {
            effective_aliases.clone()
        } else {
            vec![old_slug.as_str()]
        };
        let title = note_title(&target_path, target_before).unwrap_or_else(|| old_slug.clone());
        // The log line's link, in the form the type's other log lines use.
        let link = {
            let path_link = paths::note_address(&new_path);
            match builtin {
                Some(NoteType::Project) => new_slug.to_owned(),
                Some(NoteType::Action) => format!("{}/{new_slug}", paths::ACTIONS),
                Some(NoteType::Portfolio) => format!("{}/{new_slug}", paths::PORTFOLIOS),
                _ => path_link,
            }
        };

        let daily_path = daily_note_path(at.date())?;
        let mut edits: BTreeMap<VaultPath, PlannedEdit> = BTreeMap::new();
        let mut hashed: BTreeMap<VaultPath, String> = BTreeMap::new();

        // The references first, every path; then the log line, which a
        // relink composes from their count.
        for (path, before) in &snapshot.contents {
            let is_target = *path == target_path;
            let is_daily = *path == daily_path;
            // A Typst source may write the slug with escapes (`bias\_split`), which a raw byte
            // search misses; its scanner decodes, so no prefilter applies.
            let mentions = note_format(path) == Some(NoteFormat::Typst)
                || before.contains(old_slug.as_str())
                || own_aliases.iter().any(|alias| before.contains(alias));
            if !(mentions || is_target || is_daily) {
                continue;
            }

            let (_, mut link_edits) =
                rewrite_note_wikilink_addresses(path, before, names_target, rewrite);
            // A link that already reads the new slug (a straggler through
            // an own alias the note now takes back) is not a change. Rename
            // only: a relink's predicate never yields a byte-identical link
            // edit (a flat note's own slug resolves by stem, never through
            // the alias rule; a folder note's own name rewrites to its folder
            // form, a real change), and its own names are kept out of the
            // field values by `effective_aliases`. Keeping the relink's edits
            // unfiltered is what lets a test see a predicate that rewrites a
            // link reaching the note directly.
            if !relink {
                link_edits.retain(|e| before[e.range.clone()] != e.replacement);
            }
            let after_links = apply(before, &link_edits);
            let mut all = link_edits.clone();
            let mut changes = Vec::new();
            if !link_edits.is_empty() {
                changes.push(Change::Address {
                    count: link_edits.len(),
                });
            }

            // §3.5 fields, found in `before`'s frontmatter block (a field
            // value equal to the old slug holds no wikilink, so the address
            // rewrite left it where it was) and shifted to `before`'s bytes.
            let mut guarded: Vec<&str> = Vec::new();
            let block =
                split_frontmatter(before).map(|(block, _)| (offset_in(before, block), block));
            if let Some((base, block)) = block {
                let referrer = note_type_name(before).and_then(|t| t.parse::<NoteType>().ok());
                for key in referrer
                    .map(NoteType::reference_fields)
                    .unwrap_or_default()
                    .iter()
                    .filter(|(_, ty)| field_keys.contains(ty))
                    .map(|(key, _)| *key)
                {
                    let field_edits = field_values
                        .iter()
                        .map(|value| rewrite_frontmatter_scalar(block, key, value, new_slug).1)
                        .find(|edits| !edits.is_empty())
                        .unwrap_or_default();
                    if field_edits.is_empty() {
                        // No scalar site, but the parser (which every reader
                        // goes through) may still read the value there:
                        // a block or folded scalar, a value on the next line,
                        // a tag. Refuse rather than leave the note attached
                        // to a name that no longer exists.
                        if markdown_frontmatter_json(before).is_some_and(|fm| {
                            fm.get(key)
                                .and_then(|v| v.as_str())
                                .is_some_and(|v| field_values.contains(&v))
                        }) {
                            return Err(DomainError::ReferenceUnaddressable {
                                path: path.clone(),
                                key: key.to_owned(),
                            });
                        }
                        continue;
                    }
                    all.extend(field_edits.into_iter().map(|e| shift(e, base)));
                    changes.push(Change::Field {
                        key: key.to_owned(),
                    });
                    guarded.push(key);
                }
                if is_target && let Some(edit) = &alias_edit {
                    all.push(shift(edit.clone(), base));
                    changes.push(Change::Alias);
                    guarded.push(ALIASES_KEY);
                }
            }
            all.sort_by_key(|e| (e.range.start, e.range.end));
            if all.windows(2).any(|w| w[1].range.start < w[0].range.end) {
                return Err(DomainError::RenameEditOverlap { path: path.clone() });
            }
            let after = apply(before, &all);

            // The drift guard: past the address rewrite, only the fields
            // just rewritten (and the alias) may differ.
            if !guarded.is_empty() {
                let links_block = split_frontmatter(&after_links).map_or("", |(b, _)| b);
                let after_block = split_frontmatter(&after).map_or("", |(b, _)| b);
                frontmatter_changed_only_at(links_block, after_block, &guarded).map_err(
                    |drift| DomainError::ReferenceDrift {
                        path: path.clone(),
                        drift,
                    },
                )?;
            }

            // Today's daily note is kept even with no reference in it: the
            // log line lands there below.
            if changes.is_empty() && !is_daily {
                continue;
            }
            if !changes.is_empty()
                && path.as_path().starts_with(paths::ACTIONS_DONE)
                && let Some(row) = snapshot.frozen.get(path)
                && !frozen_prefix_verifies(before, row)
            {
                warnings.push(RenameWarning::FrozenPrefixAlreadyDiverged { path: path.clone() });
            }
            edits.insert(
                path.clone(),
                PlannedEdit {
                    path: path.clone(),
                    before: before.clone(),
                    after,
                    changes,
                    edits: all,
                },
            );
        }

        let references: usize = edits
            .values()
            .flat_map(|e| &e.changes)
            .map(|c| match c {
                Change::Address { count } => *count,
                Change::Field { .. } => 1,
                Change::Alias | Change::LogLine => 0,
            })
            .sum();
        // A relink with no straggler writes nothing, not even its log line.
        if relink && references == 0 {
            return Ok(empty_plan(resolved));
        }

        // The entry: `renamed` (T19's formatter), a question renamed with
        // its portfolio gaining the `portfolio:` continuation; or
        // `relinked`, with the count and no `was:`.
        let entry = if relink {
            format_relinked_log_entry(&link, &title, references)
        } else {
            let portfolio = pair_folder
                .is_some()
                .then(|| format!("{}/{new_slug}", paths::PORTFOLIOS));
            format_renamed_pair_log_entry(&link, &title, &old_slug, reason, portfolio.as_deref())
        };
        let line = format_log_line(at.time(), &entry);
        let anchor = self.daily_anchor_section()?;
        for edit in edits.values_mut() {
            if edit.path == daily_path {
                let logged = append_log_line(edit.after.clone(), &line, &anchor)?;
                edit.edits = fold_in(&edit.edits, &edit.after, &logged);
                hashed.insert(edit.path.clone(), format!("{}\u{0}{entry}", edit.after));
                edit.after = logged;
                edit.changes.push(Change::LogLine);
            } else {
                hashed.insert(edit.path.clone(), edit.after.clone());
            }
        }

        // Today's daily note does not exist yet: it is scaffolded, and the
        // whole of it is the one insertion.
        if !snapshot.contents.contains_key(&daily_path) {
            let scaffold = self.scaffold_daily_base(at.date())?;
            let after = append_log_line(scaffold.clone(), &line, &anchor)?;
            hashed.insert(daily_path.clone(), format!("{scaffold}\u{0}{entry}"));
            edits.insert(
                daily_path.clone(),
                PlannedEdit {
                    path: daily_path.clone(),
                    before: String::new(),
                    after: after.clone(),
                    changes: vec![Change::LogLine],
                    edits: vec![AddressEdit {
                        range: 0..0,
                        replacement: after,
                    }],
                },
            );
        }

        // A move is hashed by its paths and its kind, never by a non-note
        // file's bytes, which the planner does not read: a note's content
        // enters through its edit below (or is moved as the snapshot holds
        // it), and an artefact carries no reference to rewrite.
        let mut input = String::new();
        for (from, to) in &moves {
            let kind = if snapshot.contents.contains_key(from) {
                "move"
            } else {
                "move-file"
            };
            input.push_str(&format!("{kind}\u{0}{from}\u{0}{to}\u{0}"));
        }
        for (path, text) in &hashed {
            input.push_str(&format!("edit\u{0}{path}\u{0}{text}\u{0}"));
        }

        Ok(RenamePlan {
            target: resolved,
            new_slug: new_slug.to_owned(),
            moves,
            edits: edits.into_values().collect(),
            warnings,
            log_line: Some(entry),
            relink,
            hash: content_hash(&input),
        })
    }

    /// Plan renaming the note `target` names to `new_slug` against the
    /// vault as it is now, for the person to see before anything is
    /// written: a [`rename_snapshot`](Self::rename_snapshot) taken **without
    /// the write lock**, then [`plan_rename`](Self::plan_rename). Writes
    /// nothing. The plan's `hash` is what
    /// [`rename_note`](Self::rename_note) takes as `expected_plan`.
    pub fn rename_preview(
        &self,
        target: &str,
        new_slug: &str,
        reason: Option<&str>,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        self.preview(target, Mode::Rename(new_slug), reason, at)
    }

    /// [`rename_preview`](Self::rename_preview) for a relink
    /// ([`plan_relink`](Self::plan_relink)): the plan against the vault as
    /// it is now, without the lock. Writes nothing; its `hash` is what
    /// [`relink_note`](Self::relink_note) takes as `expected_plan`.
    pub fn relink_preview(
        &self,
        target: &str,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        self.preview(target, Mode::Relink, None, at)
    }

    fn preview(
        &self,
        target: &str,
        mode: Mode<'_>,
        reason: Option<&str>,
        at: NaiveDateTime,
    ) -> Result<RenamePlan, DomainError> {
        let snapshot = self.rename_snapshot()?;
        let plan = self.plan(&snapshot, target, mode, reason, at)?;
        ensure_destinations_free(&*self.store, &plan, false)?;
        Ok(plan)
    }

    /// Rename the note `target` names to `new_slug` (RFC 0007 §5.2–§5.6,
    /// §6.2): write exactly the plan the person saw, or nothing.
    ///
    /// The transaction is opened first, so the write lock is held while
    /// the vault is read: the snapshot and the plan are built **under the
    /// lock** and stay true until the commit. With `expected_plan`, the
    /// plan's `hash` must equal it, else [`DomainError::StalePlan`] and
    /// nothing is written: a referrer changed since the preview, or the day
    /// rolled over (the date enters the hash through today's daily note's
    /// path). `None` skips the check. A no-op plan writes nothing and
    /// returns `written: []`. Every refusal of [`plan_rename`](Self::plan_rename)
    /// is returned as it is.
    ///
    /// Staged in this order: the move, as `write_file(new, after)` then
    /// `delete_file(old)` (never `move_file` for markdown: the commit's
    /// facet seam derives an upserted note's links, tags and FTS row from
    /// its paired `Write`, so an upsert after a pure move would leave them
    /// stale); `upsert_note(new)`, its milestone and deadline rows, and
    /// `remove_note(old)` (so no ghost row keeps declaring the note's
    /// aliases); then for every other edit `write_file` and `upsert_note`,
    /// with the milestone and deadline rows of every project map among
    /// them. An archived referrer's snapshot row is left as it is: its
    /// path does not change, `upsert_note` does not cascade, and the
    /// masked hash still verifies.
    ///
    /// A portfolio's folder moves as one (§5.3): each of its notes as above
    /// (`write_file` + `delete_file`, `upsert_note` + `remove_note`), and
    /// every other file under it (an attachment artefact, a filed document,
    /// an ignored file) with the transaction's `Move` op, which keeps its
    /// bytes and needs no index row, since none of them has one. After a
    /// commit whose files landed, every folder under `portfolios/<old>/`
    /// (an empty one the move never saw too) and then the folder itself is
    /// pruned, deepest first, with [`remove_dir_if_empty`](cdno_core::store::VaultStore::remove_dir_if_empty),
    /// never above it, best-effort: a folder something still holds stays,
    /// and a failure is logged at `debug` and never surfaced. After a
    /// failed commit the same prune runs on `portfolios/<new>/`, so the
    /// folders the attempt created do not outlive the files the rollback
    /// took back.
    ///
    /// A move destination the store already holds is refused, here and in
    /// [`rename_preview`](Self::rename_preview), as
    /// [`DomainError::SlugTaken`] (`kind: stem`, the file as holder), before
    /// anything is staged; for a portfolio, so is a `portfolios/<new>/`
    /// folder holding any file (`kind: folder`, the folder as holder). An
    /// empty one is pruned here, under the lock, and refused when anything
    /// is left at its name (a symlink, a case variant that survived); the
    /// preview, which may not prune, refuses one the store does not list as
    /// a directory under exactly that name.
    ///
    /// A failed file write rolls the written files back. When the rollback
    /// itself fails for some path, the result is
    /// [`DomainError::RenameIncomplete`] naming each such path, with the
    /// write's error; when no failure names a path, the transaction error;
    /// with a clean rollback, the store error. An index failure after every file landed is not a
    /// failure of the rename: `Ok` with `index_stale: true`.
    ///
    /// `guard` runs once the write lock is held and before the vault is
    /// read or planned (RFC 0007 §5.8): an interface's re-check that no git
    /// operation was left paused between its own check and the lock. Its
    /// error is returned as it is, with nothing staged. A caller with
    /// nothing to re-check passes `|| Ok(())`.
    pub fn rename_note(
        &self,
        target: &str,
        new_slug: &str,
        reason: Option<&str>,
        expected_plan: Option<&str>,
        at: NaiveDateTime,
        guard: impl FnOnce() -> Result<(), DomainError>,
    ) -> Result<RenameOutcome, DomainError> {
        self.write(
            target,
            Mode::Rename(new_slug),
            reason,
            expected_plan,
            at,
            guard,
        )
    }

    /// [`rename_note`](Self::rename_note) for a relink
    /// ([`plan_relink`](Self::plan_relink)): the same write path, under the
    /// lock, with the same `expected_plan` contract and `guard`. Nothing
    /// moves, so `written` is the rewritten referrers and today's daily
    /// note, in path order; an empty plan writes nothing.
    pub fn relink_note(
        &self,
        target: &str,
        expected_plan: Option<&str>,
        at: NaiveDateTime,
        guard: impl FnOnce() -> Result<(), DomainError>,
    ) -> Result<RenameOutcome, DomainError> {
        self.write(target, Mode::Relink, None, expected_plan, at, guard)
    }

    fn write(
        &self,
        target: &str,
        mode: Mode<'_>,
        reason: Option<&str>,
        expected_plan: Option<&str>,
        at: NaiveDateTime,
        guard: impl FnOnce() -> Result<(), DomainError>,
    ) -> Result<RenameOutcome, DomainError> {
        let mut tx = self.transaction()?; // lock held from here to the commit (#196)
        guard()?;
        let snapshot = self.rename_snapshot()?;
        let plan = self.plan(&snapshot, target, mode, reason, at)?;
        ensure_destinations_free(&*self.store, &plan, true)?;
        if let Some(expected) = expected_plan
            && expected != plan.hash
        {
            return Err(DomainError::StalePlan {
                expected: expected.to_owned(),
                actual: plan.hash,
            });
        }
        if plan.is_empty() {
            return Ok(RenameOutcome {
                plan,
                written: Vec::new(),
                index_stale: false,
            });
        }

        let written = stage_rename(&plan, &snapshot, &mut tx)?;
        // A portfolio's folders, before and after: the old one is pruned
        // once every file has left it, the new one when the commit failed
        // and the rollback took the files back out of it.
        let folders = moved_portfolio_folders(&plan);
        let index_stale = match tx.commit() {
            Ok(_) => false,
            Err(TransactionError::IndexStale(_)) => true,
            Err(TransactionError::FileWrite {
                source,
                rollback_failures,
            }) => {
                // The store's write and move create the destination's
                // folders and the rollback restores files only: leave no
                // empty `portfolios/<new>/` behind to hold the name.
                if let Some((_, new_folder)) = &folders {
                    prune_folder_tree(&*self.store, new_folder);
                }
                if rollback_failures.is_empty() {
                    return Err(DomainError::Store(source));
                }
                let failed: Vec<VaultPath> =
                    rollback_failures.iter().filter_map(failed_path).collect();
                if failed.is_empty() {
                    // Nothing to name: the transaction's own error says
                    // what failed and that the rollback did too.
                    return Err(DomainError::Transaction(TransactionError::FileWrite {
                        source,
                        rollback_failures,
                    }));
                }
                return Err(DomainError::RenameIncomplete { failed, source });
            }
        };
        // Every file landed: prune what the folder move emptied.
        if let Some((old_folder, _)) = &folders {
            prune_folder_tree(&*self.store, old_folder);
        }
        Ok(RenameOutcome {
            plan,
            written,
            index_stale,
        })
    }

    /// Resolve the rename's `target` in the snapshot, with the grammar of
    /// [`NoteRef::parse`]: a path (with or without `.md`, or a folder for
    /// its `_index.md`), a journal reference, `type:slug`, or a bare slug,
    /// which matches a stem or folder name and, on no match, the one note
    /// declaring it as an alias.
    fn resolve_in_snapshot(
        &self,
        snapshot: &VaultSnapshot,
        target: &str,
        at: NaiveDateTime,
    ) -> Result<ResolvedNote, DomainError> {
        let registry = self.type_registry();
        let known = registry.all_names();
        let not_found =
            || DomainError::Store(cdno_core::error::StoreError::NotFound(target.to_owned()));
        let journal = |relpath: String| -> Result<ResolvedNote, DomainError> {
            let path = VaultPath::new(relpath)?;
            if snapshot.paths.contains(&path) {
                Ok(ResolvedNote::direct(path))
            } else {
                Err(not_found())
            }
        };
        let today = at.date();
        let (slug, of_type) = match NoteRef::parse(target, &known) {
            NoteRef::Relative(day) => {
                return journal(paths::daily_note_relpath(day.resolve(today)));
            }
            NoteRef::Date(date) => return journal(paths::daily_note_relpath(date)),
            NoteRef::Period(PeriodRef::Week(date)) => {
                return journal(paths::weekly_note_relpath(date));
            }
            NoteRef::Period(PeriodRef::Month(date)) => {
                return journal(paths::monthly_note_relpath(date));
            }
            NoteRef::Path(raw) => {
                let raw = raw.trim_end_matches('/');
                // A named extension keeps its format: `x.typ` never reaches
                // `x.md`, nor `x.md` an `x.typ`.
                let named = VaultPath::new(raw).ok().and_then(|p| note_format(&p));
                let address = match VaultPath::new(raw) {
                    Ok(path) if named.is_some() => paths::note_address(&path),
                    _ => raw.to_owned(),
                };
                // The exact path first: `x.MD` names that file, and its address
                // `x` must not reach a sibling `x/_index.md` instead.
                if let Ok(exact) = VaultPath::new(raw)
                    && snapshot.paths.contains(&exact)
                {
                    return Ok(ResolvedNote::direct(exact));
                }
                if let Some(path) = paths::note_candidates(&address)
                    .into_iter()
                    .filter(|path| named.is_none_or(|format| note_format(path) == Some(format)))
                    .find(|path| snapshot.paths.contains(path))
                {
                    return Ok(ResolvedNote::direct(path));
                }
                return Err(not_found());
            }
            NoteRef::Typed { note_type, slug } => (slug, Some(note_type)),
            NoteRef::Bare(slug) => (slug, None),
        };
        let of_type_ok = |path: &VaultPath| match of_type {
            None => true,
            Some(t) => {
                snapshot
                    .contents
                    .get(path)
                    .and_then(|c| note_type_name(c))
                    .as_deref()
                    == Some(t)
            }
        };
        let mut hits: Vec<&VaultPath> = snapshot
            .paths
            .iter()
            .filter(|p| candidate_slug(p).as_deref() == Some(slug) && of_type_ok(p))
            .collect();
        hits.sort();
        // A question and its same-slug portfolio's folder note answer to the
        // one bare slug (§5.3): the pair renames from the question, so that
        // is the hit. Every other multi-hit shape stays ambiguous.
        if let [a, b] = hits.as_slice() {
            // Markdown on purpose: questions and portfolio folder notes are
            // machine-written Markdown.
            let is_question = |p: &VaultPath| {
                [paths::QUESTIONS_RESEARCH, paths::QUESTIONS_LIFE]
                    .iter()
                    .any(|dir| p.as_path() == std::path::Path::new(&format!("{dir}/{slug}.md")))
            };
            let is_index = |p: &VaultPath| {
                p.as_path()
                    // Markdown on purpose: a portfolio's folder note is `_index.md`.
                    == std::path::Path::new(&format!("{}/{slug}/_index.md", paths::PORTFOLIOS))
            };
            if is_question(a) && is_index(b) {
                return Ok(ResolvedNote::direct((*a).clone()));
            }
            if is_question(b) && is_index(a) {
                return Ok(ResolvedNote::direct((*b).clone()));
            }
        }
        match hits.as_slice() {
            [one] => return Ok(ResolvedNote::direct((*one).clone())),
            [] => {}
            _ => return Err(DomainError::AmbiguousSlug(slug.to_owned())),
        }
        let mut holders: Vec<&VaultPath> = Vec::new();
        for (alias, holder) in &snapshot.aliases {
            if alias == slug && !holders.contains(&holder) && of_type_ok(holder) {
                holders.push(holder);
            }
        }
        match holders.as_slice() {
            [holder] => {
                let note = ResolvedNote::direct((*holder).clone());
                Ok(ResolvedNote::via_alias(note.path, note.slug, slug))
            }
            [] => Err(not_found()),
            _ => Err(DomainError::AmbiguousSlug(slug.to_owned())),
        }
    }
}

/// Stage `plan` in `tx`, in the order [`Vault::rename_note`] states, and
/// return the paths it writes in that order.
fn stage_rename(
    plan: &RenamePlan,
    snapshot: &VaultSnapshot,
    tx: &mut VaultTransaction,
) -> Result<Vec<VaultPath>, DomainError> {
    let after_of = |path: &VaultPath| {
        plan.edits
            .iter()
            .find(|e| e.path == *path)
            .map(|e| e.after.as_str())
    };
    let mut written = Vec::with_capacity(plan.edits.len() + plan.moves.len());
    for (from, to) in &plan.moves {
        // A file the snapshot holds no note for (a portfolio's attachment
        // artefact, a filed document, an ignored file) has no index row to
        // carry: it moves as bytes, which never pass through here.
        let Some(held) = snapshot.contents.get(from) else {
            tx.move_file(from.clone(), to.clone());
            written.push(to.clone());
            continue;
        };
        // A note's own edits (the target's alias, an evidence note's
        // `portfolio:` and links) are keyed by `from`; a note with nothing
        // to edit moves as it is.
        let content = after_of(from).unwrap_or(held);
        tx.write_file(to.clone(), content);
        tx.delete_file(from.clone());
        stage_index_rows(to, content, tx)?;
        tx.remove_note(from.clone());
        written.push(to.clone());
    }
    for edit in &plan.edits {
        if plan.moves.iter().any(|(from, _)| *from == edit.path) {
            continue;
        }
        tx.write_file(edit.path.clone(), edit.after.as_str());
        stage_index_rows(&edit.path, &edit.after, tx)?;
        written.push(edit.path.clone());
    }
    Ok(written)
}

/// Stage the index rows of a note written at `path` with `content`: its
/// `upsert_note` (the commit derives its links, tags and FTS row from the
/// paired write), and for a project map its milestone and deadline rows. A
/// note reconciliation would not index (no parseable frontmatter, no
/// `type:`) gets no row here either.
fn stage_index_rows(
    path: &VaultPath,
    content: &str,
    tx: &mut VaultTransaction,
) -> Result<(), DomainError> {
    let Some(note_type) = parse_note(path, content)
        .ok()
        .and_then(|(fm, _)| fm.require_field::<String>("type").ok())
    else {
        return Ok(());
    };
    tx.upsert_note(build_index_entry_for(path, content, &note_type)?);
    if note_type == NoteType::Project.as_str() {
        stage_project_index_rows(path, content, tx);
    }
    Ok(())
}

/// The path a failed rollback step names. Every store error a restore or
/// delete raises names its path; one that does not names nothing.
fn failed_path(error: &cdno_core::error::StoreError) -> Option<VaultPath> {
    use cdno_core::error::StoreError;
    let raw = match error {
        StoreError::NotFound(p)
        | StoreError::AlreadyExists(p)
        | StoreError::PermissionDenied(p)
        | StoreError::OutsideVault(p)
        | StoreError::Io { path: p, .. } => p,
        StoreError::LockTimeout(_) => return None,
    };
    VaultPath::new(raw.as_str()).ok()
}

/// Refuse a plan whose move would write over a file the store already
/// holds (RFC 0007 §5.2): one the snapshot did not list, one that appeared
/// since, and a case variant on a filesystem that cannot tell names apart
/// (macOS and Windows by default), where writing `other.md` would land on
/// `Other.md` and a rename that only changes case would write the note
/// over itself and then delete it. This is the only check that carries the
/// filesystem's case rule, so a case-only rename is refused exactly where
/// the filesystem cannot tell the names apart. The holder is named as the
/// store lists it: the entry of the destination's folder whose name equals
/// the destination's ignoring case, else the destination itself.
///
/// A portfolio's folder moves as one, so a `portfolios/<new>/` holding any
/// file (a folder of non-notes the snapshot's names cannot show) is refused
/// the same way, with `kind: folder`: the move never merges into a folder's
/// contents. One that holds no file at any depth (left by hand, or by an
/// attempt whose rollback could not prune it) holds nothing to merge with,
/// but its **name** still matters: on a filesystem that cannot tell names
/// apart, an empty `portfolios/New/` would take every file while the index
/// and the links say `new/`, and a symlink named `new` would have them
/// written through it. So:
///
/// - with `prune` (the write, under the lock, before staging) the empty
///   tree is pruned deepest first, and the folder is free only when nothing
///   is left at its name afterwards (a removed case variant is recreated in
///   the right case by the first write); a symlink, or whatever else
///   survives, is refused;
/// - without it (the preview, which holds no lock and must not change the
///   vault) the folder is free only when the store lists it as a real
///   directory under exactly its name, and refused otherwise.
///
/// Every move's source is checked too: one the store cannot reach (a
/// dangling symlink, one pointing outside the vault) is refused with the
/// store's error rather than at the write.
fn ensure_destinations_free(
    store: &dyn cdno_core::store::VaultStore,
    plan: &RenamePlan,
    prune: bool,
) -> Result<(), DomainError> {
    let taken = |to: &VaultPath, kind: TakenBy| -> Result<DomainError, DomainError> {
        let wanted = to.to_string().to_lowercase();
        let parent = to
            .as_path()
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| Ok(VaultPath::root()), VaultPath::new)?;
        let holder = store
            .list_dir(&parent)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.to_string().to_lowercase() == wanted)
            .unwrap_or_else(|| to.clone());
        Ok(DomainError::SlugTaken {
            slug: plan.new_slug.clone(),
            holder,
            kind,
        })
    };
    if let Some((_, new_dir)) = moved_portfolio_folders(plan)
        && store.exists(&new_dir)?
    {
        if !store.walk_dir(&new_dir)?.is_empty() {
            return Err(taken(&new_dir, TakenBy::Folder)?);
        }
        let free = if prune {
            prune_folder_tree(store, &new_dir);
            !store.exists(&new_dir)?
        } else {
            let parent = VaultPath::new(paths::PORTFOLIOS)?;
            store.walk_dirs(&parent)?.contains(&new_dir)
        };
        if !free {
            return Err(taken(&new_dir, TakenBy::Folder)?);
        }
    }
    for (from, to) in &plan.moves {
        // The source must be a file the store can reach: a symlink that
        // dangles or points outside the vault is listed by the walk as a
        // file, planned as a `Move`, and would otherwise fail only at the
        // write. `metadata` resolves it (confinement included) without
        // reading a byte, so it is refused here, before the lock, with the
        // store's own `OutsideVault` / `NotFound`.
        store.metadata(from)?;
        if store.exists(to)? {
            return Err(taken(to, TakenBy::Stem)?);
        }
    }
    Ok(())
}

/// Prune `root` and every folder beneath it that holds nothing: each folder
/// [`walk_dirs`](cdno_core::store::VaultStore::walk_dirs) lists under `root`,
/// deepest first, then `root` itself, each through
/// [`remove_dir_if_empty`](cdno_core::store::VaultStore::remove_dir_if_empty),
/// which leaves a folder that still holds anything (a file that arrived
/// since, or a folder holding one). The listing never reaches above
/// `root`, and nothing else is passed: the store's prune does not refuse
/// the vault root or `.cuaderno/`, so this is the guard. Best-effort: a
/// failure is logged at `debug`, and the rename's outcome stands.
fn prune_folder_tree(store: &dyn cdno_core::store::VaultStore, root: &VaultPath) {
    let mut folders: Vec<VaultPath> = match store.walk_dirs(root) {
        Ok(dirs) => dirs
            .into_iter()
            .filter(|dir| dir.as_path().starts_with(root.as_path()) && dir != root)
            .collect(),
        Err(error) => {
            tracing::debug!(folder = %root, %error, "rename: could not list folders to prune");
            Vec::new()
        }
    };
    // Deepest first, so a folder is tried only after everything below it.
    folders.sort_by_key(|dir| std::cmp::Reverse(dir.as_path().components().count()));
    folders.push(root.clone());
    for dir in folders {
        if let Err(error) = store.remove_dir_if_empty(&dir) {
            tracing::debug!(folder = %dir, %error, "rename: could not prune an emptied folder");
        }
    }
}

/// The §5.2 check. Returns the target's built-in type (`None` for a custom
/// note) when it is renamable now.
///
/// `relink` skips the one refusal that protects the target's own bytes:
/// an archived action note is refused a rename because its frozen prefix
/// would change, and a relink changes nothing on the target (no move, no
/// alias edit; a self-link through a former name is the masked address
/// rewrite any archived referrer gets). Every other refusal applies.
fn renamable_type(
    vault: &Vault,
    path: &VaultPath,
    type_name: Option<&str>,
    snapshot: &VaultSnapshot,
    relink: bool,
) -> Result<Option<NoteType>, DomainError> {
    let Some(name) = type_name else {
        return Err(not_renamable(path, "type none".to_owned()));
    };
    let builtin = name.parse::<NoteType>().ok();
    let renamable = match builtin {
        Some(NoteType::Project | NoteType::Question | NoteType::Action | NoteType::Portfolio) => {
            true
        }
        Some(_) => false,
        None => vault
            .type_registry()
            .resolve(name)
            .is_some_and(|descriptor| descriptor.is_custom()),
    };
    if !renamable {
        return Err(not_renamable(path, format!("type {name}")));
    }
    if !relink && path.as_path().starts_with(paths::ACTIONS_DONE) {
        return Err(not_renamable(path, "archived".to_owned()));
    }
    // A folder note moves its whole folder (§5.3). Only a portfolio's, at
    // `portfolios/<slug>/_index.md`, does: a question whose portfolio
    // shares its slug moves that folder with it (T23), and the portfolio
    // alone is refused below, since the pair's correlation
    // (`find_question_path`) is by slug.
    let a_portfolio_folder =
        builtin == Some(NoteType::Portfolio) && portfolio_folder(path).is_some();
    if (is_folder_note(path) || builtin == Some(NoteType::Portfolio)) && !a_portfolio_folder {
        return Err(not_renamable(path, "folder".to_owned()));
    }
    // Markdown on purpose: questions are machine-written Markdown.
    if a_portfolio_folder
        && let Some(slug) = candidate_slug(path)
        && [paths::QUESTIONS_RESEARCH, paths::QUESTIONS_LIFE]
            .iter()
            .filter_map(|dir| VaultPath::new(format!("{dir}/{slug}.md")).ok())
            .any(|question| snapshot.files.contains(&question))
    {
        return Err(not_renamable(path, "rename the question".to_owned()));
    }
    Ok(builtin)
}

/// The target types whose §3.5 fields follow a rename of a note of type
/// `builtin`: its own. A question's same-slug portfolio (the evidence
/// `portfolio:` rows) joins when `paired`, that is when the portfolio's
/// folder note exists and moves with it (T23); a `portfolio:` value equal
/// to a lone question's slug names no note that moves, so it is left alone.
fn field_keys_for(builtin: Option<NoteType>, paired: bool) -> Vec<NoteType> {
    match builtin {
        Some(NoteType::Question) if paired => vec![NoteType::Question, NoteType::Portfolio],
        Some(t) => vec![t],
        None => Vec::new(),
    }
}

/// The `portfolios/<slug>/_index.md` sharing the question `question`'s slug,
/// when the snapshot holds it: the pair's other half (§5.3).
fn question_portfolio_index(question: &VaultPath, snapshot: &VaultSnapshot) -> Option<VaultPath> {
    let slug = candidate_slug(question)?;
    // Markdown on purpose: a portfolio's folder note is `_index.md`.
    VaultPath::new(format!("{}/{slug}/_index.md", paths::PORTFOLIOS))
        .ok()
        .filter(|index| snapshot.paths.contains(index))
}

/// The portfolio folder `plan` moves, before and after: the first move whose
/// source is a portfolio's folder note and whose destination is one too (the
/// target's own for a portfolio, the pair's for a question).
fn moved_portfolio_folders(plan: &RenamePlan) -> Option<(VaultPath, VaultPath)> {
    plan.moves
        .iter()
        .find_map(|(from, to)| portfolio_folder(from).zip(portfolio_folder(to)))
}

/// The edit to the note's `aliases:`, relative to its frontmatter block:
/// `old_slug` appended, and `new_slug` removed when the note takes back a
/// former name (D5: a slug is never its own alias). `None` when nothing
/// changes (the old slug already listed, the new one not). An `Err` carries
/// the reason the aliases cannot be recorded, which refuses the rename:
/// without the old slug every old link would dangle, and with the new one
/// left in the note would list its own slug as a former name.
fn alias_edit(
    content: &str,
    old_slug: &str,
    new_slug: &str,
) -> Result<Option<AddressEdit>, String> {
    let Some((block, _)) = split_frontmatter(content) else {
        return Err("it has no frontmatter to record its old slug in".to_owned());
    };
    let (dropped, removal) = remove_frontmatter_list_value(block, ALIASES_KEY, new_slug);
    let (after, mut appended) = append_frontmatter_list_value(&dropped, ALIASES_KEY, old_slug);
    let listed = |text: &str, slug: &str| {
        markdown_frontmatter_json(text)
            .map(|fm| frontmatter_aliases(&fm).iter().any(|a| a == slug))
            .unwrap_or(false)
    };
    let not_a_list = || "its `aliases:` is not a list the old slug can be added to".to_owned();
    let edit = if removal.is_empty() {
        // Only the insertion, in `block`'s positions already. Whether it
        // parses is the drift guard's to say; `None` is right only when the
        // old slug is listed already.
        match appended.pop() {
            Some(edit) => Some(edit),
            None if listed(content, old_slug) => None,
            None => return Err(not_a_list()),
        }
    } else {
        // A removal, then perhaps an insertion in the shifted text: one
        // edit over the region where `block` and `after` differ.
        let (region, suffix) = diff_region(block, &after);
        let edit = AddressEdit {
            range: region.clone(),
            replacement: after[region.start..after.len() - suffix].to_owned(),
        };
        let edited = apply(content, &[shift(edit.clone(), offset_in(content, block))]);
        if !listed(&edited, old_slug) {
            return Err(not_a_list());
        }
        Some(edit)
    };
    // On either branch, the new slug must be gone from the list: a slug is
    // never its own alias.
    let edited = match &edit {
        Some(edit) => apply(content, &[shift(edit.clone(), offset_in(content, block))]),
        None => content.to_owned(),
    };
    if old_slug != new_slug && listed(&edited, new_slug) {
        return Err(
            "its `aliases:` lists the new slug in a form it cannot be removed from".to_owned(),
        );
    }
    Ok(edit)
}

fn not_renamable(path: &VaultPath, reason: String) -> DomainError {
    DomainError::NoteNotRenamable {
        path: path.to_string(),
        reason,
    }
}

/// `path` with its file stem replaced by `slug`, in the same folder.
fn sibling_path(path: &VaultPath, slug: &str) -> Result<VaultPath, DomainError> {
    let format = note_format(path).unwrap_or(NoteFormat::Markdown);
    let file = format!("{slug}.{}", format.extension());
    let joined = match path.as_path().parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file),
        _ => std::path::PathBuf::from(file),
    };
    Ok(VaultPath::new(joined)?)
}

/// Whether `path` is a folder note (`<dir>/_index.md`).
fn is_folder_note(path: &VaultPath) -> bool {
    path.as_path().file_stem() == Some(OsStr::new("_index"))
}

/// The folder `portfolios/<slug>` when `path` is exactly a portfolio's
/// folder note, `portfolios/<slug>/_index.md`.
fn portfolio_folder(path: &VaultPath) -> Option<VaultPath> {
    let p = path.as_path();
    // Markdown on purpose: a portfolio's folder note is `_index.md`.
    if p.file_name() != Some(OsStr::new("_index.md")) {
        return None;
    }
    let dir = p.parent()?;
    let mut components = dir.components();
    let (Some(root), Some(_), None) = (components.next(), components.next(), components.next())
    else {
        return None;
    };
    (root.as_os_str() == OsStr::new(paths::PORTFOLIOS))
        .then(|| VaultPath::new(dir).ok())
        .flatten()
}

/// `path`, under `from`, at the same place under `to`.
fn rebase(path: &VaultPath, from: &VaultPath, to: &VaultPath) -> Result<VaultPath, DomainError> {
    let rest = path
        .as_path()
        .strip_prefix(from.as_path())
        .map_err(|_| cdno_core::error::StoreError::NotFound(path.to_string()))?;
    Ok(VaultPath::new(to.as_path().join(rest))?)
}

/// Where `raw` resolves in the snapshot, by the index's own resolver.
fn resolve_in(raw: &str, snapshot: &VaultSnapshot, aliases: &AliasMap) -> Option<VaultPath> {
    resolve_with_alias(raw, snapshot, aliases).map(|(path, _)| path)
}

/// [`resolve_in`], with whether the alias rule was the one that matched:
/// the `via_alias` the index records and lint's straggler warning reads.
fn resolve_with_alias(
    raw: &str,
    snapshot: &VaultSnapshot,
    aliases: &AliasMap,
) -> Option<(VaultPath, bool)> {
    let link = WikilinkRaw {
        target: raw.to_owned(),
        label: None,
        is_embed: false,
    };
    resolve_wikilinks(vec![link], &snapshot.paths, aliases)
        .into_iter()
        .next()
        .and_then(|entry| entry.resolved_path.map(|path| (path, entry.via_alias)))
}

fn frontmatter_json(path: &VaultPath, content: &str) -> Option<serde_json::Value> {
    parse_note(path, content).ok().map(|(fm, _)| fm.as_json())
}

/// [`frontmatter_json`] for text known to be a Markdown frontmatter block's note: the alias edit
/// and the §3.5 field rewrite work on YAML text and run only where `split_frontmatter` found one.
fn markdown_frontmatter_json(content: &str) -> Option<serde_json::Value> {
    // Markdown on purpose: the callers hold YAML text, never a Typst source.
    Frontmatter::parse(content).ok().map(|(fm, _)| fm.as_json())
}

/// The `type:` a note's frontmatter declares, read as text (the first
/// top-level `type:` scalar) rather than through the parser, so a referrer
/// whose frontmatter no longer parses still has its §3.5 fields found, and
/// the drift guard refuses it, instead of the rename skipping them.
fn note_type_name(content: &str) -> Option<String> {
    let (block, _) = split_frontmatter(content)?;
    let range = frontmatter_scalar_spans(block, "type").into_iter().next()?;
    Some(block[range].to_owned())
}

/// The note's title: its `title:` field, else its first H1.
fn note_title(path: &VaultPath, content: &str) -> Option<String> {
    let parsed = parse_note(path, content).ok();
    if let Some((fm, _)) = &parsed
        && let Some(title) = fm.as_json().get("title").and_then(|t| t.as_str())
        && !title.trim().is_empty()
    {
        return Some(title.to_owned());
    }
    let body = parsed.map_or(content, |(_, body)| body);
    match note_format(path) {
        Some(NoteFormat::Typst) => cdno_core::note::note_headings(path, body)
            .into_iter()
            .find(|(level, _)| *level == 1)
            .map(|(_, text)| text),
        _ => first_h1(body),
    }
}

/// Byte offset of `inner`, a subslice of `outer`, within it.
fn offset_in(outer: &str, inner: &str) -> usize {
    let (o, i) = (outer.as_ptr() as usize, inner.as_ptr() as usize);
    debug_assert!(
        i >= o && i + inner.len() <= o + outer.len(),
        "offset_in: inner is not a subslice of outer"
    );
    i - o
}

fn shift(edit: AddressEdit, base: usize) -> AddressEdit {
    AddressEdit {
        range: edit.range.start + base..edit.range.end + base,
        replacement: edit.replacement,
    }
}

fn apply(text: &str, edits: &[AddressEdit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.range.start]);
        out.push_str(&edit.replacement);
        at = edit.range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Fold the change from `mid` (`before` with `edits` applied) to `after`
/// into `edits`, so they stay in `before`'s byte positions.
///
/// The change is taken as the one region where `mid` and `after` differ
/// (for a log line, an insertion at the end of the log). A region that
/// meets one of `edits` in `mid` absorbs it, so the edits stay
/// non-overlapping; when placing the line also re-pins the anchor section
/// to the bottom, that region is larger, and is still exact.
fn fold_in(edits: &[AddressEdit], mid: &str, after: &str) -> Vec<AddressEdit> {
    let (region, _) = diff_region(mid, after);
    let (mut start, mut end) = (region.start, region.end);

    // Each edit's range in `mid`, and how much it moved the text after it.
    let mut placed: Vec<(usize, usize, isize)> = Vec::with_capacity(edits.len());
    let mut delta: isize = 0;
    for edit in edits {
        let at = edit.range.start.checked_add_signed(delta).unwrap_or(0);
        let moved = edit.replacement.len() as isize - edit.range.len() as isize;
        placed.push((at, at + edit.replacement.len(), moved));
        delta += moved;
    }
    // Whether the window meets an edit: overlaps it, or (empty) sits
    // strictly inside it.
    let meets = |start: usize, end: usize, a: usize, b: usize| {
        if start == end {
            a < start && start < b
        } else {
            a < end && start < b
        }
    };
    // Grow the window over every edit it meets, until none straddles it.
    loop {
        let mut grew = false;
        for &(a, b, _) in &placed {
            if meets(start, end, a, b) && (a < start || b > end) {
                start = start.min(a);
                end = end.max(b);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    // A `mid` position no edit straddles, in `before`'s bytes.
    let to_before = |pos: usize| -> usize {
        let shift: isize = placed
            .iter()
            .filter(|&&(_, b, _)| b <= pos)
            .map(|&(_, _, moved)| moved)
            .sum();
        pos.checked_add_signed(-shift).unwrap_or(0)
    };
    let mut out: Vec<AddressEdit> = edits
        .iter()
        .zip(&placed)
        .filter(|(_, (a, b, _))| !meets(start, end, *a, *b))
        .map(|(edit, _)| edit.clone())
        .collect();
    let tail = mid.len() - end;
    out.push(AddressEdit {
        range: to_before(start)..to_before(end),
        replacement: after[start..after.len() - tail].to_owned(),
    });
    out.sort_by_key(|e| (e.range.start, e.range.end));
    out
}

/// The one region where `a` and `b` differ, as a range of `a` (and the
/// length of the common suffix): the longest common prefix, then the
/// longest common suffix of what remains, both on char boundaries.
fn diff_region(a: &str, b: &str) -> (Range<usize>, usize) {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut prefix = ab.iter().zip(bb).take_while(|(x, y)| x == y).count();
    while !(a.is_char_boundary(prefix) && b.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let room = a.len().min(b.len()) - prefix;
    let mut suffix = ab
        .iter()
        .rev()
        .zip(bb.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    while !(a.is_char_boundary(a.len() - suffix) && b.is_char_boundary(b.len() - suffix)) {
        suffix -= 1;
    }
    (prefix..a.len() - suffix, suffix)
}
