//! `Vault::lint_all_notes`.

use std::collections::{HashMap, HashSet};
use std::path::Component;
use std::str::FromStr;
use std::sync::Arc;

use cdno_core::extractors::{extract_wikilinks, resolve_wikilinks};
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;

use crate::error::DomainError;
use crate::lint::{LintIssue, LintReport};
use crate::note_type::NoteType;

use super::Vault;

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
    /// - append-only-after-completion on archived action notes;
    /// - attachment stub / artefact-folder pairing;
    /// - broken wikilinks: body links that resolve to no note
    ///   (a `Warning`, not an `Error` -- the note is structurally fine);
    /// - frontmatter-order drift: keys not in the effective template's
    ///   canonical order (a `Warning`; `cdno normalise` fixes it, #236).
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
        // per note (#248).
        let mut order_cache: HashMap<(NoteType, Option<String>), Vec<String>> = HashMap::new();

        for path in paths {
            // A concurrent writer could remove a note between the
            // listing and the lookup. Treat that as "nothing to lint
            // here" rather than a hard error — the next pass will see
            // a coherent state.
            let Some(entry) = self.index.find_by_path(&path)? else {
                continue;
            };

            // The reconciler enforces that every indexed note has a
            // `type` field, but it does not constrain the value to a
            // known variant. An unknown type means downstream code
            // can't pick a schema, so don't bother checking
            // `extra_required` — the report would just compound a
            // problem the user already needs to fix.
            let note_type = match NoteType::from_str(&entry.note_type) {
                Ok(t) => t,
                Err(_) => {
                    issues.push(LintIssue::error(
                        path,
                        format!("unknown note type: `{}`", entry.note_type),
                    ));
                    continue;
                }
            };

            for required in self.config.extra_required_fields(&entry.note_type) {
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
            // links like `project:`/`origin:` are out of scope, matching
            // the reconciler's link graph) and resolved against the
            // *current* path set rather than the index's stored
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
            match self.canonical_frontmatter_order(note_type, &content, &mut order_cache) {
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

        issues.extend(orphan_artefact_issues(&self.store)?);

        Ok(LintReport { issues })
    }
}

/// Attachment stub ↔ artefact-folder pairing, reverse direction (#154).
///
/// Walks the non-markdown artefacts under `portfolios/` and reports any
/// whose evidence stub is gone — the mirror of the forward check above.
/// We look for the shape we create: `portfolios/<p>/<stem>/<artefact>`,
/// which must be paired with the stub `portfolios/<p>/<stem>.md`. The
/// artefacts are not indexed, so this filesystem walk is the only way to
/// catch a folder whose stub was hand-deleted or moved, leaving evidence
/// invisible to every structural retrieval.
///
/// Folders are reported at most once regardless of how many artefacts
/// they hold. Anything that doesn't match the exact three-segment shape
/// (markdown files, stray top-level non-markdown, deeper nesting we never
/// generate) is left alone — lint is best-effort, not a fsck.
///
/// Shape alone can't distinguish an artefact folder from a hand-made
/// grouping subfolder a user might drop under a portfolio, so the message
/// hedges ("orphaned attachment or stray file") rather than asserting the
/// file *is* a detached artefact.
fn orphan_artefact_issues(store: &Arc<dyn VaultStore>) -> Result<Vec<LintIssue>, DomainError> {
    let Ok(root) = VaultPath::new(cdno_core::paths::PORTFOLIOS) else {
        return Ok(Vec::new());
    };
    // A vault with no portfolios yet has no `portfolios/` directory;
    // that's not an error, just nothing to check.
    let Ok(artefacts) = store.walk_dir(&root) else {
        return Ok(Vec::new());
    };

    let mut issues = Vec::new();
    let mut seen: HashSet<VaultPath> = HashSet::new();
    for file in artefacts {
        let p = file.as_path();
        if p.extension().and_then(|e| e.to_str()) == Some("md") {
            continue;
        }
        let Some(parent) = p.parent() else { continue };
        let segments: Vec<&str> = parent
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        // Exactly `portfolios/<p>/<stem>` — the folder an attachment
        // stub owns. Deeper paths aren't a layout we produce.
        if segments.len() != 3 || segments[0] != cdno_core::paths::PORTFOLIOS {
            continue;
        }
        let Ok(folder) = VaultPath::new(parent) else {
            continue;
        };
        if !seen.insert(folder.clone()) {
            continue;
        }
        let Ok(stub) = VaultPath::new(parent.with_extension("md")) else {
            continue;
        };
        if !store.exists(&stub).unwrap_or(false) {
            let message = format!(
                "artefact folder `{folder}` has no evidence stub `{stub}` \
                 -- orphaned attachment or stray non-markdown file"
            );
            issues.push(LintIssue::error(folder, message));
        }
    }
    Ok(issues)
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
