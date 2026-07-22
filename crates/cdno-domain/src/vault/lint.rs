//! `Vault::lint_all_notes`.

use std::collections::{HashMap, HashSet};
use std::path::Component;
use std::sync::Arc;

use chrono::NaiveDate;

use cdno_core::config::IgnoreSet;
use cdno_core::extractors::{extract_wikilinks, resolve_wikilinks};
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;

use crate::error::DomainError;
use crate::lint::{LintIssue, LintReport};
use crate::note_type::NoteType;

use super::Vault;
use super::commitments::parse_periodic_line;
use super::orient::{ACTIVE_HABITS_SECTION, parse_habit_line};
use super::stewardships::PERIODIC_COMMITMENTS_SECTION;

impl Vault {
    /// Validate every indexed note and return a structured report.
    ///
    /// The pass is read-only and skips any file that's not in the
    /// index — non-markdown attachments (PDFs, notebooks) and any
    /// file under `.cuaderno/` are by definition not present, so the
    /// scope of `lint` is exactly "the notes the index knows about".
    ///
    /// Today's checks:
    /// - the entry's `type` field parses as a known [`NoteType`];
    /// - every field listed in the type's `[schemas.<type>]
    ///   extra_required` config section is present in the
    ///   frontmatter;
    /// - value type-mismatch on a declared `[schemas.<type>.fields]`
    ///   field: a present field whose value doesn't match its declared
    ///   `FieldType` (or `values` constraint) — a `Warning`, opt-in per
    ///   type, #301. (The undeclared-key check is deferred — the correct
    ///   allowed-set isn't exposed yet.)
    /// - append-only-after-completion on archived action notes;
    /// - attachment stub / artefact-folder pairing;
    /// - broken wikilinks: body links that resolve to no note
    ///   (a `Warning`, not an `Error` -- the note is structurally fine);
    /// - frontmatter-order drift: keys not in the effective template's
    ///   canonical order (a `Warning`; `cdno normalise` fixes it, #236).
    /// - malformed stewardship-dashboard bullets: `## Active Habits` and
    ///   `## Periodic Commitments` lines the canonical parsers reject —
    ///   the near-misses that would otherwise vanish silently from the
    ///   lapse scan and the commitments aggregation (a `Warning`, #312).
    ///
    /// Per-type structural checks (e.g. `ProjectFrontmatter` invariants)
    /// land alongside their domain code in Phase 2/3.
    pub fn lint_all_notes(&self) -> Result<LintReport, DomainError> {
        let mut issues: Vec<LintIssue> = Vec::new();

        // The full note-path set, used to resolve wikilinks below.
        // Built once and shared across every note's link check.
        let paths = self.index.list_all_paths()?;
        let path_set: HashSet<VaultPath> = paths.iter().cloned().collect();

        // Memoise canonical frontmatter order per (type, variant) for this
        // pass so the effective template is resolved once per key, not once
        // per note (#248). Keyed by the type *string* so config-defined custom
        // types share the cache with built-ins.
        let mut order_cache: HashMap<(String, Option<String>), Vec<String>> = HashMap::new();

        for path in paths {
            // A concurrent writer could remove a note between the
            // listing and the lookup. Treat that as "nothing to lint
            // here" rather than a hard error — the next pass will see
            // a coherent state.
            let Some(entry) = self.index.find_by_path(&path)? else {
                continue;
            };

            // The reconciler enforces that every indexed note has a `type`
            // field, but it does not constrain the value to a known type. A
            // type that is neither a built-in variant nor a config-defined
            // custom type means downstream code can't pick a schema, so flag it
            // and move on rather than compounding the report.
            let Some(descriptor) = self.type_registry().resolve(&entry.note_type) else {
                issues.push(LintIssue::error(
                    path,
                    format!("unknown note type: `{}`", entry.note_type),
                ));
                continue;
            };

            // Required fields: a built-in type's `[schemas.<type>].extra_required`
            // additions, or a config type's declared `required`. (A built-in's
            // *intrinsic* fields are enforced by its typed parse, not here.)
            for required in descriptor.required_fields(&self.config) {
                let present = entry
                    .frontmatter
                    .as_object()
                    .and_then(|obj| obj.get(required))
                    .is_some_and(|v| !v.is_null());
                if !present {
                    issues.push(LintIssue::error(
                        path.clone(),
                        format!(
                            "missing required field `{required}` for note type `{}`",
                            entry.note_type
                        ),
                    ));
                }
            }

            // Declared-field value type-check (#301). Opt-in: runs only for a
            // type carrying an explicit `[schemas.<type>.fields]` block, so a
            // vault using only the legacy `extra_required` (or none) is
            // unaffected. For each declared field *present* in the note's
            // frontmatter, warn (never error) when its value doesn't match the
            // declared `FieldType` (or its `values` constraint). Presence is out
            // of scope here — the `required_fields` error above covers missing
            // built-in-schema fields, and the undeclared-key lint is deferred.
            // Reuses the canonical `FieldSpec::check_value`, never a parallel
            // parse. Field names are sorted so the report is deterministic.
            if let Some(schema) = self.config.schema_for(&entry.note_type)
                && !schema.fields.is_empty()
            {
                let obj = entry.frontmatter.as_object();
                let mut declared: Vec<(String, cdno_core::config::FieldSpec)> =
                    schema.declared_fields().into_iter().collect();
                declared.sort_by(|a, b| a.0.cmp(&b.0));
                for (field, spec) in declared {
                    let Some(value) = obj.and_then(|o| o.get(&field)) else {
                        continue;
                    };
                    if value.is_null() {
                        continue;
                    }
                    if let Some(reason) = spec.check_value(value) {
                        issues.push(LintIssue::warning(
                            path.clone(),
                            format!(
                                "field `{field}` {reason} for note type `{}`",
                                entry.note_type
                            ),
                        ));
                    }
                }
            }

            // Append-only-after-completion (design §5.11, #111).
            // Archived action notes (`actions/_done/<year>/`) may grow
            // new lines but their pre-archival prefix is frozen. We
            // verify by re-hashing the first `frozen_size` bytes and
            // comparing to the snapshot recorded at archival time.
            if entry.note_type == "action"
                && path.as_path().starts_with(cdno_core::paths::ACTIONS_DONE)
                && let Some(snap) = self.index.find_archival_snapshot(&path)?
                && let Some(msg) = check_append_only(&self.store, &path, &snap)?
            {
                issues.push(LintIssue::error(path.clone(), msg));
            }

            // Attachment stub ↔ artefact-folder pairing, forward
            // direction (#154). An evidence note carrying a `kind`
            // field is an attachment stub: it links a non-markdown
            // artefact living in a sibling folder named after the
            // stub's stem (`portfolios/<p>/<stem>.md` pairs with
            // `portfolios/<p>/<stem>/`). The artefacts aren't indexed
            // (only the stub is), so lint is the only thing that can
            // notice the folder went missing — e.g. the artefacts were
            // hand-deleted while the stub survived.
            if entry.note_type == "evidence"
                && entry
                    .frontmatter
                    .as_object()
                    .and_then(|obj| obj.get("kind"))
                    .is_some_and(|v| !v.is_null())
                && let Ok(folder) = VaultPath::new(path.as_path().with_extension(""))
                // A missing folder reads back as `Ok(empty)` — both stores
                // normalise "no such dir" to an empty listing — so that's
                // the case we flag. A genuine `Err` is an I/O fault, not a
                // pairing problem; don't manufacture an issue from it.
                && self
                    .store
                    .list_dir(&folder)
                    .map(|c| c.is_empty())
                    .unwrap_or(false)
            {
                issues.push(LintIssue::error(
                    path.clone(),
                    format!(
                        "attachment evidence links artefacts in `{folder}` but that \
                         folder is missing or empty"
                    ),
                ));
            }

            // Broken-wikilink check (#205). Body-scanned (frontmatter
            // links like `project:`/`origin:` are out of scope for lint —
            // the reconciler's backlink graph now indexes them (#395), but
            // lint deliberately re-extracts the body here) and resolved
            // against the *current* path set rather than the index's stored
            // resolution -- which goes stale when a link target is
            // deleted without the linking note changing (the
            // reconciler's mtime fast-path skips it). A dangling link
            // is a Warning: the note parses, a link just points
            // nowhere. This is the check that would have caught the
            // #200 dangling backlink (`[[portfolios/<slug>]]` instead
            // of `[[portfolios/<slug>/_index]]`).
            //
            // Read/parse failures here are reported as an Error against
            // this note and skipped, never propagated: a note that's
            // indexed but corrupt on disk (a stale index row the
            // reconciler couldn't refresh) is exactly what lint exists
            // to surface -- aborting the whole run would instead hide
            // every other issue.
            let content = match self.store.read_file(&path) {
                Ok(c) => c,
                Err(e) => {
                    issues.push(LintIssue::error(
                        path.clone(),
                        format!("could not read note: {e}"),
                    ));
                    continue;
                }
            };
            let body = match Frontmatter::parse(&content) {
                Ok((_fm, body)) => body,
                Err(e) => {
                    issues.push(LintIssue::error(
                        path.clone(),
                        format!("malformed frontmatter: {e}"),
                    ));
                    continue;
                }
            };

            // Frontmatter-order drift (#236): keys not in the effective
            // template's canonical order. A Warning -- the note is valid,
            // just untidy; `cdno normalise` reorders it. Computed via the
            // same order `normalise` would apply, so lint and the fixer
            // never disagree. A broken template surfaces as an Error
            // rather than aborting the whole pass.
            match self.canonical_frontmatter_order(&entry.note_type, &content, &mut order_cache) {
                Ok(order) => {
                    if super::normalise::reorder_frontmatter(&content, &order).is_some() {
                        issues.push(LintIssue::warning(
                            path.clone(),
                            "frontmatter keys are not in canonical order \
                             (run `cdno normalise` to fix)"
                                .to_owned(),
                        ));
                    }
                }
                Err(e) => issues.push(LintIssue::error(
                    path.clone(),
                    format!("could not resolve canonical frontmatter order: {e}"),
                )),
            }

            for link in resolve_wikilinks(extract_wikilinks(body), &path_set) {
                if link.resolved_path.is_none() {
                    issues.push(LintIssue::warning(
                        path.clone(),
                        format!(
                            "broken wikilink `[[{}]]` resolves to no note",
                            link.target_raw
                        ),
                    ));
                }
            }
        }

        // `path_set` is the indexed notes; the ignore matcher is compiled
        // from the same config reconciliation used, so lint and the index
        // agree on what is not a note.
        let ignore = self.config.ignore_set()?;
        issues.extend(orphan_artefact_issues(&self.store, &path_set, &ignore)?);
        issues.extend(self.stewardship_dashboard_issues()?);

        Ok(LintReport { issues })
    }

    /// Scan every stewardship dashboard for malformed `## Active Habits`
    /// and `## Periodic Commitments` bullets (#312).
    ///
    /// The lapse scan ([`Vault::lapsed_habits`]) and the periodic
    /// commitment aggregator both skip lines they can't parse *by
    /// design* — a hand-typed near-miss (an ASCII hyphen or en-dash
    /// where the em-dash belongs, a missing `next:` marker, an
    /// unparseable date) therefore disappears with no diagnostic
    /// anywhere. This rule turns that silent skip into a visible
    /// `Warning`.
    ///
    /// Acceptance is delegated to the *canonical* parsers, never a
    /// parallel regex that could drift: a bullet is a near-miss exactly
    /// when [`parse_habit_line`] (habits) or [`parse_periodic_line`]
    /// (commitments) rejects it. Only list bullets (`- ...`) are
    /// checked, so section prose, blank lines and the heading itself are
    /// never flagged. The hint that follows is a cheap heuristic guess
    /// at the likely typo; it never changes the accept/reject verdict.
    fn stewardship_dashboard_issues(&self) -> Result<Vec<LintIssue>, DomainError> {
        let mut issues = Vec::new();
        for entry in self.index.list_by_type(NoteType::Stewardship.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            // A dashboard whose markdown won't parse is already surfaced
            // by the read/frontmatter checks in the main loop; a section
            // scan has nothing further to add, so move on.
            let Ok(doc) = MarkdownDocument::parse(raw) else {
                continue;
            };

            // Active Habits — canonical shape `- {habit} — {status}`.
            if let Ok(section) = doc.section(ACTIVE_HABITS_SECTION) {
                for line in section.lines() {
                    if is_dashboard_bullet(line) && parse_habit_line(line).is_none() {
                        issues.push(LintIssue::warning(
                            entry.path.clone(),
                            format!(
                                "malformed Active Habits line `{}` -- {}",
                                line.trim(),
                                habit_line_hint(line)
                            ),
                        ));
                    }
                }
            }

            // Periodic Commitments — canonical shape
            // `- {title} — {recurrence} — next: YYYY-MM-DD`.
            if let Ok(section) = doc.section(PERIODIC_COMMITMENTS_SECTION) {
                for line in section.lines() {
                    if is_dashboard_bullet(line) && parse_periodic_line(line).is_none() {
                        issues.push(LintIssue::warning(
                            entry.path.clone(),
                            format!(
                                "malformed Periodic Commitments line `{}` -- {}",
                                line.trim(),
                                periodic_line_hint(line)
                            ),
                        ));
                    }
                }
            }
        }
        Ok(issues)
    }
}

/// A dashboard-section line is a candidate bullet when, ignoring leading
/// whitespace, it opens with the markdown list marker `- ` — the exact
/// prefix both canonical parsers key on. Gating on this first means a
/// line the parser could ever accept always reaches the parser, while
/// prose, blank lines and the section heading never do and so can never
/// be flagged.
///
/// Known limit, shared with the parsers by construction: `*`/`+` list
/// markers and `-` without a trailing space are invisible to both, so a
/// wrong-marker near-miss still vanishes without a diagnostic. Closing
/// that would mean widening this gate beyond what the canonical grammar
/// accepts — out of scope for #312.
fn is_dashboard_bullet(line: &str) -> bool {
    line.trim_start().starts_with("- ")
}

/// The bullet body a hint inspects: everything after the `- ` marker.
/// The callers only reach a hint for lines that already passed
/// [`is_dashboard_bullet`], so the prefix is always present.
fn bullet_body(line: &str) -> &str {
    line.trim_start().strip_prefix("- ").unwrap_or("").trim()
}

/// Best-effort guess at *why* an `## Active Habits` bullet fails the
/// `- {habit} — {status}` grammar. Heuristic only — the accept/reject
/// verdict is [`parse_habit_line`]'s; this just names the most likely
/// typo so the fix is obvious. Ordered most-specific first.
fn habit_line_hint(line: &str) -> &'static str {
    let body = bullet_body(line);
    if body.contains('\u{2014}') {
        // The separator is present, so one side must be empty. Checked
        // before any dash guess: an en-dash inside the habit *name* is
        // legitimate, and the hint must not blame a dash that isn't
        // broken when the real defect is elsewhere.
        "the habit text or the status either side of the em-dash is empty"
    } else if body.contains('\u{2013}') {
        // En-dash: the near-miss the naked eye can't tell from an em-dash.
        "found an en-dash (\u{2013}) where an em-dash (\u{2014}) separates habit from status"
    } else if body.contains(" - ") {
        // ASCII hyphen standing in for the em-dash separator.
        "found an ASCII hyphen (-) where an em-dash (\u{2014}) separates habit from status"
    } else {
        "missing the em-dash (\u{2014}) that separates habit from status"
    }
}

/// Best-effort guess at *why* a `## Periodic Commitments` bullet fails
/// the `- {title} — {recurrence} — next: YYYY-MM-DD` grammar. Heuristic
/// only, mirroring [`habit_line_hint`]: the verdict is
/// [`parse_periodic_line`]'s. Staged so the most actionable pointer wins.
fn periodic_line_hint(line: &str) -> &'static str {
    let body = bullet_body(line);
    // The grammar needs two em-dashes: `title — recurrence — next: date`.
    let parts: Vec<&str> = body.splitn(3, '\u{2014}').collect();
    if parts.len() < 3 {
        // Only blame a dash once the em-dash structure is known to be
        // incomplete: an en-dash inside a *title* (`Q1\u{2013}Q2 review`)
        // is legitimate, and a line failing on a missing `next:` or a bad
        // date must not be pointed at a dash that isn't broken.
        if body.contains('\u{2013}') {
            return "found an en-dash (\u{2013}) where an em-dash (\u{2014}) is expected";
        }
        if body.contains(" - ") {
            return "found an ASCII hyphen (-) where an em-dash (\u{2014}) is expected";
        }
        return "expected `Title \u{2014} recurrence \u{2014} next: YYYY-MM-DD` (needs two em-dashes)";
    }
    let next_part = parts[2].trim();
    let Some(after_marker) = next_part.strip_prefix("next:") else {
        return "missing the `next:` marker before the date";
    };
    // Mirror the parser: a trailing `(overdue)` annotation is tolerated,
    // so only the first whitespace-delimited token is the date.
    let date_str = after_marker.split_whitespace().next().unwrap_or("");
    if NaiveDate::parse_from_str(date_str, "%Y-%m-%d").is_err() {
        return "unparseable date after `next:` (expected YYYY-MM-DD)";
    }
    // Structure looks right but the parser still rejected it (e.g. an
    // empty title) — a generic pointer beats a confidently wrong guess.
    "does not match `Title \u{2014} recurrence \u{2014} next: YYYY-MM-DD`"
}

/// Attachment stub ↔ artefact-folder pairing, reverse direction (#154).
///
/// Walks the files under `portfolios/` and reports any subfolder holding
/// files that are neither notes nor claimed by an evidence stub — the
/// mirror of the forward check above. Reconciliation excludes a stub's
/// artefacts from the index ([`cdno_core::artefacts::owning_artefact_stub`]),
/// so this filesystem walk is the only way to catch a folder whose stub was
/// hand-deleted or moved, leaving evidence invisible to every structural
/// retrieval.
///
/// Ownership is resolved with the same helper reconciliation uses, so the
/// two can never disagree about what an artefact is. That also makes the
/// check depth-independent: a stub owns its folder however deeply the
/// artefact sits, which the previous fixed three-segment shape missed
/// (#451).
///
/// Three things are exempt, and between them they confine the check to what
/// it is actually for:
///
/// - **Notes.** A file in the index is a note, wherever it sits, so it can
///   never make its folder an orphan. Without this, an evidence stub filed
///   one level down would report *its own* folder as unowned — and the
///   remedy the message names (create `<folder>.md`) is precisely what
///   would make reconciliation treat that stub as an artefact and drop it
///   from the index. Lint must not hand out advice that loses notes.
/// - **Ignored files.** `ignore` globs are the user saying "not a note";
///   lint walks the store directly, so it has to honour them itself or it
///   reports on files the rest of the tool has been told to disregard.
/// - **Markdown owned by a stub**, like any other artefact — filing a `.md`
///   document produces exactly the same stub-plus-folder pair as filing a
///   PDF.
///
/// Folders are reported at most once regardless of how many artefacts
/// they hold, and at the outermost ancestor that holds no note (see
/// [`orphan_folder_for`]), so a deep tree yields one finding rather than
/// one per file — but a detached folder inside a grouping folder is still
/// named individually rather than being swallowed by it. Files sitting
/// directly at a portfolio's root are left alone — they are notes, or
/// strays, but never orphaned artefacts.
///
/// The message still hedges ("orphaned attachment or stray file") rather
/// than asserting the file *is* a detached artefact: an unowned
/// non-markdown file could equally be something dropped in by hand.
fn orphan_artefact_issues(
    store: &Arc<dyn VaultStore>,
    notes: &HashSet<VaultPath>,
    ignore: &IgnoreSet,
) -> Result<Vec<LintIssue>, DomainError> {
    let Ok(root) = VaultPath::new(cdno_core::paths::PORTFOLIOS) else {
        return Ok(Vec::new());
    };
    // A vault with no portfolios yet has no `portfolios/` directory;
    // that's not an error, just nothing to check.
    let Ok(files) = store.walk_dir(&root) else {
        return Ok(Vec::new());
    };

    // Every folder holding an indexed note, at any depth. A stub named
    // after one of these would claim those notes as artefacts, so no
    // finding may ever name one.
    //
    // Marked for *all* ancestors of each note, not just the fixed
    // `portfolios/<p>/<folder>` level, because the walk below relies on the
    // transitive property: if a folder holds a note then so does every
    // folder containing it.
    let mut folders_with_notes: HashSet<VaultPath> = HashSet::new();
    for file in &files {
        if !notes.contains(file) {
            continue;
        }
        for dir in portfolio_ancestors(file) {
            folders_with_notes.insert(dir);
        }
    }

    // Stub-ness costs a read and a YAML parse, and the ancestor walk probes
    // the same candidate once per file beneath it. Memoise per pass.
    let mut stub_cache: HashMap<VaultPath, bool> = HashMap::new();

    let mut issues = Vec::new();
    let mut seen: HashSet<VaultPath> = HashSet::new();
    for file in &files {
        if notes.contains(file) || ignore.is_match(file.as_path()) {
            continue;
        }
        let owned = cdno_core::artefacts::owning_artefact_stub(file, |stub| {
            *stub_cache
                .entry(stub.clone())
                .or_insert_with(|| cdno_core::artefacts::is_attachment_stub(store, stub))
        })
        .is_some();
        if owned {
            continue;
        }
        let Some(folder) = orphan_folder_for(file, &folders_with_notes) else {
            continue;
        };
        if !seen.insert(folder.clone()) {
            continue;
        }
        // Name the stub the folder would pair with, so the message points
        // at the exact file to restore. When that file is already there but
        // isn't a stub, say so rather than calling it missing — otherwise
        // the message names a file the user can plainly see.
        let stub = format!("{folder}.md");
        let stub_present = VaultPath::new(&stub)
            .ok()
            .and_then(|p| store.exists(&p).ok())
            .unwrap_or(false);
        let claim = if stub_present {
            format!(
                "artefact folder `{folder}` is not claimed by `{stub}`, which is not an \
                 attachment stub (an evidence note carrying a `kind`)"
            )
        } else {
            format!("artefact folder `{folder}` has no evidence stub `{stub}`")
        };
        // Unindexed markdown in the folder is ambiguous: a filed document
        // whose stub was lost, or a note whose frontmatter is broken (which
        // reconciliation reports separately). Creating the stub would be
        // right for the first and would permanently hide the second, so the
        // message must not prescribe it blindly.
        let holds_markdown = files.iter().any(|f| {
            f.as_path().extension() == Some(std::ffi::OsStr::new("md"))
                && f.as_path().starts_with(folder.as_path())
        });
        let message = if holds_markdown {
            format!(
                "{claim} -- it holds markdown that is not indexed: either notes whose \
                 frontmatter needs fixing, or filed documents whose stub was lost \
                 (restore the stub only in the second case)"
            )
        } else {
            format!("{claim} -- orphaned attachment or stray file")
        };
        issues.push(LintIssue::error(folder, message));
    }
    Ok(issues)
}

/// `file`'s ancestor directories that could be artefact folders —
/// everything from `portfolios/<p>/<folder>` down to the file's immediate
/// parent — ordered nearest first.
///
/// Empty when the file sits at a portfolio's root or outside `portfolios/`
/// entirely: those are notes, or strays, but never orphaned artefacts.
fn portfolio_ancestors(file: &VaultPath) -> Vec<VaultPath> {
    // Bail on anything that isn't a plain UTF-8 component rather than
    // filtering it out: dropping one shifts every later index, which would
    // name a folder that does not exist (or miss the orphan entirely).
    let mut segments: Vec<&str> = Vec::new();
    for component in file.as_path().components() {
        match component {
            Component::Normal(s) => match s.to_str() {
                Some(text) => segments.push(text),
                None => return Vec::new(),
            },
            _ => return Vec::new(),
        }
    }
    // `portfolios/<p>/<folder>/…/<file>` — four components minimum.
    if segments.len() < 4 || segments[0] != cdno_core::paths::PORTFOLIOS {
        return Vec::new();
    }
    // Nearest first: the file's parent, then outward to `portfolios/<p>/<folder>`.
    (3..segments.len())
        .rev()
        .filter_map(|end| VaultPath::new(segments[..end].join("/")).ok())
        .collect()
}

/// The folder to report `file` against: the **outermost** ancestor that
/// holds no note, or `None` when even its immediate parent holds one.
///
/// Outermost, so a deep tree of stray files yields one finding naming the
/// folder a user would actually act on rather than one per subdirectory.
/// But never past a folder holding a note, because the remedy a finding
/// implies — pairing the folder with an evidence stub — would claim every
/// note beneath it as an artefact and drop them from the index.
///
/// Stopping at the boundary rather than suppressing the whole subtree is
/// what keeps orphan detection alive inside a grouping folder: a stub
/// hand-deleted from `portfolios/<p>/<group>/<stem>/` is still reported,
/// against `<stem>`, even though `<group>` holds notes. Suppressing
/// instead would blind the check to every detached sibling of any
/// surviving stub — a stub is itself an indexed note.
fn orphan_folder_for(
    file: &VaultPath,
    folders_with_notes: &HashSet<VaultPath>,
) -> Option<VaultPath> {
    let mut safe = None;
    for dir in portfolio_ancestors(file) {
        // Notes are transitive: once an ancestor holds one, so does every
        // folder containing it, so nothing further out can be safe.
        if folders_with_notes.contains(&dir) {
            break;
        }
        safe = Some(dir);
    }
    safe
}

/// Compare the current file against an archival snapshot. Returns
/// `None` when the file is unchanged or has only grown past the frozen
/// prefix (both allowed); returns a `Some(message)` describing the
/// violation otherwise.
fn check_append_only(
    store: &std::sync::Arc<dyn cdno_core::store::VaultStore>,
    path: &cdno_core::path::VaultPath,
    snap: &cdno_core::index::ArchivalSnapshot,
) -> Result<Option<String>, DomainError> {
    let content = store.read_file(path)?;
    let bytes = content.as_bytes();
    let frozen = snap.frozen_size as usize;
    if bytes.len() < frozen {
        return Ok(Some(format!(
            "archived action note was truncated below its frozen prefix \
             (was {} bytes at archival, now {})",
            snap.frozen_size,
            bytes.len(),
        )));
    }
    // The frozen prefix was valid UTF-8 (it's exactly the file content
    // at archival), so slicing on `frozen` lands on a char boundary
    // both then and now — any insert that disturbs the boundary would
    // already perturb the hash and flag below.
    let prefix = &content[..frozen];
    if cdno_core::hash::content_hash(prefix) != snap.frozen_hash {
        return Ok(Some(
            "archived action note modified an existing line (append-only after completion)"
                .to_owned(),
        ));
    }
    Ok(None)
}
