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
use cdno_domain::{Miss, RefResolution, Vault};

use crate::bootstrap;

/// How many near-misses to name when a reference resolves to nothing.
///
/// The per-type resolvers answer a miss with `available_slugs_hint`, which
/// lists every slug of one type. That is the right idiom and the wrong
/// content here: `open` does not know the type, so the equivalent would be
/// every slug in the vault. Near matches carry the same "here is what you
/// could have meant" contract with far better signal.
const HINT_LIMIT: usize = 5;

#[allow(clippy::too_many_arguments)]
pub fn run(
    root: &Path,
    today: NaiveDate,
    reference: Option<String>,
    editor_flag: Option<String>,
    print_path: bool,
    list: bool,
    no_interactive: bool,
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

    // `--json` implies non-interactive, so this one expression covers a pipe,
    // a script, `--no-interactive`, and `--json` alike.
    let interactive = crate::prompt::reports_interactively(no_interactive, json);

    let path = match reference {
        Some(reference) => resolve(&vault, &reference, today, interactive)?,
        None => {
            if !interactive {
                // Rule 5's other half: naming a `--reference` that does not
                // exist would send the reader to `--help` for nothing.
                return Err(crate::prompt::missing_positional("reference")
                    .context("no note to open — pass a reference, or `--list` to see every note"));
            }
            pick_from_all(&vault, None)?
        }
    };

    // Absolute, so the output composes: `$(cdno open --path today)` has to
    // work from any directory, not just the vault root.
    let absolute = std::path::absolute(root.join(path.as_path()))
        .with_context(|| format!("resolving an absolute path for {path}"))?;

    // Printing is a terminal state: never also spawn.
    if print_path || json || !interactive {
        if json {
            let payload = serde_json::json!({ "path": absolute.to_string_lossy() });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("{}", absolute.display());
        }
        return Ok(());
    }

    open_in_editor(vault, &path, &absolute, editor_flag.as_deref())
}

/// Hand a resolved note to the editor.
///
/// Takes the `Vault` by value so it can be dropped before the editor starts:
/// a session can last hours, and there is no reason to hold the SQLite
/// connection open for it. Reconciliation on the next command picks up
/// whatever was saved, which is also why no reindex happens here — for a
/// detached GUI editor, cdno exits before the file is even written.
///
/// Shared with `cdno search`'s drill-down so the two cannot diverge on
/// editor resolution or the frozen-note warning.
pub fn open_in_editor(
    vault: Vault,
    path: &cdno_core::path::VaultPath,
    absolute: &Path,
    editor_flag: Option<&str>,
) -> Result<()> {
    warn_if_frozen(&vault, path);

    // Note what is *not* passed: anything from the vault. The editor is
    // resolved only from the flag and the user's own environment, because a
    // vault can be cloned and a setting that names a program to run must not
    // travel with data. See `crate::editor`'s module docs.
    let editor = crate::editor::resolve(editor_flag, &|key| std::env::var(key).ok(), None)?;
    drop(vault);

    match editor.spawn(absolute)? {
        Some(0) | None => Ok(()),
        Some(code) => {
            // A non-zero editor exit is a signal, not a cdno failure: it is
            // how `git commit` learns that an edit was abandoned, and a script
            // wrapping `cdno open` deserves the same. Anyhow would collapse
            // every error to 1, so the code is propagated directly.
            eprintln!("Editor exited with status {code}.");
            std::process::exit(code);
        }
    }
}

/// Warn before opening a note whose content is frozen.
///
/// An archived action carries a hash of its content at archival, and `cdno
/// lint` reports an edited *prefix* as an error (appending past it is fine).
/// Best-effort: a failed index read is not a reason to refuse to open a file.
fn warn_if_frozen(vault: &Vault, path: &cdno_core::path::VaultPath) {
    if vault.is_frozen(path).unwrap_or(false) {
        eprintln!(
            "warning: {path} is an archived note — its existing text is frozen and \
             `cdno lint` will flag an edit to it. Appending is fine."
        );
    }
}

/// Offer every note in a picker, most-recently-edited first.
fn pick_from_all(vault: &Vault, seed: Option<&str>) -> Result<cdno_core::path::VaultPath> {
    // Unlike `drill_down`, which can silently decline on a narrow terminal
    // because its listing is already on screen, the picker *is* this command.
    // Declining would leave nothing at all, so say what to do instead.
    if !crate::prompt::picker_fits(crate::output::terminal_columns()) {
        bail!("terminal too narrow to draw the picker — pass a reference, or use `--list`");
    }
    let candidates = vault
        .list_note_candidates()
        .context("listing notes in the vault")?;
    if candidates.is_empty() {
        bail!("this vault has no notes yet");
    }
    match crate::prompt::prompt_note(&candidates, picker_label, seed)? {
        Some(index) => Ok(candidates[index].path.clone()),
        // Esc is the ordinary way out and must exit 0, matching every other
        // read verb's picker.
        None => std::process::exit(0),
    }
}

/// A picker row: the title, then the slug, then the type.
///
/// The slug is not decoration. inquire filters on the rendered label, and the
/// fallback picker is seeded with what the user typed — which is a *slug*,
/// and a slug is not a subsequence of its own title once a hyphen stands
/// where a space does. Without the slug here, `cdno open surrogate-mod` would
/// open a picker filtered to nothing, making the interactive path strictly
/// worse than the piped one it exists to improve on.
pub fn picker_label(candidate: &NoteCandidate) -> String {
    let title = crate::output::sanitise(&label(candidate));
    let slug = slug_of(&candidate.path);
    // Don't repeat the slug when it is already the label (a note with no H1).
    if title == slug {
        format!("{slug}  ({})", candidate.note_type)
    } else {
        format!("{title}  ·  {slug}  ({})", candidate.note_type)
    }
}

/// The part of a reference worth filtering a picker with.
///
/// Strips a `type:` prefix, mirroring what `not_found_message` does before
/// ranking: the `:` appears in no label, so seeding with it filters the
/// picker to nothing.
fn filter_seed(reference: &str) -> &str {
    reference
        .split_once(':')
        .map_or(reference, |(_, rest)| rest)
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
    interactive: bool,
) -> Result<cdno_core::path::VaultPath> {
    // A path outside the vault, or one containing `..`, is rejected by
    // `VaultPath` as a layer error. That is correct but unreadable — "path
    // must be relative" does not tell someone who typed a real path what to
    // do — so it is turned into the same not-found the rest of this arm
    // produces.
    if reference_escapes_the_vault(reference) {
        bail!(
            "`{reference}` is outside this vault — `cdno open` resolves notes \
             within the vault; use `--vault` to point at a different one"
        );
    }
    match vault
        .resolve_note_ref(reference, today)
        .context("resolving the note reference")?
    {
        RefResolution::Resolved(path) => Ok(path),
        RefResolution::Ambiguous(hits) => {
            // In a terminal, an ambiguity is a question rather than a dead
            // end: the candidates are already known, so offer exactly those.
            if interactive && crate::prompt::picker_fits(crate::output::terminal_columns()) {
                return match crate::prompt::prompt_note(&hits, picker_label, None)? {
                    Some(index) => Ok(hits[index].path.clone()),
                    None => std::process::exit(0),
                };
            }
            bail!(ambiguous_message(reference, &hits))
        }
        // A reference that named a file gets neither the picker nor a
        // "did you mean": both would answer a precise question with unrelated
        // notes. `cdno open today` before anything is logged is the commonest
        // reference there is, and it is not a mistake.
        RefResolution::NotFound {
            miss: Miss::JournalNote,
            ..
        } => bail!(
            "no journal note at `{reference}` yet — one is created the first time you \
             write to it (`cdno log \"…\"`)"
        ),
        RefResolution::NotFound {
            miss: Miss::Path, ..
        } => bail!("no note at `{reference}` — `cdno open --list` shows every note"),
        RefResolution::NotFound {
            miss: Miss::Slug, ..
        } => {
            if interactive {
                return pick_from_all(vault, Some(filter_seed(reference)));
            }
            let candidates = vault
                .list_note_candidates()
                .context("listing notes in the vault")?;
            bail!(not_found_message(reference, &candidates))
        }
    }
}

/// Whether a reference names somewhere outside the vault, which `VaultPath`
/// will reject with a message written for a programmer rather than a user.
fn reference_escapes_the_vault(reference: &str) -> bool {
    let p = Path::new(reference.trim());
    p.is_absolute() || p.components().any(|c| c == std::path::Component::ParentDir)
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
            // Best of title and slug. Scoring the title alone looks right —
            // it is what the picker shows — but the thing people mistype is
            // the *slug*, and a slug is not a subsequence of its own title:
            // `surrogate-modle` cannot match "Surrogate model", because the
            // hyphen is not there. Missing that meant a mistyped slug got no
            // hint at all.
            let title = matcher.fuzzy_match(&label(c), query);
            let slug = matcher.fuzzy_match(&slug_of(&c.path), query);
            title.max(slug).map(|s| (s, c))
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

/// Strings that each resolve to exactly one of `hits`.
///
/// The typed form is the natural answer, but it does not always distinguish:
/// two notes of the *same* type can share a slug — `stewardships/gym.md` and
/// `stewardships/gym/_index.md` are both `stewardship:gym`. Offering that
/// twice would hand the user a suggestion that fails exactly as their input
/// did, so when the typed forms collide, fall back to the paths, which are
/// unique by construction.
fn disambiguating_refs(hits: &[NoteCandidate]) -> Vec<String> {
    let typed: Vec<String> = hits.iter().map(typed_ref).collect();
    let mut unique: Vec<&String> = typed.iter().collect();
    unique.sort();
    unique.dedup();
    if unique.len() == typed.len() {
        typed
    } else {
        hits.iter().map(|c| c.path.to_string()).collect()
    }
}

fn ambiguous_message(reference: &str, hits: &[NoteCandidate]) -> String {
    format!(
        "ambiguous reference `{reference}` — try {}",
        disambiguating_refs(hits).join(" or ")
    )
}

fn not_found_message(reference: &str, candidates: &[NoteCandidate]) -> String {
    // Rank on the slug alone. A typed reference is exactly what the ambiguity
    // error tells people to type, so `project:surrogate-modle` losing its
    // hint — because `project:` drags the score down — would give the worst
    // message to the form we recommend.
    let query = reference
        .split_once(':')
        .map_or(reference, |(_, rest)| rest);
    let near = fuzzy_rank(query, candidates);
    if near.is_empty() {
        return format!("no note matching `{reference}` — `cdno open --list` shows every note");
    }
    // Dedupe: two notes can share a typed form (a flat and an expanded
    // stewardship, say), and naming it twice reads as a bug.
    let mut names: Vec<String> = Vec::new();
    for candidate in near {
        let name = typed_ref(candidate);
        if !names.contains(&name) {
            names.push(name);
        }
        if names.len() == HINT_LIMIT {
            break;
        }
    }
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
/// Without this, the fzf round-trip breaks: `--path` emits an absolute path,
/// so feeding one back to `cdno open` must resolve rather than being read as
/// a slug. (`--list` emits vault-relative paths, which need no stripping —
/// but a user pasting from either must get the same behaviour.) The domain
/// cannot do this: it has no idea where the vault sits on disk.
pub fn strip_vault_root(reference: &str, root: &Path) -> String {
    let p = PathBuf::from(reference);
    if !p.is_absolute() {
        return reference.to_owned();
    }
    // An absolute path outside the vault stays absolute and is left to fail
    // as a plain not-found. Reinterpreting it relative to the vault would
    // silently open a different file than the one that was named.

    // Compare through `absolute` so a root carrying `.` or `..` still matches.
    let root_abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(rel) = p.strip_prefix(&root_abs) {
        return rel.to_string_lossy().into_owned();
    }
    // Then again with symlinks resolved on *both* sides. `current_dir`
    // resolves symlinks but `--vault` and `CUADERNO_VAULT_PATH` do not, so
    // the same vault reached two ways spells its root differently — and the
    // documented `$(cdno open --path …)` round-trip would fail with "outside
    // this vault" for a vault reached through a symlink. Canonicalisation is
    // used only for this comparison; what gets emitted stays as the user
    // spelled it, and a failure here simply means no match.
    match (p.canonicalize(), root.canonicalize()) {
        (Ok(real_p), Ok(real_root)) => match real_p.strip_prefix(&real_root) {
            Ok(rel) => rel.to_string_lossy().into_owned(),
            Err(_) => reference.to_owned(),
        },
        _ => reference.to_owned(),
    }
}
