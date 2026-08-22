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
//! **What this deliberately does not do:** fuzzy matching. An exact-slug miss
//! comes back as [`RefResolution::NotFound`], and the interface layer decides
//! whether to offer a picker or error. Keeping the matcher in one place (the
//! CLI, which owns `inquire`'s) stops the auto-open rule and the picker from
//! disagreeing about what "matches".

use chrono::NaiveDate;

use cdno_core::index::NoteCandidate;
use cdno_core::path::VaultPath;
use cdno_core::paths;

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
    fn resolve(self, today: NaiveDate) -> NaiveDate {
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
    Resolved(VaultPath),
    /// One slug, several notes. Never resolved by preference order: guessing
    /// silently opens the wrong file, which is the one failure a navigation
    /// verb cannot afford, because you find out only after typing into it.
    Ambiguous(Vec<NoteCandidate>),
    NotFound {
        reference: String,
        miss: Miss,
    },
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

        if trimmed.contains('/') || trimmed.ends_with(".md") {
            return Self::Path(trimmed);
        }

        Self::Bare(trimmed)
    }
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
        // `.md` as well as existence, because `exists` is true for a
        // directory: without this, `cdno open projects/` would hand back a
        // directory that no editor can open and no `$(…)` can use. A note is
        // always a markdown file.
        let is_note_file =
            path.as_path().extension().is_some_and(|e| e == "md") && self.store.exists(&path)?;
        if is_note_file {
            Ok(RefResolution::Resolved(path))
        } else {
            Ok(RefResolution::NotFound {
                reference: reference.to_owned(),
                miss,
            })
        }
    }

    /// `type:slug` — scoped to one type, so a slug shared across types is no
    /// longer ambiguous. Still returns [`RefResolution::Ambiguous`] when one
    /// *type* holds two notes of that slug (both stewardship variants, say),
    /// which is the case the typed form cannot disambiguate on its own.
    fn resolve_typed_ref(
        &self,
        note_type: &str,
        slug: &str,
        reference: &str,
    ) -> Result<RefResolution, DomainError> {
        let hits: Vec<NoteCandidate> = self
            .index
            .list_candidates()?
            .into_iter()
            .filter(|c| c.note_type == note_type)
            .filter(|c| candidate_slug(&c.path).as_deref() == Some(slug))
            .collect();
        Ok(Self::narrow(hits, reference))
    }

    /// A bare slug, matched across every type.
    fn resolve_bare_slug(&self, slug: &str, reference: &str) -> Result<RefResolution, DomainError> {
        let hits: Vec<NoteCandidate> = self
            .index
            .list_candidates()?
            .into_iter()
            .filter(|c| candidate_slug(&c.path).as_deref() == Some(slug))
            .collect();
        Ok(Self::narrow(hits, reference))
    }

    /// Collapse exact-slug hits to a resolution, applying the single
    /// type-specific preference the generic scan cannot infer.
    fn narrow(hits: Vec<NoteCandidate>, reference: &str) -> RefResolution {
        match hits.len() {
            0 => RefResolution::NotFound {
                reference: reference.to_owned(),
                miss: Miss::Slug,
            },
            1 => RefResolution::Resolved(hits[0].path.clone()),
            _ => {
                // An active project beats its parked namesake, mirroring
                // `resolve_project_path` (commitments.rs).
                //
                // `park_project` moves the file rather than copying it, so
                // cdno itself never produces this state. It arises from a
                // hand-edit, an interrupted sync, or a migration — and there
                // the two paths are one project in two places, not two
                // projects, so preferring the active one is a documented
                // rule rather than a guess. Defensive, not load-bearing.
                let projects_only = hits.iter().all(|c| c.note_type == "project");
                let mut unparked = hits
                    .iter()
                    .filter(|c| !c.path.as_path().starts_with(paths::PROJECTS_PARKED));
                // Exactly one unparked hit, not merely at least one. With a
                // bare `find`, two *unparked* projects sharing a slug would
                // resolve to whichever the index returned first — the silent
                // wrong-file open this enum exists to prevent.
                if projects_only && let (Some(active), None) = (unparked.next(), unparked.next()) {
                    return RefResolution::Resolved(active.path.clone());
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
