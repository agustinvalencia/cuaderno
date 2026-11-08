//! `Vault::resolve_note_ref` — turn a user-typed reference into a note path.
//!
//! The addressing layer behind `cdno open`. It answers a question none of the
//! existing resolvers do: *which of all notes*, rather than *which project* or
//! *which question*. That difference is why this supersedes the per-type
//! resolvers here rather than delegating to them — only four types
//! (`project`, `question`, `stewardship`, `portfolio`) have one, while
//! actions, commitments, evidence, tracking, inbox and every config-defined
//! custom type do not, and `cdno open <action-slug>` has to work.
//!
//! Two halves, deliberately split:
//!
//! 1. [`NoteRef::parse`] classifies a reference by **shape alone** — pure, no
//!    vault access, so the grammar is testable independently of what happens
//!    to exist in a given vault.
//! 2. [`Vault::resolve_note_ref`] consults the store and index.
//!
//! The split is what lets a *write* verb reuse the grammar without inheriting
//! the fuzzy, ambiguity-tolerant resolution that only a read verb can afford.
//!
//! A slug reference that matches no note's stem gets one more tier: the one
//! note declaring it as an alias, a former name (RFC 0007 §5.5). The result
//! is a [`ResolvedNote`] carrying the current slug and, when the alias tier
//! matched, the alias; a shared alias is ambiguous between its holders.
//! [`Vault::alias_of`] is that rule on its own, shared by the per-type
//! selectors (project, question, stewardship, commitment), each counting
//! uniqueness over the holders of its own type.
//!
//! **What this deliberately does not do:** fuzzy matching. An exact-slug miss
//! comes back as [`RefResolution::NotFound`], and the interface layer decides
//! whether to offer a picker or error. Keeping the matcher in one place (the
//! CLI, which owns `inquire`'s) stops the auto-open rule and the picker from
//! disagreeing about what "matches".

use chrono::NaiveDate;

use cdno_core::index::NoteCandidate;
use cdno_core::path::VaultPath;
use cdno_core::paths;
use cdno_core::paths::{NoteFormat, note_format, note_format_candidates};

use crate::error::DomainError;

/// A note reference classified by shape. Borrows from the input, so parsing
/// allocates nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteRef<'a> {
    /// `today`, `yesterday`, `tomorrow` — resolved against the caller's clock.
    Relative(RelativeDay),
    /// `YYYY-MM-DD` → the daily note for that date.
    Date(NaiveDate),
    /// `YYYY-Www` → the weekly note; `YYYY-MM` → the monthly note.
    Period(PeriodRef),
    /// Contains `/` or ends in `.md` — a vault-relative path.
    Path(&'a str),
    /// `<known-type>:<slug>`, e.g. `project:surrogate-model`.
    Typed { note_type: &'a str, slug: &'a str },
    /// Anything else: a bare slug, to be matched against every note.
    Bare(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeDay {
    Yesterday,
    Today,
    Tomorrow,
}

impl RelativeDay {
    /// The date this names, relative to `today`. Saturating at the calendar
    /// bounds rather than panicking — `chrono`'s range is far outside any
    /// date a vault will hold, so the saturation arm is unreachable in
    /// practice and exists only to keep this total.
    pub(in crate::vault) fn resolve(self, today: NaiveDate) -> NaiveDate {
        let offset = match self {
            Self::Yesterday => -1,
            Self::Today => 0,
            Self::Tomorrow => 1,
        };
        today
            .checked_add_signed(chrono::Duration::days(offset))
            .unwrap_or(today)
    }
}

/// A calendar period naming a journal note: an ISO week or a calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodRef {
    /// `YYYY-Www`. Carried as a date inside the week, which is what
    /// [`paths::weekly_note_relpath`] takes.
    Week(NaiveDate),
    /// `YYYY-MM`. Carried as a date inside the month.
    Month(NaiveDate),
}

/// The outcome of resolving a reference.
///
/// [`Self::NotFound`] is an `Ok` variant, not an error, and that is
/// load-bearing: the CLI falls through to its fuzzy tier without
/// pattern-matching on a [`DomainError`], which stays reserved for genuine
/// failures (a store read that blew up, a path the index cannot parse).
#[derive(Debug, Clone, PartialEq)]
pub enum RefResolution {
    Resolved(ResolvedNote),
    /// One slug, several notes. Never resolved by preference order: guessing
    /// silently opens the wrong file, which is the one failure a navigation
    /// verb cannot afford, because you find out only after typing into it.
    Ambiguous(Vec<NoteCandidate>),
    NotFound {
        reference: String,
        miss: Miss,
    },
}

/// A note a reference or a selector resolved to (RFC 0007 §5.5).
///
/// `slug` is the note's **current** name (`candidate_slug` of `path`),
/// whatever the reference spelled. It is the one a verb writes into a log
/// line, so a journal entry is never composed from a former name the caller
/// happened to type. `resolved_via_alias` is the alias the reference matched,
/// `None` when it matched the note's stem or path directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNote {
    pub path: VaultPath,
    pub slug: String,
    pub resolved_via_alias: Option<String>,
}

impl ResolvedNote {
    /// A note reached by its own stem or path, not through an alias.
    pub(in crate::vault) fn direct(path: VaultPath) -> Self {
        let slug = candidate_slug(&path).unwrap_or_else(|| path.to_string());
        Self {
            path,
            slug,
            resolved_via_alias: None,
        }
    }

    /// A note reached through `alias`, one of its former slugs. `slug` is
    /// the holder's current one, as [`Vault::alias_of`](super::Vault::alias_of)
    /// returns it.
    pub(in crate::vault) fn via_alias(path: VaultPath, slug: String, alias: &str) -> Self {
        Self {
            path,
            slug,
            resolved_via_alias: Some(alias.to_owned()),
        }
    }
}

/// What kind of reference failed to resolve.
///
/// Carried because the three misses want three different things said, and
/// only the grammar knows which one happened. Guessing in the interface layer
/// would mean reimplementing the shape rules there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// A slug matched nothing. The only kind that deserves "did you mean…" —
    /// there is a set of candidates it could plausibly have been.
    ///
    /// A slug no stem matches and no note declares as an alias is this
    /// miss; one two or more notes declare is
    /// [`RefResolution::Ambiguous`] with those notes (RFC 0007 §5.5).
    Slug,
    /// A calendar word or date named a journal note that does not exist yet.
    /// Journal notes are scaffolded lazily on first write, so this is
    /// routine rather than an error in the reference.
    JournalNote,
    /// A path named a file that is not there. Near matches would be actively
    /// unhelpful: someone who typed a path wants "no such file", not a list
    /// of notes with vaguely similar names.
    Path,
}

impl<'a> NoteRef<'a> {
    /// Classify `raw` by shape. `known_types` is the vault's type registry —
    /// needed because the `type:slug` form is only that form when the prefix
    /// really names a type; otherwise `foo:bar` is just an odd slug.
    ///
    /// Precedence is first-match-wins, and the order is the whole design:
    ///
    /// 1. **`<known-type>:<slug>`** first, because it is the escape hatch that
    ///    makes every ambiguity below mechanically resolvable. A user told to
    ///    "try `project:foo`" must always be obeyed.
    /// 2. **Reserved words** (`today`/`yesterday`/`tomorrow`) beat slugs. A
    ///    project genuinely named `today` stays reachable as `project:today`.
    /// 3. **Calendar shapes**, which cannot collide with a slug: a bare
    ///    `2026-08-21` is never a note name.
    /// 4. **Path-shaped**, so an explicit path is never reinterpreted.
    /// 5. **Bare**, the only tier that searches.
    pub fn parse(raw: &'a str, known_types: &[&str]) -> Self {
        let trimmed = raw.trim();

        if let Some((prefix, rest)) = trimmed.split_once(':')
            && known_types.contains(&prefix)
            && !rest.is_empty()
        {
            return Self::Typed {
                note_type: prefix,
                slug: rest,
            };
        }

        match trimmed {
            "today" => return Self::Relative(RelativeDay::Today),
            "yesterday" => return Self::Relative(RelativeDay::Yesterday),
            "tomorrow" => return Self::Relative(RelativeDay::Tomorrow),
            _ => {}
        }

        if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            return Self::Date(date);
        }
        if let Some(week) = parse_iso_week(trimmed) {
            return Self::Period(PeriodRef::Week(week));
        }
        if let Some(month) = parse_month(trimmed) {
            return Self::Period(PeriodRef::Month(month));
        }

        if trimmed.contains('/') || ends_in_note_extension(trimmed) {
            return Self::Path(trimmed);
        }

        Self::Bare(trimmed)
    }
}

/// Whether `raw` ends in a note extension, ASCII case-insensitively.
fn ends_in_note_extension(raw: &str) -> bool {
    VaultPath::new(raw).is_ok_and(|p| note_format(&p).is_some())
}

/// `YYYY-Www` → any date inside that ISO week (the Monday).
///
/// Parsed by hand rather than through a `chrono` format string: `%G-W%V`
/// round-trips on output but will not parse without a weekday component, so
/// the Monday has to be supplied explicitly.
fn parse_iso_week(raw: &str) -> Option<NaiveDate> {
    let (year, week) = raw.split_once("-W")?;
    let year: i32 = year.parse().ok()?;
    // Reject `-W7`: the canonical form is zero-padded, and accepting both
    // would mean two spellings of one note.
    if week.len() != 2 {
        return None;
    }
    let week: u32 = week.parse().ok()?;
    NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
}

/// `YYYY-MM` → the first of that month.
fn parse_month(raw: &str) -> Option<NaiveDate> {
    let (year, month) = raw.split_once('-')?;
    if year.len() != 4 || month.len() != 2 {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, 1)
}

/// The slug a note is addressed by.
///
/// `_index.md` takes its **parent directory's** name, which is what makes a
/// portfolio (`portfolios/surrogate-model/_index.md`) and an expanded
/// stewardship (`stewardships/gym/_index.md`) reachable under the name people
/// actually use rather than the literal string `_index`. That one rule is
/// what lets this generic scan reproduce `resolve_stewardship_with_variant`'s
/// flat-vs-expanded handling and the portfolio index convention without a
/// per-type resolver for each.
pub(in crate::vault) fn candidate_slug(path: &VaultPath) -> Option<String> {
    let p = path.as_path();
    let stem = p.file_stem()?.to_str()?;
    if stem == "_index" {
        p.parent()?.file_name()?.to_str().map(str::to_owned)
    } else {
        Some(stem.to_owned())
    }
}

impl super::Vault {
    /// Resolve a user-typed reference to an existing note.
    ///
    /// `today` is the caller's clock, stamped at the interface boundary — the
    /// domain never reads the system time.
    ///
    /// Note the deliberate asymmetry between tiers: a **path** reference is
    /// checked against the *store*, while a **slug** is matched against the
    /// *index*. So `cdno open CLAUDE.md` works even in a vault whose `ignore`
    /// globs exclude it — you named the file explicitly — while a bare slug
    /// will not surface an ignored file, consistent with its invisibility to
    /// search and lint.
    pub fn resolve_note_ref(
        &self,
        reference: &str,
        today: NaiveDate,
    ) -> Result<RefResolution, DomainError> {
        let known = self.type_registry();
        let known_names = known.all_names();
        let parsed = NoteRef::parse(reference, &known_names);

        // Which miss this would be, decided by the shape rather than guessed
        // at downstream.
        let miss = match parsed {
            NoteRef::Path(_) => Miss::Path,
            _ => Miss::JournalNote,
        };
        let relpath = match parsed {
            NoteRef::Relative(day) => paths::daily_note_relpath(day.resolve(today)),
            NoteRef::Date(date) => paths::daily_note_relpath(date),
            NoteRef::Period(PeriodRef::Week(date)) => paths::weekly_note_relpath(date),
            NoteRef::Period(PeriodRef::Month(date)) => paths::monthly_note_relpath(date),
            NoteRef::Path(raw) => raw.to_owned(),
            NoteRef::Typed { note_type, slug } => {
                return self.resolve_typed_ref(note_type, slug, reference);
            }
            NoteRef::Bare(slug) => return self.resolve_bare_slug(slug, reference),
        };

        // Everything reaching here named a file outright, whether the user
        // spelled the path or a calendar word did it for them. A miss is a
        // miss: never fall through to fuzzy matching, because someone who
        // typed a path with a typo wants "no such file", not a picker that
        // opens something adjacent.
        let path = VaultPath::new(&relpath)?;
        if self.is_note_file(&path)? {
            return Ok(RefResolution::Resolved(ResolvedNote::direct(path)));
        }
        // A path without `.md` is how a wikilink spells a note
        // (`[[concepts/foo]]`), so a reference copied from one names
        // `concepts/foo.md`. Retried here rather than in each interface so
        // that `cdno open`, `read_note` and every later verb accept the same
        // spellings. Only path-shaped references qualify (the calendar forms
        // already name a `.md` file), and a trailing `/` names a directory,
        // not a note. The miss still reports the reference as typed.
        if matches!(parsed, NoteRef::Path(_))
            && !ends_in_note_extension(&relpath)
            && !relpath.ends_with('/')
        {
            for with_extension in note_format_candidates(&relpath) {
                if self.is_note_file(&with_extension)? {
                    return Ok(RefResolution::Resolved(ResolvedNote::direct(
                        with_extension,
                    )));
                }
            }
        }
        Ok(RefResolution::NotFound {
            reference: reference.to_owned(),
            miss,
        })
    }

    /// Whether `path` names an existing note file. A note extension as well
    /// as existence, because `exists` is true for a directory: without this,
    /// `cdno open projects/` would hand back a directory that no editor can
    /// open and no `$(…)` can use. A Typst file counts only with an index
    /// row, so an attachment never resolves as a note.
    fn is_note_file(&self, path: &VaultPath) -> Result<bool, DomainError> {
        Ok(match note_format(path) {
            Some(NoteFormat::Markdown) => self.store.exists(path)?,
            Some(NoteFormat::Typst) => {
                self.index.find_by_path(path)?.is_some() && self.store.exists(path)?
            }
            None => false,
        })
    }

    /// `type:slug` — scoped to one type, so a slug shared across types is no
    /// longer ambiguous. Still returns [`RefResolution::Ambiguous`] when one
    /// *type* holds two notes of that slug (both stewardship variants, say),
    /// which is the case the typed form cannot disambiguate on its own.
    ///
    /// On no stem match, the alias tier: the one note declaring `slug` as an
    /// alias, provided it is of `note_type`.
    fn resolve_typed_ref(
        &self,
        note_type: &str,
        slug: &str,
        reference: &str,
    ) -> Result<RefResolution, DomainError> {
        let typed: Vec<NoteCandidate> = self
            .index
            .list_candidates()?
            .into_iter()
            .filter(|c| c.note_type == note_type)
            .collect();
        self.resolve_in_pool(typed, slug, reference)
    }

    /// A bare slug, matched across every type, then through the alias tier.
    fn resolve_bare_slug(&self, slug: &str, reference: &str) -> Result<RefResolution, DomainError> {
        let all = self.index.list_candidates()?;
        self.resolve_in_pool(all, slug, reference)
    }

    /// Resolve `slug` among the notes of `pool`: by stem first, and only on
    /// no stem match through the alias tier (RFC 0007 §5.5), so a stem always
    /// beats an alias. The alias tier counts only holders in `pool`, so for
    /// `type:slug` a note of another type declaring the same alias neither
    /// answers nor hides the one that should. One holder resolves; several
    /// are [`RefResolution::Ambiguous`] with exactly those notes, so a picker
    /// offers the notes that claim the name rather than a fuzzy list of
    /// current names that cannot contain it.
    fn resolve_in_pool(
        &self,
        pool: Vec<NoteCandidate>,
        slug: &str,
        reference: &str,
    ) -> Result<RefResolution, DomainError> {
        let hits: Vec<NoteCandidate> = pool
            .iter()
            .filter(|c| candidate_slug(&c.path).as_deref() == Some(slug))
            .cloned()
            .collect();
        if !hits.is_empty() {
            return Ok(Self::narrow(hits, reference));
        }
        let holders = self.alias_holders(slug, |holder| pool.iter().any(|c| &c.path == holder))?;
        Ok(match holders.as_slice() {
            [] => RefResolution::NotFound {
                reference: reference.to_owned(),
                miss: Miss::Slug,
            },
            [holder] => {
                let note = ResolvedNote::direct(holder.clone());
                RefResolution::Resolved(ResolvedNote::via_alias(note.path, note.slug, slug))
            }
            _ => RefResolution::Ambiguous(
                pool.into_iter()
                    .filter(|c| holders.contains(&c.path))
                    .collect(),
            ),
        })
    }

    /// Every distinct note declaring `slug` as an alias that `accepts`
    /// takes, in the index's order. Read from the index's alias rows
    /// ([`VaultIndex::list_aliases`](cdno_core::index::VaultIndex::list_aliases)),
    /// which carry only slug-shaped values; a note listing the alias twice
    /// counts once.
    pub(in crate::vault) fn alias_holders(
        &self,
        slug: &str,
        accepts: impl Fn(&VaultPath) -> bool,
    ) -> Result<Vec<VaultPath>, DomainError> {
        let mut holders: Vec<VaultPath> = Vec::new();
        for (alias, path) in self.index.list_aliases()? {
            if alias == slug && !holders.contains(&path) && accepts(&path) {
                holders.push(path);
            }
        }
        Ok(holders)
    }

    /// Every alias `path` declares, in the index's order, each once: the
    /// inverse of [`alias_holders`](Self::alias_holders), read from the same
    /// `list_aliases` rows.
    pub(in crate::vault) fn aliases_of(
        &self,
        path: &VaultPath,
    ) -> Result<Vec<String>, DomainError> {
        let mut aliases: Vec<String> = Vec::new();
        for (alias, holder) in self.index.list_aliases()? {
            if &holder == path && !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
        Ok(aliases)
    }

    /// The one note declaring `slug` as an alias among those `accepts`
    /// takes, and that note's current slug; `None` when none does, or when
    /// two or more share it.
    ///
    /// The single alias rule every selector shares, so `cdno open`, the
    /// project, question, stewardship and commitment lookups cannot disagree
    /// about which note an old name means. Uniqueness is counted only over
    /// the accepted holders, as the stem tier counts only notes of the type
    /// asked for: each selector passes its own folder or type predicate, so
    /// a hand-written alias on a note of another type cannot hide a project.
    /// Callers consult it only after their own exact lookup missed.
    pub(in crate::vault) fn alias_of(
        &self,
        slug: &str,
        accepts: impl Fn(&VaultPath) -> bool,
    ) -> Result<Option<(VaultPath, String)>, DomainError> {
        let mut holders = self.alias_holders(slug, accepts)?;
        if holders.len() != 1 {
            return Ok(None);
        }
        let path = holders.remove(0);
        Ok(candidate_slug(&path).map(|current| (path, current)))
    }

    /// The form of a bare wikilink `target` (`projects/surrogate-model`,
    /// `questions/research/why`, optionally with a `#Heading` anchor or a
    /// `|label`, which are kept) a verb records in a note it writes.
    ///
    /// A target naming an existing file, or naming nothing at all, is kept
    /// exactly as given: these are loose pointers and their existence is not
    /// required. Only when the file is missing and its last segment is a
    /// former name, the alias of exactly one note `accepts` takes, is that
    /// segment replaced by the note's current slug (RFC 0007 §5.5). The
    /// caller's folder form is kept — `projects/<slug>`, never the parked or
    /// closed path the map happens to sit at — and the last-segment rule of
    /// link resolution finds the note from there, as it does for every link.
    ///
    /// The one place a path-shaped value meets the alias tier: a reference a
    /// person types to *open* a path still never does, but a pointer cdno
    /// *writes* must not carry a name it knows is stale.
    pub(in crate::vault) fn current_link_target(
        &self,
        target: &str,
        accepts: impl Fn(&VaultPath) -> bool,
    ) -> Result<String, DomainError> {
        // An anchor (`#Heading`) or a label (`|text`) is not part of the
        // name: split it off before the lookup and put it back after, as
        // link resolution strips the anchor before it matches a stem.
        let (base, suffix) = target
            .find(['#', '|'])
            .map_or((target, ""), |at| target.split_at(at));
        for path in note_format_candidates(base) {
            if self.is_note_file(&path)? {
                return Ok(target.to_owned());
            }
        }
        let (folder, last) = match base.rsplit_once('/') {
            Some((folder, last)) => (Some(folder), last),
            None => (None, base),
        };
        Ok(match self.alias_of(last, accepts)? {
            Some((_holder, current)) => match folder {
                Some(folder) => format!("{folder}/{current}{suffix}"),
                None => format!("{current}{suffix}"),
            },
            None => target.to_owned(),
        })
    }

    /// Collapse exact-slug hits to a resolution, applying the single
    /// type-specific preference the generic scan cannot infer.
    fn narrow(hits: Vec<NoteCandidate>, reference: &str) -> RefResolution {
        match hits.len() {
            0 => RefResolution::NotFound {
                reference: reference.to_owned(),
                miss: Miss::Slug,
            },
            1 => RefResolution::Resolved(ResolvedNote::direct(hits[0].path.clone())),
            _ => {
                // An active project beats its parked namesake. This is a
                // *read* rule: writes go through `locate_project`, which
                // refuses a duplicate as `AmbiguousProject`, while a read
                // that only wants to open the note prefers the active map.
                // A map under `_parked/` or `_done/<year>/` never counts as
                // active.
                //
                // `park_project` moves the file rather than copying it, so
                // cdno itself never produces this state. It arises from a
                // hand-edit, an interrupted sync, or a migration — and there
                // the two paths are one project in two places, not two
                // projects, so preferring the active one is a documented
                // rule rather than a guess. Defensive, not load-bearing.
                let projects_only = hits.iter().all(|c| c.note_type == "project");
                let mut unparked = hits.iter().filter(|c| {
                    let p = c.path.as_path();
                    !p.starts_with(paths::PROJECTS_PARKED) && !p.starts_with(paths::PROJECTS_DONE)
                });
                // Exactly one active hit, not merely at least one. With a
                // bare `find`, two *active* projects sharing a slug would
                // resolve to whichever the index returned first — the silent
                // wrong-file open this enum exists to prevent.
                if projects_only && let (Some(active), None) = (unparked.next(), unparked.next()) {
                    return RefResolution::Resolved(ResolvedNote::direct(active.path.clone()));
                }
                RefResolution::Ambiguous(hits)
            }
        }
    }

    /// Every indexed note as an addressable candidate, most-recently-modified
    /// first. Backs the `cdno open` picker, its `--list` output, and shell
    /// completion.
    pub fn list_note_candidates(&self) -> Result<Vec<NoteCandidate>, DomainError> {
        Ok(self.index.list_candidates()?)
    }

    /// Whether a note's existing content is frozen — an archived action whose
    /// text was hashed at archival.
    ///
    /// Editing the frozen prefix is a `cdno lint` error (appending past it is
    /// not), so an interface that is about to hand the file to an editor can
    /// say so first. A question, not an enforcement: markdown stays the source
    /// of truth and nothing here prevents the edit.
    pub fn is_frozen(&self, path: &VaultPath) -> Result<bool, DomainError> {
        Ok(self.index.find_archival_snapshot(path)?.is_some())
    }
}
