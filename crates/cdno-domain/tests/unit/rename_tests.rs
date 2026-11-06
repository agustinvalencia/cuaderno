//! The rename (RFC 0007 §5.2–§5.6, §6.2). `Vault::plan_rename` (T18), the
//! pure planner over a store snapshot, planned against a snapshot read from
//! a `MemoryVaultStore`; then `rename_preview` and `rename_note` (T20), the
//! write under the lock, on the same fixture.

use std::collections::BTreeMap;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::extractors::{differs_only_by, wikilink_spans};
use cdno_core::frontmatter::split_frontmatter;
use cdno_core::frontmatter_edit::frontmatter_scalar_spans;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::TakenBy;
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::vault::{
    Change, CommitmentSource, RefResolution, RenameOutcome, RenamePlan, RenameWarning,
    VaultSnapshot, format_renamed_log_entry,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use super::support::{FailingStore, FlakyIndex};

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n- [ ] [[actions/fit-the-baseline]] (deep)\n\n## Milestones\n- [ ] Baseline fitted \u{2014} hard: 2026-05-20\n";

const ACTION: &str = "---\ntype: action\nstatus: active\nproject: old\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Fit the baseline\n\n## Notes\nTagged #action/old in the log.\n";

const COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-06-01\ncreated: 2026-05-01\ncontext: work\nproject: \"old\"\nstewardship: null\n---\n\n# Send the error budget\n";

const INBOX: &str =
    "---\ntype: inbox\ncreated: 2026-05-08\ncaptured_during: old\n---\n\nTry a smaller kernel.\n";

const EMULATOR: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-02\n---\n\n# Emulator\n\n## Current State\nWaiting on [[old]].\n\n## Next Actions\n\n## Milestones\n- [ ] Hand-off from [[old]] \u{2014} target: 2026-06-15\n";

/// A note whose frontmatter holds `project: old` nested under another key:
/// not a §3.5 field, since only a top-level key is one.
const NESTED: &str = "---\ntype: action\nstatus: active\nproject: emulator\nenergy: deep\ncreated: 2026-05-01\nupstream:\n  project: old\n---\n\n# Wire the emulator\n";

const PORTFOLIO: &str = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\nproject: \"[[projects/old]]\"\n---\n\n# How large is the error?\n";

const DAY_8: &str = "---\ntype: daily\ndate: 2026-05-08\n---\n\n# Friday, 8 May 2026\n\n## Logs\n- **09:00**: started [[old]] \u{2014} fit the baseline\n- **09:30**: state on [[old]]\n  was: Going.\n  now: Calibrating.\n- **11:00**: wrote `[[old]]` in a code span, and #action/old\n  during: [[old]]\n";

const DAY_9: &str = "---\ntype: daily\ndate: 2026-05-09\n---\n\n# Saturday, 9 May 2026\n\n## Logs\n- **09:00**: read [[old|the surrogate]] again\n- **10:00**: see [[old#Current State]] and [[projects/old]]\n";

const DAY_10: &str = "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **08:00**: back on [[old]]\n";

const ARCHIVED: &str = "actions/_done/2026/characterise-the-residuals.md";
const DAILY_TODAY: &str = "journal/2026/daily/2026-05-10.md";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 5, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

/// Today, for every plan below.
fn at() -> NaiveDateTime {
    dt(10, 10, 30)
}

struct Fixture {
    store: Arc<dyn VaultStore>,
    /// The store and index under `store` and the vault, for a test that
    /// opens the vault again over a wrapper of either.
    mem: Arc<MemoryVaultStore>,
    index: Arc<MemoryIndex>,
    vault: Vault,
}

impl Fixture {
    fn snapshot(&self) -> VaultSnapshot {
        self.vault.rename_snapshot().expect("snapshot")
    }

    fn plan(&self, target: &str, new_slug: &str) -> Result<RenamePlan, DomainError> {
        self.vault
            .plan_rename(&self.snapshot(), target, new_slug, None, at())
    }

    fn write(&self, path: &str, content: &str) {
        self.store.write_file(&vp(path), content).unwrap();
    }

    fn read(&self, path: &str) -> String {
        self.store.read_file(&vp(path)).unwrap()
    }

    /// Write a plan by hand, as the write verb (T20) will: each `after` at
    /// its path, the target's at its new path.
    fn apply(&self, plan: &RenamePlan) {
        for edit in &plan.edits {
            match plan.moves.iter().find(|(from, _)| *from == edit.path) {
                Some((from, to)) => {
                    self.store.write_file(to, &edit.after).unwrap();
                    self.store.delete_file(from).unwrap();
                }
                None => self.store.write_file(&edit.path, &edit.after).unwrap(),
            }
        }
    }
}

/// The fixture the plan's T18 probes name: project `old` ("Surrogate
/// model"), an attached action, a quoted commitment, an inbox item captured
/// during it, a mutable project map linking it, three daily notes (the last
/// one today's), a portfolio index linking it from its frontmatter, a
/// nested `project: old`, a code span, a tag, and an archived action with
/// `[[old]]` in its frozen prefix, archived through the verb so its snapshot
/// row is exactly what archival stamps.
fn fixture() -> Fixture {
    let mem = Arc::new(MemoryVaultStore::new());
    let mem_index = Arc::new(MemoryIndex::new());
    let store: Arc<dyn VaultStore> = mem.clone();
    let index: Arc<dyn VaultIndex> = mem_index.clone();
    store.write_file(&vp("projects/old.md"), PROJECT).unwrap();
    let vault = open(Arc::clone(&store), Arc::clone(&index));
    let action = vault
        .add_action_with_note(
            dt(7, 9, 0),
            "old",
            "Characterise the residuals",
            EnergyLevel::Deep,
        )
        .expect("add action");
    let note = store.read_file(&action).unwrap();
    store
        .write_file(&action, &format!("{note}\nCompared against [[old]].\n"))
        .unwrap();
    vault
        .complete_action(dt(7, 10, 0), "old", "characterise-the-residuals")
        .expect("complete action");
    assert!(
        index
            .find_archival_snapshot(&vp(ARCHIVED))
            .unwrap()
            .is_some(),
        "archival stamps a snapshot"
    );

    // What the verbs wrote besides the archived note is replaced by the
    // fixture's own files.
    for path in store.walk_dir(&VaultPath::root()).unwrap() {
        if path != vp(ARCHIVED) {
            store.delete_file(&path).unwrap();
        }
    }
    for (path, content) in [
        ("projects/old.md", PROJECT),
        ("actions/fit-the-baseline.md", ACTION),
        ("commitments/send-the-error-budget.md", COMMITMENT),
        ("inbox/2026-05-08-smaller-kernel.md", INBOX),
        ("projects/emulator.md", EMULATOR),
        ("actions/wire-the-emulator.md", NESTED),
        ("portfolios/surrogate-error-budget/_index.md", PORTFOLIO),
        ("journal/2026/daily/2026-05-08.md", DAY_8),
        ("journal/2026/daily/2026-05-09.md", DAY_9),
        (DAILY_TODAY, DAY_10),
    ] {
        store.write_file(&vp(path), content).unwrap();
    }
    // Opened again, so reconciliation indexes the files written by hand
    // and the index agrees with the store before any rename.
    drop(vault);
    let vault = open(Arc::clone(&store), index);
    Fixture {
        store,
        mem,
        index: mem_index,
        vault,
    }
}

fn open(store: Arc<dyn VaultStore>, index: Arc<dyn VaultIndex>) -> Vault {
    Vault::new(store, index, VaultConfig::default())
        .expect("vault opens")
        .0
}

fn changes_by_path(plan: &RenamePlan) -> BTreeMap<String, Vec<Change>> {
    plan.edits
        .iter()
        .map(|e| (e.path.to_string(), e.changes.clone()))
        .collect()
}

fn field(key: &str) -> Change {
    Change::Field {
        key: key.to_owned(),
    }
}

#[test]
fn the_plan_lists_every_reference_kind_once() {
    let f = fixture();
    let plan = f.plan("old", "new").expect("plan");

    let expected: BTreeMap<String, Vec<Change>> = [
        (
            ARCHIVED,
            vec![Change::Address { count: 1 }, field("project")],
        ),
        ("actions/fit-the-baseline.md", vec![field("project")]),
        (
            "commitments/send-the-error-budget.md",
            vec![field("project")],
        ),
        (
            "inbox/2026-05-08-smaller-kernel.md",
            vec![field("captured_during")],
        ),
        (
            "journal/2026/daily/2026-05-08.md",
            vec![Change::Address { count: 3 }],
        ),
        (
            "journal/2026/daily/2026-05-09.md",
            vec![Change::Address { count: 3 }],
        ),
        (
            DAILY_TODAY,
            vec![Change::Address { count: 1 }, Change::LogLine],
        ),
        (
            "portfolios/surrogate-error-budget/_index.md",
            vec![Change::Address { count: 1 }],
        ),
        ("projects/emulator.md", vec![Change::Address { count: 2 }]),
        ("projects/old.md", vec![Change::Alias]),
    ]
    .into_iter()
    .map(|(p, c)| (p.to_owned(), c))
    .collect();
    assert_eq!(changes_by_path(&plan), expected);
    assert_eq!(
        plan.moves,
        vec![(vp("projects/old.md"), vp("projects/new.md"))]
    );
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);

    // The code span and the tag are not references: they stay as written.
    let day_8 = plan
        .edits
        .iter()
        .find(|e| e.path == vp("journal/2026/daily/2026-05-08.md"))
        .unwrap();
    assert!(day_8.after.contains("`[[old]]`"));
    assert!(day_8.after.contains("#action/old"));
    assert!(day_8.after.contains("state on [[new]]"));
    // The commitment's value stays quoted; the portfolio's frontmatter
    // wikilink is rewritten once, as an address.
    let commitment = plan
        .edits
        .iter()
        .find(|e| e.path == vp("commitments/send-the-error-budget.md"))
        .unwrap();
    assert!(commitment.after.contains("project: \"new\"\n"));
    let portfolio = plan
        .edits
        .iter()
        .find(|e| e.path.to_string().starts_with("portfolios/"))
        .unwrap();
    assert!(portfolio.after.contains("project: \"[[projects/new]]\"\n"));
}

#[test]
fn every_edit_differs_only_by_its_reference_edits() {
    let f = fixture();
    let plan = f.plan("old", "new").expect("plan");
    assert!(!plan.edits.is_empty());
    for edit in &plan.edits {
        assert!(
            differs_only_by(&edit.before, &edit.after, &edit.edits),
            "{}: after is not before with its edits",
            edit.path
        );
        let spans = wikilink_spans(&edit.before);
        let fields: Vec<std::ops::Range<usize>> = split_frontmatter(&edit.before)
            .map(|(block, _)| {
                let base = block.as_ptr() as usize - edit.before.as_ptr() as usize;
                ["project", "captured_during", "stewardship", "portfolio"]
                    .iter()
                    .flat_map(|key| frontmatter_scalar_spans(block, key))
                    .map(|r| r.start + base..r.end + base)
                    .collect()
            })
            .unwrap_or_default();
        let block_end = split_frontmatter(&edit.before).map(|(block, _)| {
            block.as_ptr() as usize - edit.before.as_ptr() as usize + block.len()
        });
        let last = edit.edits.last().map(|e| e.range.clone());
        for e in &edit.edits {
            let address = spans.iter().any(|s| s.address == e.range);
            let field = fields.contains(&e.range);
            // An insertion is the alias line, at the end of the frontmatter
            // block, or the log line, the last edit of today's note.
            let alias = edit.changes.contains(&Change::Alias)
                && e.range.is_empty()
                && Some(e.range.start) == block_end
                && e.replacement == "aliases: [old]\n";
            let log_line = edit.changes.contains(&Change::LogLine)
                && e.range.is_empty()
                && Some(e.range.clone()) == last
                && e.replacement.starts_with(
                    "- **10:30**: renamed [[new]] \u{2014} Surrogate model\n  was: old\n",
                );
            let insertion = alias || log_line;
            assert!(
                address || field || insertion,
                "{}: edit {:?} -> {:?} is not a reference",
                edit.path,
                e.range,
                e.replacement
            );
            if address || field {
                assert!(
                    e.replacement == "new" || e.replacement.ends_with("/new"),
                    "{}: {:?}",
                    edit.path,
                    e.replacement
                );
            }
        }
    }
}

#[test]
fn a_nested_reference_key_is_not_rewritten() {
    let f = fixture();
    let plan = f.plan("old", "new").expect("plan");
    assert!(
        plan.edits
            .iter()
            .all(|e| e.path != vp("actions/wire-the-emulator.md")),
        "a nested `project: old` is not a reference"
    );
}

#[test]
fn the_daily_note_has_one_after_with_the_log_line_last() {
    let f = fixture();
    let plan = f
        .vault
        .plan_rename(&f.snapshot(), "old", "new", Some("a clearer\nname"), at())
        .expect("plan");
    let today: Vec<_> = plan
        .edits
        .iter()
        .filter(|e| e.path == vp(DAILY_TODAY))
        .collect();
    assert_eq!(today.len(), 1, "one edit for today's daily note");
    let after = &today[0].after;
    assert!(after.contains("- **08:00**: back on [[new]]\n"), "{after}");
    assert!(
        after.ends_with(
            "- **10:30**: renamed [[new]] \u{2014} Surrogate model\n  was: old\n  reason: a clearer name\n"
        ),
        "{after}"
    );
    assert!(!after.contains("[[old"), "{after}");
}

#[test]
fn a_rename_on_a_day_without_a_daily_note_scaffolds_it() {
    let f = fixture();
    f.store.delete_file(&vp(DAILY_TODAY)).unwrap();
    let plan = f.plan("old", "new").expect("plan");
    let today = plan
        .edits
        .iter()
        .find(|e| e.path == vp(DAILY_TODAY))
        .expect("today's daily note is planned");
    assert_eq!(today.changes, vec![Change::LogLine]);
    assert_eq!(today.before, "");
    assert!(differs_only_by(&today.before, &today.after, &today.edits));
    assert!(today.after.starts_with("---\ntype: daily\n"));
    assert!(
        today
            .after
            .ends_with("- **10:30**: renamed [[new]] \u{2014} Surrogate model\n  was: old\n")
    );
}

#[test]
fn the_target_records_its_old_slug_as_an_alias() {
    let f = fixture();
    let plan = f.plan("old", "new").expect("plan");
    let own = plan
        .edits
        .iter()
        .find(|e| e.path == vp("projects/old.md"))
        .unwrap();
    assert!(own.after.contains("aliases: [old]\n---\n"), "{}", own.after);
}

#[test]
fn a_rename_to_itself_is_an_empty_plan() {
    let f = fixture();
    let plan = f.plan("old", "old").expect("plan");
    assert!(plan.is_empty());
    assert!(plan.edits.is_empty() && plan.moves.is_empty() && plan.warnings.is_empty());
}

// ---------------------------------------------------------------------
// Refusals, in the order the planner checks them.
// ---------------------------------------------------------------------

#[test]
fn a_non_renamable_type_is_refused_first() {
    let f = fixture();
    // An invalid slug too: the type refusal comes first.
    let err = f
        .plan("journal/2026/daily/2026-05-08", "Not A Slug")
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "type daily"),
        "{err:?}"
    );
}

#[test]
fn an_archived_action_is_not_renamable() {
    let f = fixture();
    let err = f.plan(ARCHIVED, "Not A Slug").unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "archived"),
        "{err:?}"
    );
}

#[test]
fn a_folder_note_is_not_renamable_yet() {
    let f = fixture();
    let err = f
        .plan("portfolios/surrogate-error-budget", "new")
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "folder"),
        "{err:?}"
    );
}

#[test]
fn an_invalid_slug_is_refused_before_taken() {
    let f = fixture();
    // `2026-05-08` is a daily note's stem, and YAML reads it as a date.
    let err = f.plan("old", "2026-05-08").unwrap_err();
    assert!(
        matches!(&err, DomainError::InvalidSlug { slug, .. } if slug == "2026-05-08"),
        "{err:?}"
    );
}

#[test]
fn a_taken_slug_is_refused_before_stem_shared() {
    let f = fixture();
    f.write("notes/old.md", "---\ntype: daily\n---\n\n# Old\n");
    let err = f.plan("projects/old", "emulator").unwrap_err();
    assert!(
        matches!(&err, DomainError::SlugTaken { slug, holder, .. }
            if slug == "emulator" && *holder == vp("projects/emulator.md")),
        "{err:?}"
    );
}

#[test]
fn a_slug_held_as_an_alias_is_taken() {
    let f = fixture();
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace(
            "created: 2026-04-02\n",
            "created: 2026-04-02\naliases: [newer]\n",
        ),
    );
    let err = f.plan("old", "newer").unwrap_err();
    assert!(
        matches!(&err, DomainError::SlugTaken { slug, .. } if slug == "newer"),
        "{err:?}"
    );
}

#[test]
fn a_shared_stem_is_refused() {
    let f = fixture();
    f.write("notes/old.md", "---\ntype: daily\n---\n\n# Old\n");
    let err = f.plan("projects/old", "new").unwrap_err();
    assert!(
        matches!(&err, DomainError::StemShared { slug, others, .. }
            if slug == "old" && *others == vec![vp("notes/old.md")]),
        "{err:?}"
    );
}

#[test]
fn a_shared_stem_is_refused_before_the_no_op() {
    let f = fixture();
    f.write("notes/old.md", "---\ntype: daily\n---\n\n# Old\n");
    let err = f.plan("projects/old", "old").unwrap_err();
    assert!(matches!(err, DomainError::StemShared { .. }), "{err:?}");
}

#[test]
fn a_field_rewrite_that_changes_more_than_the_field_is_refused() {
    let f = fixture();
    // Two top-level `project:` keys: both are rewritten, and the block no
    // longer parses, so the guard cannot show that only the field moved.
    f.write(
        "actions/fit-the-baseline.md",
        &ACTION.replace("project: old\n", "project: old\nproject: old\n"),
    );
    let err = f.plan("old", "new").unwrap_err();
    assert!(
        matches!(&err, DomainError::ReferenceDrift { path, .. }
            if *path == vp("actions/fit-the-baseline.md")),
        "{err:?}"
    );
}

#[test]
fn a_field_the_rewrite_cannot_address_is_refused() {
    let f = fixture();
    // The parser reads `project` as `old` in each form, but the text rewrite
    // finds no value on the key's line equal to it: a plain scalar on the
    // next line, a folded block scalar, a tagged scalar.
    for form in [
        "project:\n  old\n",
        "project: >-\n  old\n",
        "project: !!str old\n",
    ] {
        f.write(
            "actions/fit-the-baseline.md",
            &ACTION.replace("project: old\n", form),
        );
        let err = f.plan("old", "new").unwrap_err();
        assert!(
            matches!(&err, DomainError::ReferenceUnaddressable { path, key }
                if *path == vp("actions/fit-the-baseline.md") && key == "project"),
            "{form:?}: {err:?}"
        );
    }
}

#[test]
fn a_folder_note_named_like_the_target_is_not_a_shared_stem() {
    let f = fixture();
    f.write(
        "portfolios/old/_index.md",
        "---\ntype: portfolio\nquestion: \"Is it old?\"\ncreated: 2026-05-01\n---\n\n# Is it old?\n",
    );
    let plan = f.plan("projects/old", "new").expect("not a shared stem");
    // `[[old]]` still resolves to the project, so it is still rewritten.
    assert!(
        plan.edits
            .iter()
            .any(|e| e.path == vp("projects/emulator.md"))
    );
}

#[test]
fn a_question_rename_leaves_portfolio_rows_alone() {
    let f = fixture();
    f.write(
        "questions/research/how-large-is-the-error.md",
        "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# How large is the error?\n",
    );
    // An evidence note whose `portfolio:` equals the question's slug, with
    // no such portfolio: it names nothing that moves.
    f.write(
        "portfolios/how-large-is-the-error/2026-05-02-baseline.md",
        "---\ntype: evidence\nportfolio: how-large-is-the-error\ncreated: 2026-05-02\n---\n\n# Baseline\n",
    );
    let plan = f
        .plan("questions/research/how-large-is-the-error", "error-size")
        .expect("plan");
    assert!(
        plan.edits
            .iter()
            .all(|e| !e.path.to_string().starts_with("portfolios/how-large")),
        "{:?}",
        plan.edits
            .iter()
            .map(|e| e.path.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_old_slug_already_another_notes_alias_is_a_warning() {
    let f = fixture();
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace(
            "created: 2026-04-02\n",
            "created: 2026-04-02\naliases: [old]\n",
        ),
    );
    let plan = f.plan("projects/old", "new").expect("still planned");
    assert_eq!(
        plan.warnings,
        vec![RenameWarning::OldSlugAlreadyAnAlias {
            holder: vp("projects/emulator.md")
        }]
    );
}

#[test]
fn renamed_entry_has_was_then_reason_as_continuations() {
    let head = "renamed [[new]] \u{2014} Surrogate model";
    assert_eq!(
        format_renamed_log_entry("new", "Surrogate model", "old", Some("a clearer name")),
        format!("{head}\n  was: old\n  reason: a clearer name")
    );
    assert_eq!(
        format_renamed_log_entry("new", "Surrogate model", "old", None),
        format!("{head}\n  was: old")
    );
    // A reason that is blank once flattened writes no `reason:` line; an
    // interior newline is flattened, so the entry stays one entry.
    assert_eq!(
        format_renamed_log_entry("new", "Surrogate model", "old", Some(" \n\t ")),
        format!("{head}\n  was: old")
    );
    assert_eq!(
        format_renamed_log_entry("new", "Surrogate\nmodel", "old", Some("a\nclearer name")),
        format!("{head}\n  was: old\n  reason: a clearer name")
    );
    // `was:` is a copied value, never a link.
    let entry = format_renamed_log_entry("projects/new", "Surrogate model", "old", None);
    let was = entry
        .lines()
        .find(|l| l.trim_start().starts_with("was:"))
        .unwrap();
    assert!(!was.contains("[["), "{was}");
}

/// The one day the span property does not hold, recorded so a per-reference
/// view (T24/T25) does not rely on it: when today's note has a hand-written
/// section after `## Logs`, placing the line re-pins `## Logs` to the bottom,
/// and the merged edit is one region that absorbs the day's address edits.
/// `after` is still exactly `before` with the plan's edits.
#[test]
fn the_span_property_yields_to_a_hand_written_section_after_the_log_anchor() {
    let f = fixture();
    f.write(DAILY_TODAY, &format!("{DAY_10}\n## Scratch\nA thought.\n"));
    let plan = f.plan("old", "new").expect("plan");
    let today = plan
        .edits
        .iter()
        .find(|e| e.path == vp(DAILY_TODAY))
        .unwrap();
    assert_eq!(
        today.changes,
        vec![Change::Address { count: 1 }, Change::LogLine]
    );
    assert!(differs_only_by(&today.before, &today.after, &today.edits));
    assert_eq!(today.edits.len(), 1, "{:?}", today.edits);
    let edit = &today.edits[0];
    assert!(!edit.range.is_empty());
    assert!(
        wikilink_spans(&today.before)
            .iter()
            .all(|s| s.address != edit.range),
        "the merged edit is not an address span"
    );
    assert!(today.after.contains("back on [[new]]"));
    // The log line is the last line, and the hand-written section now sits
    // before `## Logs`.
    assert!(
        today
            .after
            .ends_with("- **10:30**: renamed [[new]] \u{2014} Surrogate model\n  was: old\n"),
        "{}",
        today.after
    );
    let scratch = today.after.find("## Scratch").expect("section kept");
    let logs = today.after.find("## Logs").expect("logs kept");
    assert!(scratch < logs, "{}", today.after);
}

// ---------------------------------------------------------------------
// Frozen prefixes and the plan hash.
// ---------------------------------------------------------------------

#[test]
fn a_diverged_frozen_prefix_is_a_warning_not_a_refusal() {
    let f = fixture();
    // A prose edit inside the archived note's frozen prefix: neither hash
    // verifies any more.
    let raw = f.read(ARCHIVED);
    let edited = raw.replace(
        "# Characterise the residuals",
        "# Characterise the residual",
    );
    assert_ne!(raw, edited);
    f.write(ARCHIVED, &edited);

    let plan = f.plan("old", "new").expect("still planned");
    assert_eq!(
        plan.warnings,
        vec![RenameWarning::FrozenPrefixAlreadyDiverged { path: vp(ARCHIVED) }]
    );
    let archived = plan.edits.iter().find(|e| e.path == vp(ARCHIVED)).unwrap();
    assert!(archived.after.contains("Compared against [[new]]."));
}

#[test]
fn a_second_rename_of_a_frozen_note_does_not_warn() {
    let f = fixture();
    let first = f.plan("old", "new").expect("first plan");
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    f.apply(&first);
    assert!(f.read(ARCHIVED).contains("[[new]]"));

    // The archived note's plain hash no longer verifies; its masked hash
    // does, so the second rename has nothing to warn about.
    let second = f.plan("new", "surrogate-model").expect("second plan");
    assert!(
        second.edits.iter().any(|e| e.path == vp(ARCHIVED)),
        "the archived note is a referrer again"
    );
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
}

#[test]
fn the_hash_ignores_the_stamp_and_the_date_but_not_a_referrer() {
    let f = fixture();
    let snapshot = f.snapshot();
    let plan = |at: NaiveDateTime| {
        f.vault
            .plan_rename(&snapshot, "old", "new", None, at)
            .unwrap()
    };
    let base = plan(at());

    // Another minute of the same day: the stamp moved, the plan did not.
    let later = plan(dt(10, 23, 59));
    assert_ne!(base.edits, later.edits, "the stamp is in the after");
    assert_eq!(base.hash, later.hash);

    // The date enters only through today's daily note's path: a rollover is
    // a different plan (T20 refuses it as stale).
    assert_ne!(base.hash, plan(dt(11, 10, 30)).hash);

    // A referrer that changed is a different plan.
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace("Waiting on [[old]].", "Waiting on [[old]] again."),
    );
    let changed = f.plan("old", "new").unwrap();
    assert_ne!(base.hash, changed.hash);
}

// ---------------------------------------------------------------------
// The write (T20): `rename_preview`, then `rename_note` under the lock.
// ---------------------------------------------------------------------

impl Fixture {
    fn preview(&self, target: &str, new_slug: &str) -> RenamePlan {
        self.vault
            .rename_preview(target, new_slug, None, at())
            .expect("preview")
    }

    /// Preview, then write that plan, as a surface does.
    fn rename(&self, target: &str, new_slug: &str) -> RenameOutcome {
        let plan = self.preview(target, new_slug);
        self.vault
            .rename_note(target, new_slug, None, Some(&plan.hash), at())
            .expect("rename")
    }

    /// Every file in the store, by path.
    fn files(&self) -> BTreeMap<String, String> {
        files_of(&*self.store)
    }

    fn resolves(&self, reference: &str) -> VaultPath {
        match self
            .vault
            .resolve_note_ref(reference, at().date())
            .expect("resolve")
        {
            RefResolution::Resolved(note) => note.path,
            other => panic!("`{reference}` did not resolve: {other:?}"),
        }
    }
}

fn files_of(store: &dyn VaultStore) -> BTreeMap<String, String> {
    store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|p| (p.to_string(), store.read_file(&p).unwrap()))
        .collect()
}

/// Where each planned edit's `after` lands: its path, or the target's new
/// path.
fn destinations(plan: &RenamePlan) -> BTreeMap<VaultPath, String> {
    plan.edits
        .iter()
        .map(|e| {
            let dest = plan
                .moves
                .iter()
                .find(|(from, _)| *from == e.path)
                .map_or(e.path.clone(), |(_, to)| to.clone());
            (dest, e.after.clone())
        })
        .collect()
}

#[test]
fn a_rename_writes_exactly_the_plan() {
    let f = fixture();
    let before = f.files();
    let preview = f.preview("old", "new");
    let outcome = f
        .vault
        .rename_note("old", "new", None, Some(&preview.hash), at())
        .expect("rename");
    assert_eq!(
        outcome.plan, preview,
        "the plan written is the one previewed"
    );
    assert!(!outcome.index_stale);

    let planned = destinations(&preview);
    let written: std::collections::BTreeSet<VaultPath> = outcome.written.iter().cloned().collect();
    assert_eq!(
        written.len(),
        outcome.written.len(),
        "each path written once"
    );
    assert_eq!(
        written,
        planned.keys().cloned().collect(),
        "the paths written are the plan's"
    );
    for (path, after) in &planned {
        assert_eq!(&f.read(&path.to_string()), after, "{path}");
    }
    // The old path is gone, and nothing outside the plan changed.
    let after = f.files();
    assert!(!after.contains_key("projects/old.md"));
    let touched: Vec<String> = preview.edits.iter().map(|e| e.path.to_string()).collect();
    for (path, content) in &before {
        if !touched.contains(path) {
            assert_eq!(after.get(path), Some(content), "{path} changed");
        }
    }
    assert_eq!(
        after.len(),
        before.len(),
        "{:?}",
        after.keys().collect::<Vec<_>>()
    );

    let note = f.read("projects/new.md");
    assert!(note.contains("aliases: [old]\n---\n"), "{note}");
    assert!(
        f.read(DAILY_TODAY)
            .ends_with("- **10:30**: renamed [[new]] \u{2014} Surrogate model\n  was: old\n")
    );
}

#[test]
fn references_resolve_after_the_rename() {
    let f = fixture();
    let outcome = f.rename("old", "new");
    let new = vp("projects/new.md");
    let old = vp("projects/old.md");

    // Every referrer by wikilink is a backlink of the new path, and none
    // is left on the old one.
    let mut backlinks = f.index.find_backlinks(&new).unwrap();
    backlinks.sort();
    let mut linking: Vec<VaultPath> = outcome
        .plan
        .edits
        .iter()
        .filter(|e| {
            e.changes
                .iter()
                .any(|c| matches!(c, Change::Address { .. }))
        })
        .map(|e| e.path.clone())
        .collect();
    linking.sort();
    assert_eq!(backlinks, linking);
    assert!(f.index.find_backlinks(&old).unwrap().is_empty());

    // The moved note's own links and its search row are at the new path.
    let outgoing = f.index.find_outgoing_links(&new).unwrap();
    assert!(
        outgoing
            .iter()
            .any(|l| l.resolved_path.as_ref() == Some(&vp("actions/fit-the-baseline.md"))),
        "{outgoing:?}"
    );
    let hits: Vec<VaultPath> = f
        .index
        .search("Calibrating", 10)
        .unwrap()
        .into_iter()
        .map(|h| h.path)
        .collect();
    assert!(hits.contains(&new), "{hits:?}");
    assert!(!hits.contains(&old), "{hits:?}");

    // The attached action, the milestones and the hard deadline follow.
    let actions = f.vault.list_actions("new").expect("list actions");
    assert!(
        actions.iter().any(|a| a
            .attached
            .as_ref()
            .is_some_and(|n| n.slug == "fit-the-baseline")),
        "{actions:?}"
    );
    assert!(!f.index.milestones_for_project(&new).unwrap().is_empty());
    assert!(f.index.milestones_for_project(&old).unwrap().is_empty());
    let commitments = f.vault.commitments(at().date(), 14).expect("commitments");
    assert!(
        commitments
            .iter()
            .any(|c| c.title.contains("Baseline fitted")
                && c.source == CommitmentSource::ProjectMilestone("new".to_owned())),
        "{commitments:?}"
    );
    assert!(
        f.read("commitments/send-the-error-budget.md")
            .contains("project: \"new\"\n")
    );
    // A referrer project's milestone rows carry its rewritten text.
    let emulator = f
        .index
        .milestones_for_project(&vp("projects/emulator.md"))
        .unwrap();
    assert_eq!(emulator.len(), 1);
    assert!(emulator[0].name.contains("[[new]]"), "{emulator:?}");
}

#[test]
fn history_is_one_search_over_the_new_slug() {
    let f = fixture();
    let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let states = |vault: &Vault| {
        vault
            .project_state_changes_between(from, at().date())
            .unwrap()
    };
    let before_states = states(&f.vault);
    // The focus started on the 8th, read on the 9th (inside the default
    // one-day carry-over window).
    let day_9 = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
    let before_focus = f
        .vault
        .current_focus(day_9)
        .unwrap()
        .expect("a focus is open");
    assert_eq!(before_focus.project, "old");
    assert_eq!(before_states.len(), 1);
    assert_eq!(before_states[0].project, "old");

    f.rename("old", "new");

    let after_states = states(&f.vault);
    assert_eq!(after_states.len(), 1);
    assert_eq!(after_states[0].project, "new");
    assert_eq!(
        (
            &after_states[0].old_state,
            &after_states[0].new_state,
            after_states[0].date
        ),
        (
            &before_states[0].old_state,
            &before_states[0].new_state,
            before_states[0].date
        )
    );
    let after_focus = f
        .vault
        .current_focus(day_9)
        .unwrap()
        .expect("the focus is still open");
    assert_eq!(after_focus.project, "new");
    assert_eq!(
        (&after_focus.action, after_focus.started, after_focus.date),
        (
            &before_focus.action,
            before_focus.started,
            before_focus.date
        )
    );
}

#[test]
fn a_stale_plan_is_refused_and_writes_nothing() {
    let f = fixture();
    let preview = f.preview("old", "new");
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace("Waiting on [[old]].", "Waiting on [[old]] again."),
    );
    let before = f.files();
    let err = f
        .vault
        .rename_note("old", "new", None, Some(&preview.hash), at())
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::StalePlan { expected, actual }
            if *expected == preview.hash && *actual != preview.hash),
        "{err:?}"
    );
    assert_eq!(f.files(), before, "nothing is written");
    assert!(
        f.index
            .find_by_path(&vp("projects/old.md"))
            .unwrap()
            .is_some()
    );

    // Without an expected plan there is nothing to be stale against.
    f.vault
        .rename_note("old", "new", None, None, at())
        .expect("unchecked rename");
    assert!(f.read("projects/new.md").contains("aliases: [old]"));
}

#[test]
fn a_rollover_is_a_stale_plan() {
    let f = fixture();
    let preview = f.preview("old", "new");
    let before = f.files();
    let err = f
        .vault
        .rename_note("old", "new", None, Some(&preview.hash), dt(11, 0, 5))
        .unwrap_err();
    assert!(matches!(err, DomainError::StalePlan { .. }), "{err:?}");
    assert_eq!(f.files(), before, "nothing is written");

    // Later the same day is the same plan: the stamp is not in the hash.
    let outcome = f
        .vault
        .rename_note("old", "new", None, Some(&preview.hash), dt(10, 23, 59))
        .expect("same day");
    assert_eq!(outcome.plan.hash, preview.hash);
}

#[test]
fn a_rename_to_itself_writes_nothing() {
    let f = fixture();
    let before = f.files();
    let plan = f.preview("old", "old");
    let outcome = f
        .vault
        .rename_note("old", "old", None, Some(&plan.hash), at())
        .expect("no-op");
    assert!(outcome.plan.is_empty());
    assert!(outcome.written.is_empty());
    assert_eq!(f.files(), before);
}

#[test]
fn a_failed_write_rolls_back_byte_identical() {
    let f = fixture();
    let preview = f.preview("old", "new");
    let before = files_of(&*f.mem);
    // The move is the first two ops (write the new path, delete the old);
    // the third, the first referrer's write, fails.
    let failing: Arc<dyn VaultStore> = Arc::new(FailingStore::new(Arc::clone(&f.mem), 3));
    let index: Arc<dyn VaultIndex> = f.index.clone();
    let vault = open(failing, index);
    let err = vault
        .rename_note("old", "new", None, Some(&preview.hash), at())
        .unwrap_err();
    assert!(
        matches!(err, DomainError::Store(StoreError::PermissionDenied(_))),
        "{err:?}"
    );
    assert_eq!(files_of(&*f.mem), before, "rolled back byte for byte");
    assert!(
        f.index
            .find_by_path(&vp("projects/old.md"))
            .unwrap()
            .is_some()
    );
    assert!(
        f.index
            .find_by_path(&vp("projects/new.md"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_failed_rollback_is_reported_per_path() {
    let f = fixture();
    let preview = f.preview("old", "new");
    // The third op fails, and the rollback's restore of the deleted old
    // path fails too; removing the new path still succeeds.
    let failing: Arc<dyn VaultStore> =
        Arc::new(FailingStore::new(Arc::clone(&f.mem), 3).with_stuck_path(vp("projects/old.md")));
    let index: Arc<dyn VaultIndex> = f.index.clone();
    let vault = open(failing, index);
    let err = vault
        .rename_note("old", "new", None, Some(&preview.hash), at())
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::RenameIncomplete { failed }
            if *failed == vec![vp("projects/old.md")]),
        "{err:?}"
    );
    assert!(err.to_string().contains("projects/old.md"), "{err}");
}

#[test]
fn index_stale_is_success() {
    let f = fixture();
    let preview = f.preview("old", "new");
    let flaky = Arc::new(FlakyIndex::new(f.index.clone()));
    let index: Arc<dyn VaultIndex> = flaky.clone();
    let vault = open(Arc::clone(&f.store), index);
    flaky.arm();
    let outcome = vault
        .rename_note("old", "new", None, Some(&preview.hash), at())
        .expect("the files landed, so the rename succeeded");
    assert!(outcome.index_stale);
    for (path, after) in destinations(&preview) {
        assert_eq!(f.read(&path.to_string()), after, "{path}");
    }
    assert!(!f.store.exists(&vp("projects/old.md")).unwrap());
}

#[test]
fn a_second_rename_extends_aliases() {
    let f = fixture();
    f.rename("old", "older");
    f.rename("older", "new");
    let note = f.read("projects/new.md");
    assert!(note.contains("aliases: [old, older]\n"), "{note}");
    assert_eq!(f.resolves("old"), vp("projects/new.md"));
    assert_eq!(f.resolves("older"), vp("projects/new.md"));
    assert!(
        f.read(DAILY_TODAY)
            .contains("renamed [[new]] \u{2014} Surrogate model\n  was: older\n")
    );
}

#[test]
fn renaming_back_to_an_own_alias_drops_it_from_the_list() {
    let f = fixture();
    f.rename("old", "new");
    let outcome = f.rename("new", "old");
    assert_eq!(
        outcome.plan.moves,
        vec![(vp("projects/new.md"), vp("projects/old.md"))]
    );
    // A slug is never its own alias; the slug left is one.
    let note = f.read("projects/old.md");
    assert!(note.contains("aliases: [new]\n"), "{note}");
    let aliases: Vec<String> = f
        .index
        .list_aliases()
        .unwrap()
        .into_iter()
        .filter(|(_, holder)| *holder == vp("projects/old.md"))
        .map(|(alias, _)| alias)
        .collect();
    assert_eq!(aliases, vec!["new".to_owned()]);
    assert_eq!(f.resolves("old"), vp("projects/old.md"));
    assert_eq!(f.resolves("new"), vp("projects/old.md"));
    // `[[old]]` and `[[new]]` both reach it from a note written now.
    f.vault
        .log_to_daily_note(dt(10, 11, 0), "compare [[old]] with [[new]]")
        .unwrap();
    let links = f.index.find_outgoing_links(&vp(DAILY_TODAY)).unwrap();
    for raw in ["old", "new"] {
        assert!(
            links
                .iter()
                .any(|l| l.target_raw == raw
                    && l.resolved_path.as_ref() == Some(&vp("projects/old.md"))),
            "`[[{raw}]]`: {links:?}"
        );
    }
}

#[test]
fn another_notes_alias_is_still_taken() {
    let f = fixture();
    // The emulator lists `newer`; the renamed note lists `shared`, and so
    // does the emulator: its own listing does not free a name another note
    // holds.
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace(
            "created: 2026-04-02\n",
            "created: 2026-04-02\naliases: [newer, shared]\n",
        ),
    );
    f.write(
        "projects/old.md",
        &PROJECT.replace(
            "created: 2026-04-01\n",
            "created: 2026-04-01\naliases: [shared]\n",
        ),
    );
    let before = f.files();
    for slug in ["newer", "shared"] {
        let err = f
            .vault
            .rename_note("projects/old", slug, None, None, at())
            .unwrap_err();
        assert!(
            matches!(&err, DomainError::SlugTaken { slug: s, holder, kind: TakenBy::Alias }
                if s == slug && *holder == vp("projects/emulator.md")),
            "{slug}: {err:?}"
        );
    }
    assert_eq!(f.files(), before, "nothing is written");
}

#[test]
fn a_rename_stages_remove_note_of_the_source_in_the_same_transaction() {
    let f = fixture();
    f.rename("old", "older");
    f.rename("older", "new");
    // No ghost row at a path the note left: the alias `old` has one holder
    // right after the commit, with no reconcile in between.
    assert!(
        f.index
            .find_by_path(&vp("projects/old.md"))
            .unwrap()
            .is_none()
    );
    assert!(
        f.index
            .find_by_path(&vp("projects/older.md"))
            .unwrap()
            .is_none()
    );
    let holders: Vec<VaultPath> = f
        .index
        .list_aliases()
        .unwrap()
        .into_iter()
        .filter(|(alias, _)| alias == "old")
        .map(|(_, holder)| holder)
        .collect();
    assert_eq!(holders, vec![vp("projects/new.md")]);
    assert_eq!(f.resolves("old"), vp("projects/new.md"));
}
