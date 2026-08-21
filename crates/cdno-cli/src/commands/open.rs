//! `cdno open [REFERENCE] [--path] [--list]` — reach a note.
//!
//! The navigation counterpart to `cdno search`. Search answers "where did I
//! write about X"; this answers "take me to the note I mean", and the two are
//! kept apart deliberately — a single query verb that also navigated would
//! make the user choose between synonyms.
//!
//! Resolution runs in two tiers, and the split matters:
//!
//! 1. **Deterministic**, in the domain ([`Vault::resolve_note_ref`]): typed
//!    references, calendar words, dates, paths, and exact slugs. Shareable
//!    with any other interface.
//! 2. **Fuzzy**, here, because the matcher is the one `inquire`'s picker uses.
//!    Scoring a reference with a *different* matcher from the picker's would
//!    let `cdno open surro` auto-open one note while the picker ranked another
//!    first for the same string.
//!
//! Split into `resolve`/`render_list` seams (like `search::build_search`) so
//! tests assert the resolution and the listing without capturing stdout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use cdno_core::index::NoteCandidate;
use cdno_domain::{RefResolution, Vault};

use crate::bootstrap;

/// How many near-misses to name when a reference resolves to nothing.
///
/// The per-type resolvers answer a miss with `available_slugs_hint`, which
/// lists every slug of one type. That is the right idiom and the wrong
/// content here: `open` does not know the type, so the equivalent would be
/// every slug in the vault. Near matches carry the same "here is what you
/// could have meant" contract with far better signal.
const HINT_LIMIT: usize = 5;

pub fn run(
    root: &Path,
    today: NaiveDate,
    reference: Option<String>,
    list: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;

    if list {
        let candidates = vault
            .list_note_candidates()
            .context("listing notes in the vault")?;
        if json {
            println!("{}", serde_json::to_string_pretty(&as_json(&candidates))?);
        } else {
            print!("{}", render_list(&candidates));
        }
        return Ok(());
    }

    // Without a picker to fall back on (that arrives with the editor), a
    // missing reference has nothing to offer but the listing.
    let Some(reference) = reference else {
        bail!(
            "missing note reference — pass one (a slug, `project:<slug>`, \
             `today`, a date, or a path), or use `--list` to see every note"
        );
    };

    let path = resolve(&vault, &reference, today)?;
    // Absolute, so the output composes: `$(cdno open --path today)` has to
    // work from any directory, not just the vault root.
    let absolute = std::path::absolute(root.join(path.as_path()))
        .with_context(|| format!("resolving an absolute path for {path}"))?;

    if json {
        let payload = serde_json::json!({ "path": absolute.to_string_lossy() });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", absolute.display());
    }
    Ok(())
}

/// Resolve a reference to a vault path, or fail with a message that names
/// what the caller could have meant.
///
/// The shared seam behind every output mode, so they cannot drift on which
/// note a reference picks.
pub fn resolve(
    vault: &Vault,
    reference: &str,
    today: NaiveDate,
) -> Result<cdno_core::path::VaultPath> {
    match vault
        .resolve_note_ref(reference, today)
        .context("resolving the note reference")?
    {
        RefResolution::Resolved(path) => Ok(path),
        RefResolution::Ambiguous(hits) => bail!(ambiguous_message(reference, &hits)),
        RefResolution::NotFound { .. } => {
            let candidates = vault
                .list_note_candidates()
                .context("listing notes in the vault")?;
            bail!(not_found_message(reference, &candidates))
        }
    }
}

/// Rank candidates against `query` with the picker's own matcher, best first.
///
/// Returns only genuine matches — a zero score means the subsequence is not
/// present at all, and including those would turn "did you mean" into "here
/// are some notes".
pub fn fuzzy_rank<'a>(query: &str, candidates: &'a [NoteCandidate]) -> Vec<&'a NoteCandidate> {
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut scored: Vec<(i64, &NoteCandidate)> = candidates
        .iter()
        .filter_map(|c| {
            // Score against the same string the picker will show, so the
            // ranking the user sees is the ranking that chose for them.
            matcher.fuzzy_match(&label(c), query).map(|s| (s, c))
        })
        .collect();
    // Descending score; the candidate list is already recency-ordered, so a
    // stable sort keeps recency as the tiebreak for equal scores.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// The human label for a candidate: its title, falling back to the slug.
///
/// The fallback lives here rather than in the index's SQL so the index stays
/// honest about what it actually knows — a note with no H1 has no title, and
/// saying so is different from inventing one.
pub fn label(candidate: &NoteCandidate) -> String {
    candidate
        .title
        .clone()
        .unwrap_or_else(|| slug_of(&candidate.path))
}

/// The addressable slug of a note: its filename stem, except that an
/// `_index.md` takes its parent directory's name — the rule that makes a
/// portfolio or an expanded stewardship reachable under the name people use
/// rather than the literal string `_index`. Mirrors the domain's own
/// `candidate_slug`, which is private to the vault module.
pub fn slug_of(path: &cdno_core::path::VaultPath) -> String {
    let p = path.as_path();
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    if stem == "_index" {
        p.parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .unwrap_or(stem)
            .to_owned()
    } else {
        stem.to_owned()
    }
}

/// The typed form of a candidate, e.g. `project:surrogate-model`. This is the
/// string that mechanically resolves an ambiguity, so it is what an ambiguous
/// error must offer — a list of paths would make the caller derive it.
fn typed_ref(candidate: &NoteCandidate) -> String {
    format!("{}:{}", candidate.note_type, slug_of(&candidate.path))
}

fn ambiguous_message(reference: &str, hits: &[NoteCandidate]) -> String {
    let options: Vec<String> = hits.iter().map(typed_ref).collect();
    format!(
        "ambiguous reference `{reference}` — try {}",
        options.join(" or ")
    )
}

fn not_found_message(reference: &str, candidates: &[NoteCandidate]) -> String {
    let near = fuzzy_rank(reference, candidates);
    if near.is_empty() {
        return format!("no note matching `{reference}` — `cdno open --list` shows every note");
    }
    let names: Vec<String> = near.iter().take(HINT_LIMIT).map(|c| typed_ref(c)).collect();
    format!(
        "no note matching `{reference}` — did you mean {}?",
        names.join(", ")
    )
}

/// `path<TAB>title<TAB>type`, one note per line.
///
/// Tab-separated rather than aligned: the consumer is a fuzzy finder, and
/// `fzf --with-nth=2..` needs a delimiter it can split on, not columns.
pub fn render_list(candidates: &[NoteCandidate]) -> String {
    let mut out = String::new();
    for c in candidates {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            c.path,
            crate::output::sanitise(&label(c)),
            c.note_type
        ));
    }
    out
}

fn as_json(candidates: &[NoteCandidate]) -> Vec<serde_json::Value> {
    candidates
        .iter()
        .map(|c| {
            serde_json::json!({
                "path": c.path.to_string(),
                "title": c.title,
                "type": c.note_type,
            })
        })
        .collect()
}

/// Strip a vault-root prefix from an absolute path, leaving everything else
/// untouched.
///
/// Without this, every fzf round-trip breaks: `--list` and `--path` emit
/// absolute paths, so feeding one back to `cdno open` must resolve rather
/// than being read as a slug. The domain cannot do this — it has no idea
/// where the vault sits on disk.
pub fn strip_vault_root(reference: &str, root: &Path) -> String {
    let p = PathBuf::from(reference);
    if !p.is_absolute() {
        return reference.to_owned();
    }
    // Compare through `absolute` so a root carrying `.` or `..` still
    // matches. Symlinks are deliberately not resolved: `canonicalize` would
    // rewrite the common `~/vault -> /Volumes/…` layout into a path the user
    // never typed, and fails outright on a file that does not exist.
    let root_abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    match p.strip_prefix(&root_abs) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => reference.to_owned(),
    }
}
