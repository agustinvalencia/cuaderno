//! Tests for the `cdno open` reference grammar: `NoteRef::parse` (pure, shape
//! only) and `Vault::resolve_note_ref` (consults store and index).
//!
//! Notes are seeded as raw files and indexed through `Vault::new`'s
//! reconciliation, so the FTS titles these rely on are populated the same way
//! a real vault populates them.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{NoteRef, PeriodRef, RefResolution, RelativeDay, Vault};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn note(note_type: &str, title: &str) -> String {
    format!("---\ntype: {note_type}\n---\n# {title}\n\nBody.\n")
}

fn vault_with(notes: &[(&str, String)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, content) in notes {
        store.write_file(&vp(path), content).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// The registry names a real vault carries. Passed explicitly so the parser
/// tests do not need a vault at all.
const TYPES: &[&str] = &[
    "daily",
    "weekly",
    "monthly",
    "project",
    "action",
    "question",
    "portfolio",
    "evidence",
    "stewardship",
    "tracking",
    "commitment",
    "inbox",
];

fn resolved(vault: &Vault, reference: &str, today: NaiveDate) -> String {
    match vault.resolve_note_ref(reference, today).expect("resolve") {
        RefResolution::Resolved(path) => path.as_path().to_string_lossy().into_owned(),
        other => panic!("expected Resolved for `{reference}`, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// NoteRef::parse — shape classification, no vault
// ---------------------------------------------------------------------

#[test]
fn parses_a_typed_reference_only_when_the_prefix_names_a_real_type() {
    assert_eq!(
        NoteRef::parse("project:surrogate-model", TYPES),
        NoteRef::Typed {
            note_type: "project",
            slug: "surrogate-model"
        }
    );
    // `notes:foo` is not a type, so the whole thing is just an odd slug
    // rather than a typed reference to a type that does not exist.
    assert_eq!(
        NoteRef::parse("notes:foo", TYPES),
        NoteRef::Bare("notes:foo")
    );
    // A dangling colon has no slug to resolve.
    assert_eq!(NoteRef::parse("project:", TYPES), NoteRef::Bare("project:"));
}

#[test]
fn reserved_day_words_beat_slugs() {
    assert_eq!(
        NoteRef::parse("today", TYPES),
        NoteRef::Relative(RelativeDay::Today)
    );
    assert_eq!(
        NoteRef::parse("yesterday", TYPES),
        NoteRef::Relative(RelativeDay::Yesterday)
    );
    assert_eq!(
        NoteRef::parse("tomorrow", TYPES),
        NoteRef::Relative(RelativeDay::Tomorrow)
    );
    // The escape hatch: a project actually named `today` stays reachable.
    assert_eq!(
        NoteRef::parse("project:today", TYPES),
        NoteRef::Typed {
            note_type: "project",
            slug: "today"
        }
    );
}

#[test]
fn parses_the_three_calendar_shapes() {
    assert_eq!(
        NoteRef::parse("2026-08-21", TYPES),
        NoteRef::Date(day(2026, 8, 21))
    );
    assert_eq!(
        NoteRef::parse("2026-W34", TYPES),
        NoteRef::Period(PeriodRef::Week(day(2026, 8, 17)))
    );
    assert_eq!(
        NoteRef::parse("2026-08", TYPES),
        NoteRef::Period(PeriodRef::Month(day(2026, 8, 1)))
    );
}

#[test]
fn rejects_calendar_shapes_that_are_not_canonical() {
    // Unpadded week numbers would give one note two spellings.
    assert_eq!(NoteRef::parse("2026-W7", TYPES), NoteRef::Bare("2026-W7"));
    // Not a real month.
    assert_eq!(NoteRef::parse("2026-13", TYPES), NoteRef::Bare("2026-13"));
    // Not a real day.
    assert_eq!(
        NoteRef::parse("2026-02-30", TYPES),
        NoteRef::Bare("2026-02-30")
    );
}

#[test]
fn parses_path_shaped_references() {
    assert_eq!(
        NoteRef::parse("journal/2026/daily/2026-08-21.md", TYPES),
        NoteRef::Path("journal/2026/daily/2026-08-21.md")
    );
    // A bare filename still counts, on the `.md` suffix alone.
    assert_eq!(
        NoteRef::parse("CLAUDE.md", TYPES),
        NoteRef::Path("CLAUDE.md")
    );
}

#[test]
fn parse_trims_surrounding_whitespace() {
    assert_eq!(
        NoteRef::parse("  today  ", TYPES),
        NoteRef::Relative(RelativeDay::Today)
    );
    assert_eq!(NoteRef::parse(" foo ", TYPES), NoteRef::Bare("foo"));
}

// ---------------------------------------------------------------------
// Vault::resolve_note_ref — calendar and path tiers
// ---------------------------------------------------------------------

#[test]
fn resolves_today_and_its_neighbours_against_the_callers_clock() {
    let vault = vault_with(&[
        (
            "journal/2026/daily/2026-08-20.md",
            note("daily", "2026-08-20"),
        ),
        (
            "journal/2026/daily/2026-08-21.md",
            note("daily", "2026-08-21"),
        ),
        (
            "journal/2026/daily/2026-08-22.md",
            note("daily", "2026-08-22"),
        ),
    ]);
    let today = day(2026, 8, 21);

    assert_eq!(
        resolved(&vault, "today", today),
        "journal/2026/daily/2026-08-21.md"
    );
    assert_eq!(
        resolved(&vault, "yesterday", today),
        "journal/2026/daily/2026-08-20.md"
    );
    assert_eq!(
        resolved(&vault, "tomorrow", today),
        "journal/2026/daily/2026-08-22.md"
    );
    assert_eq!(
        resolved(&vault, "2026-08-20", today),
        "journal/2026/daily/2026-08-20.md"
    );
}

#[test]
fn resolves_weekly_and_monthly_periods() {
    let vault = vault_with(&[
        (
            "journal/2026/weekly/2026-W34.md",
            note("weekly", "2026-W34"),
        ),
        (
            "journal/2026/monthly/2026-08.md",
            note("monthly", "2026-08"),
        ),
    ]);
    let today = day(2026, 8, 21);

    assert_eq!(
        resolved(&vault, "2026-W34", today),
        "journal/2026/weekly/2026-W34.md"
    );
    assert_eq!(
        resolved(&vault, "2026-08", today),
        "journal/2026/monthly/2026-08.md"
    );
}

#[test]
fn a_missing_daily_note_is_not_found_rather_than_an_error() {
    // The most common thing anyone will type, on a day not yet logged:
    // daily notes are scaffolded lazily by `cdno log`.
    let vault = vault_with(&[]);
    let result = vault
        .resolve_note_ref("today", day(2026, 8, 21))
        .expect("resolve");
    assert!(matches!(result, RefResolution::NotFound { .. }));
}

/// The rule that keeps a typo'd path honest: a path reference names a file
/// outright, so a miss must never fall through to fuzzy matching and open
/// something merely adjacent.
#[test]
fn a_path_reference_never_falls_through_to_slug_matching() {
    let vault = vault_with(&[(
        "projects/surrogate-model.md",
        note("project", "Surrogate model"),
    )]);
    let today = day(2026, 8, 21);

    // The slug exists, but spelled as a path it is simply absent.
    let result = vault
        .resolve_note_ref("projects/surrogate-modle.md", today)
        .expect("resolve");
    assert!(
        matches!(result, RefResolution::NotFound { .. }),
        "got {result:?}"
    );
}

#[test]
fn resolves_a_vault_relative_path() {
    let vault = vault_with(&[(
        "projects/surrogate-model.md",
        note("project", "Surrogate model"),
    )]);
    assert_eq!(
        resolved(&vault, "projects/surrogate-model.md", day(2026, 8, 21)),
        "projects/surrogate-model.md"
    );
}

// ---------------------------------------------------------------------
// Slug tiers
// ---------------------------------------------------------------------

#[test]
fn resolves_a_bare_slug_across_types() {
    let vault = vault_with(&[
        (
            "projects/surrogate-model.md",
            note("project", "Surrogate model"),
        ),
        ("actions/write-it-up.md", note("action", "Write it up")),
    ]);
    let today = day(2026, 8, 21);

    assert_eq!(
        resolved(&vault, "surrogate-model", today),
        "projects/surrogate-model.md"
    );
    // An action — a type with no per-type resolver of its own, which is
    // precisely why this scan supersedes them rather than delegating.
    assert_eq!(
        resolved(&vault, "write-it-up", today),
        "actions/write-it-up.md"
    );
}

/// `_index.md` is addressed by its parent directory's name, which is what
/// makes portfolios and expanded stewardships reachable under the name
/// people actually use.
#[test]
fn an_index_note_is_addressed_by_its_folder_name() {
    let vault = vault_with(&[
        (
            "portfolios/surrogate-model/_index.md",
            note("portfolio", "Surrogate model"),
        ),
        ("stewardships/gym/_index.md", note("stewardship", "Gym")),
    ]);
    let today = day(2026, 8, 21);

    assert_eq!(
        resolved(&vault, "surrogate-model", today),
        "portfolios/surrogate-model/_index.md"
    );
    assert_eq!(resolved(&vault, "gym", today), "stewardships/gym/_index.md");
}

#[test]
fn a_typed_reference_disambiguates_a_slug_shared_across_types() {
    let vault = vault_with(&[
        (
            "projects/surrogate-model.md",
            note("project", "Surrogate model"),
        ),
        (
            "questions/research/surrogate-model.md",
            note("question", "Surrogate model"),
        ),
    ]);
    let today = day(2026, 8, 21);

    assert_eq!(
        resolved(&vault, "project:surrogate-model", today),
        "projects/surrogate-model.md"
    );
    assert_eq!(
        resolved(&vault, "question:surrogate-model", today),
        "questions/research/surrogate-model.md"
    );
}

#[test]
fn a_slug_in_two_types_is_ambiguous_rather_than_guessed() {
    let vault = vault_with(&[
        (
            "projects/surrogate-model.md",
            note("project", "Surrogate model"),
        ),
        (
            "questions/research/surrogate-model.md",
            note("question", "Surrogate model"),
        ),
    ]);

    let result = vault
        .resolve_note_ref("surrogate-model", day(2026, 8, 21))
        .expect("resolve");

    match result {
        RefResolution::Ambiguous(hits) => {
            let mut types: Vec<&str> = hits.iter().map(|c| c.note_type.as_str()).collect();
            types.sort_unstable();
            assert_eq!(types, vec!["project", "question"]);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

/// The one type-specific preference kept from the per-type resolvers:
/// parking is a lifecycle state of the same project, so the two paths are
/// one note in two places rather than two notes.
#[test]
fn an_active_project_beats_its_parked_namesake() {
    let vault = vault_with(&[
        (
            "projects/surrogate-model.md",
            note("project", "Surrogate model"),
        ),
        (
            "projects/_parked/surrogate-model.md",
            note("project", "Surrogate model"),
        ),
    ]);

    assert_eq!(
        resolved(&vault, "surrogate-model", day(2026, 8, 21)),
        "projects/surrogate-model.md"
    );
}

#[test]
fn an_unknown_slug_is_not_found() {
    let vault = vault_with(&[(
        "projects/surrogate-model.md",
        note("project", "Surrogate model"),
    )]);

    let result = vault
        .resolve_note_ref("no-such-note", day(2026, 8, 21))
        .expect("resolve");

    match result {
        RefResolution::NotFound { reference } => assert_eq!(reference, "no-such-note"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// list_note_candidates
// ---------------------------------------------------------------------

/// The listing must carry the body H1, not the (absent) frontmatter title —
/// the failure that would leave every picker row unlabelled.
#[test]
fn list_note_candidates_carries_the_body_h1_as_the_title() {
    let vault = vault_with(&[(
        "projects/surrogate-model.md",
        note("project", "Surrogate model"),
    )]);

    let candidates = vault.list_note_candidates().expect("candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].title.as_deref(), Some("Surrogate model"));
    assert_eq!(candidates[0].note_type, "project");
}
