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
    VaultSnapshot, format_relinked_log_entry, format_renamed_log_entry,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use super::support::{CaseInsensitiveStore, FailingStore, FlakyIndex, RacingStore};

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
fn a_folder_note_other_than_a_portfolios_is_not_renamable_yet() {
    // Only `portfolios/<slug>/_index.md` moves its folder (T22): a
    // portfolio nested in a grouping folder is still refused.
    let f = fixture();
    f.write("portfolios/group/nested/_index.md", PORTFOLIO);
    let err = f.plan("portfolios/group/nested", "new").unwrap_err();
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

/// An upper-case `.MD` note is renamed by its exact path.
#[test]
fn an_upper_case_md_note_is_renamable_by_its_path() {
    let f = fixture();
    f.write("projects/other.MD", PROJECT);
    let plan = f.plan("projects/other.MD", "new").expect("plan");
    assert!(
        plan.moves
            .iter()
            .any(|(from, _)| *from == vp("projects/other.MD")),
        "moves: {:?}",
        plan.moves
    );
}

/// `x.MD` names that file, not the folder note its address `x` also reaches.
#[test]
fn an_upper_case_md_path_does_not_plan_the_sibling_folder_note() {
    let f = fixture();
    f.write("portfolios/surrogate-model/_index.md", PORTFOLIO);
    f.write("portfolios/surrogate-model.MD", PROJECT);
    if let Ok(plan) = f.plan("portfolios/surrogate-model.MD", "new") {
        assert!(
            plan.moves
                .iter()
                .all(|(from, _)| *from != vp("portfolios/surrogate-model/_index.md")),
            "moves: {:?}",
            plan.moves
        );
    }
}

#[test]
fn a_dangling_portfolio_field_is_not_rewritten_on_a_question_rename() {
    let f = fixture();
    f.write(
        "questions/research/how-large-is-the-error.md",
        "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# How large is the error?\n",
    );
    // An evidence note whose `portfolio:` equals the question's slug, with
    // no portfolio folder note: it names nothing that moves.
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
            .rename_note(target, new_slug, None, Some(&plan.hash), at(), || Ok(()))
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
        .rename_note("old", "new", None, Some(&preview.hash), at(), || Ok(()))
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
        .rename_note("old", "new", None, Some(&preview.hash), at(), || Ok(()))
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
        .rename_note("old", "new", None, None, at(), || Ok(()))
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
        .rename_note(
            "old",
            "new",
            None,
            Some(&preview.hash),
            dt(11, 0, 5),
            || Ok(()),
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::StalePlan { .. }), "{err:?}");
    assert_eq!(f.files(), before, "nothing is written");

    // Later the same day is the same plan: the stamp is not in the hash.
    let outcome = f
        .vault
        .rename_note(
            "old",
            "new",
            None,
            Some(&preview.hash),
            dt(10, 23, 59),
            || Ok(()),
        )
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
        .rename_note("old", "old", None, Some(&plan.hash), at(), || Ok(()))
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
        .rename_note("old", "new", None, Some(&preview.hash), at(), || Ok(()))
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
        .rename_note("old", "new", None, Some(&preview.hash), at(), || Ok(()))
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::RenameIncomplete { failed, source: StoreError::PermissionDenied(_) }
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
        .rename_note("old", "new", None, Some(&preview.hash), at(), || Ok(()))
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
            .rename_note("projects/old", slug, None, None, at(), || Ok(()))
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

// ---------------------------------------------------------------------
// An occupied destination (review of #906): the write never overwrites a
// file, whatever the snapshot showed.
// ---------------------------------------------------------------------

/// A vault over `store` holding `notes`, with `ignore` globs.
fn vault_over(store: Arc<dyn VaultStore>, notes: &[(&str, &str)], ignore: &[&str]) -> Vault {
    for (path, content) in notes {
        store.write_file(&vp(path), content).unwrap();
    }
    let config = VaultConfig {
        ignore: ignore.iter().map(|g| (*g).to_owned()).collect(),
        ..VaultConfig::default()
    };
    Vault::new(store, Arc::new(MemoryIndex::new()), config)
        .expect("vault opens")
        .0
}

/// Both verbs refuse renaming `target` to `new_slug` with `slug_taken {
/// kind: stem }` held by `holder`, and the store is left as it was. The
/// planner never asks the store: `by_snapshot` says whether its taken check
/// refuses too, or only the store's existence check does.
fn assert_destination_taken(
    vault: &Vault,
    store: &dyn VaultStore,
    target: &str,
    new_slug: &str,
    holder: &str,
    by_snapshot: bool,
) {
    let before = files_of(store);
    let refused = |err: DomainError| {
        assert!(
            matches!(&err, DomainError::SlugTaken { slug, holder: h, kind: TakenBy::Stem }
                if slug == new_slug && *h == vp(holder)),
            "{target} -> {new_slug}: {err:?}"
        );
    };
    let snapshot = vault.rename_snapshot().expect("snapshot");
    let planned = vault.plan_rename(&snapshot, target, new_slug, None, at());
    if by_snapshot {
        refused(planned.unwrap_err());
    } else {
        planned.expect("the snapshot's names compare exactly");
    }
    refused(
        vault
            .rename_preview(target, new_slug, None, at())
            .unwrap_err(),
    );
    refused(
        vault
            .rename_note(target, new_slug, None, None, at(), || Ok(()))
            .unwrap_err(),
    );
    assert_eq!(files_of(store), before, "nothing is written");
}

#[test]
fn an_ignored_file_at_the_destination_is_taken() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/old.md", PROJECT),
            ("projects/scratch.md", "Notes kept out of the vault.\n"),
        ],
        &["projects/scratch.md"],
    );
    assert_destination_taken(
        &vault,
        &*store,
        "old",
        "scratch",
        "projects/scratch.md",
        true,
    );
}

#[test]
fn a_case_variant_at_the_destination_is_taken() {
    let store: Arc<dyn VaultStore> = Arc::new(CaseInsensitiveStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/old.md", PROJECT),
            (
                "projects/Other.md",
                &EMULATOR.replace("# Emulator", "# Other"),
            ),
        ],
        &[],
    );
    assert_destination_taken(&vault, &*store, "old", "other", "projects/Other.md", false);
}

#[test]
fn a_case_variant_elsewhere_is_not_a_shared_stem() {
    // Resolution compares stems exactly, so `[[model]]` reaches only the
    // project whatever the question's file is called: renaming the project
    // is not refused as a shared stem, on either kind of store.
    let question = "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# Model\n";
    let notes = [
        ("projects/model.md", PROJECT),
        ("questions/research/Model.md", question),
    ];
    for store in [
        Arc::new(MemoryVaultStore::new()) as Arc<dyn VaultStore>,
        Arc::new(CaseInsensitiveStore::new()) as Arc<dyn VaultStore>,
    ] {
        let vault = vault_over(Arc::clone(&store), &notes, &[]);
        let plan = vault
            .rename_preview("projects/model", "fresh", None, at())
            .expect("not a shared stem");
        assert_eq!(
            plan.moves,
            vec![(vp("projects/model.md"), vp("projects/fresh.md"))]
        );
    }

    // A rename *to* the name is still refused where the filesystem cannot
    // tell it from a case variant at the destination: the store says so.
    let store: Arc<dyn VaultStore> = Arc::new(CaseInsensitiveStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/old.md", PROJECT),
            (
                "projects/Model.md",
                &EMULATOR.replace("# Emulator", "# Model"),
            ),
        ],
        &[],
    );
    assert_destination_taken(&vault, &*store, "old", "model", "projects/Model.md", false);
}

#[test]
fn a_case_only_rename_is_refused() {
    let store: Arc<dyn VaultStore> = Arc::new(CaseInsensitiveStore::new());
    let vault = vault_over(Arc::clone(&store), &[("projects/Old.md", PROJECT)], &[]);
    assert_destination_taken(
        &vault,
        &*store,
        "projects/Old",
        "old",
        "projects/Old.md",
        false,
    );
}

#[test]
fn a_file_appearing_at_the_destination_is_taken() {
    // The file lands behind the lock's back after the walk, so the
    // snapshot never lists it: only asking the store sees it.
    let f = fixture();
    let racing = Arc::new(RacingStore::new(
        Arc::clone(&f.mem),
        vp("projects/emulator.md"),
        vp("projects/new.md"),
        "Written by another tool.\n",
    ));
    let store: Arc<dyn VaultStore> = racing.clone();
    let index: Arc<dyn VaultIndex> = f.index.clone();
    let vault = open(store, index);
    racing.arm();
    let err = vault
        .rename_note("old", "new", None, None, at(), || Ok(()))
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::SlugTaken { holder, kind: TakenBy::Stem, .. }
            if *holder == vp("projects/new.md")),
        "{err:?}"
    );
    assert_eq!(f.read("projects/new.md"), "Written by another tool.\n");
    assert_eq!(f.read("projects/old.md"), PROJECT);
}

#[test]
fn a_leftover_link_already_reading_the_new_slug_is_not_an_edit() {
    let f = fixture();
    f.rename("old", "new");
    // By hand, after the rename: one link reads the new slug, one is left
    // over on the old (resolving through the alias), and a daily note
    // holds only leftovers.
    f.write(
        "projects/emulator.md",
        &EMULATOR.replace("Waiting on [[old]].", "Waiting on [[new]]."),
    );
    let day_9 = "journal/2026/daily/2026-05-09.md";
    f.write(day_9, DAY_9);

    let plan = f.preview("new", "old");
    for edit in &plan.edits {
        assert_ne!(edit.before, edit.after, "{} is not an edit", edit.path);
    }
    assert!(
        plan.edits.iter().all(|e| e.path != vp(day_9)),
        "a note holding only leftovers is not in the plan"
    );
    let emulator = plan
        .edits
        .iter()
        .find(|e| e.path == vp("projects/emulator.md"))
        .expect("the emulator links the note as `[[new]]`");
    assert_eq!(emulator.changes, vec![Change::Address { count: 1 }]);
    assert!(emulator.after.contains("Waiting on [[old]]."));
    assert!(emulator.after.contains("Hand-off from [[old]]"));
}

#[test]
fn a_case_variant_in_another_folder_is_not_taken() {
    // Names compare exactly, as resolution and `create_project` compare
    // them: a stem differing only in case in another folder never shares
    // a file with the destination.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let question =
        "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# Kernel trick\n";
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/old.md", PROJECT),
            ("questions/research/Kernel-Trick.md", question),
        ],
        &[],
    );
    let outcome = vault
        .rename_note("old", "kernel-trick", None, None, at(), || Ok(()))
        .expect("not taken");
    assert_eq!(outcome.written[0], vp("projects/kernel-trick.md"));
    assert_eq!(
        store
            .read_file(&vp("questions/research/Kernel-Trick.md"))
            .unwrap(),
        question
    );
}

#[test]
fn a_case_only_rename_works_where_the_filesystem_tells_the_names_apart() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(Arc::clone(&store), &[("projects/Old.md", PROJECT)], &[]);
    let plan = vault
        .rename_preview("projects/Old", "old", None, at())
        .expect("preview");
    vault
        .rename_note("projects/Old", "old", None, Some(&plan.hash), at(), || {
            Ok(())
        })
        .expect("rename");
    assert!(!store.exists(&vp("projects/Old.md")).unwrap());
    let note = store.read_file(&vp("projects/old.md")).unwrap();
    assert!(note.contains("aliases: [Old]\n"), "{note}");
}

// ---------------------------------------------------------------------
// A portfolio (T22): the folder moves as one note.
// ---------------------------------------------------------------------

const P_INDEX: &str = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\nproject: \"[[projects/surrogate-model]]\"\n---\n\n# How large is the error?\n\n## Evidence\n- [[portfolios/old/2026-10-12-baseline]]\n- [[portfolios/old/2026-10-13-scan]]\n";

const P_BASELINE: &str = "---\ntype: evidence\ncreated: 2026-05-02\nsource: \"Baseline sweep\"\nportfolio: old\norigin: \"[[projects/surrogate-model]]\"\n---\n\n# Baseline sweep\n\nFiled in [[portfolios/old]], beside [[portfolios/old/2026-10-13-scan]].\n";

const P_SCAN: &str = "---\ntype: evidence\ncreated: 2026-05-03\nsource: \"Scan\"\nportfolio: old\norigin: \"[[projects/surrogate-model]]\"\nkind: pdf\n---\n\n# Scan\n\n## Attachment\n\n[scan.pdf](<./2026-10-13-scan/scan.pdf>)\n\n## Abstract\n\nThe residuals, scanned.\n";

const P_PDF: &str = "%PDF-1.4 the residuals, scanned";

const P_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCollecting evidence in [[portfolios/old]].\n\n## Next Actions\n\n## Milestones\n";

const P_DAY: &str = "---\ntype: daily\ndate: 2026-05-09\n---\n\n# Saturday, 9 May 2026\n\n## Logs\n- **09:00**: scanned ![[portfolios/old/2026-10-13-scan/scan.pdf]]\n- **09:30**: see [[portfolios/old/2026-10-12-baseline|the baseline]] and [[portfolios/old#Evidence]]\n";

const P_SIBLING: &str = "---\ntype: portfolio\nquestion: \"Is the emulator faster?\"\ncreated: 2026-05-01\n---\n\n# Is the emulator faster?\n";

/// A vault holding portfolio `old` ("How large is the error?") with two
/// evidence notes (`portfolio: old`), one of them an attachment stub owning
/// a PDF, a project map linking `[[portfolios/old]]`, a daily note
/// embedding the PDF, and a sibling portfolio `surrogate-emulator`.
fn portfolio_files(pdf: &str, pdf_name: &str) -> Vec<(String, String)> {
    vec![
        ("portfolios/old/_index.md".into(), P_INDEX.into()),
        (
            "portfolios/old/2026-10-12-baseline.md".into(),
            P_BASELINE.into(),
        ),
        (
            "portfolios/old/2026-10-13-scan.md".into(),
            P_SCAN.replace("scan.pdf", pdf_name),
        ),
        (
            format!("portfolios/old/2026-10-13-scan/{pdf_name}"),
            pdf.into(),
        ),
        (
            "portfolios/surrogate-emulator/_index.md".into(),
            P_SIBLING.into(),
        ),
        ("projects/surrogate-model.md".into(), P_PROJECT.into()),
        (
            "journal/2026/daily/2026-05-09.md".into(),
            P_DAY.replace("scan.pdf", pdf_name),
        ),
    ]
}

struct PortfolioVault {
    store: Arc<dyn VaultStore>,
    index: Arc<MemoryIndex>,
    vault: Vault,
}

fn portfolio_vault_over(store: Arc<dyn VaultStore>, files: &[(String, String)]) -> PortfolioVault {
    for (path, content) in files {
        store.write_file(&vp(path), content).unwrap();
    }
    let index = Arc::new(MemoryIndex::new());
    let vault = open(Arc::clone(&store), index.clone());
    PortfolioVault {
        store,
        index,
        vault,
    }
}

fn portfolio_vault() -> PortfolioVault {
    portfolio_vault_over(
        Arc::new(MemoryVaultStore::new()),
        &portfolio_files(P_PDF, "scan.pdf"),
    )
}

impl PortfolioVault {
    fn plan(&self, target: &str, new_slug: &str) -> Result<RenamePlan, DomainError> {
        let snapshot = self.vault.rename_snapshot().expect("snapshot");
        self.vault
            .plan_rename(&snapshot, target, new_slug, None, at())
    }

    fn read(&self, path: &str) -> String {
        self.store.read_file(&vp(path)).unwrap()
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

#[test]
fn a_portfolio_rename_moves_every_file_and_rewrites_the_folder_segment() {
    let p = portfolio_vault();
    let pdf_before = p
        .store
        .read_bytes(&vp("portfolios/old/2026-10-13-scan/scan.pdf"))
        .unwrap();
    for target in ["portfolios/old", "portfolio:old", "old"] {
        let plan = p.plan(target, "new").expect(target);
        assert_eq!(plan.target.path, vp("portfolios/old/_index.md"), "{target}");
    }
    let preview = p
        .vault
        .rename_preview("portfolio:old", "new", None, at())
        .expect("preview");
    let outcome = p
        .vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .expect("rename");
    assert!(!outcome.index_stale);

    // Every file at the new path, nothing at the old one, the PDF's bytes
    // as they were.
    let files: Vec<String> = p
        .store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|p| p.to_string())
        .collect();
    assert!(
        !files.iter().any(|f| f.starts_with("portfolios/old/")),
        "{files:?}"
    );
    for moved in [
        "portfolios/new/_index.md",
        "portfolios/new/2026-10-12-baseline.md",
        "portfolios/new/2026-10-13-scan.md",
        "portfolios/new/2026-10-13-scan/scan.pdf",
    ] {
        assert!(files.iter().any(|f| f == moved), "{moved}: {files:?}");
    }
    assert_eq!(
        p.store
            .read_bytes(&vp("portfolios/new/2026-10-13-scan/scan.pdf"))
            .unwrap(),
        pdf_before
    );
    assert!(
        p.read("portfolios/new/_index.md")
            .contains("aliases: [old]\n---\n")
    );

    // The evidence notes' `portfolio:` field and every folder segment,
    // inside the folder and out, read the new slug.
    for evidence in [
        "portfolios/new/2026-10-12-baseline.md",
        "portfolios/new/2026-10-13-scan.md",
    ] {
        assert!(
            p.read(evidence).contains("\nportfolio: new\n"),
            "{evidence}"
        );
    }
    assert!(
        p.read("portfolios/new/2026-10-12-baseline.md")
            .contains("Filed in [[portfolios/new]], beside [[portfolios/new/2026-10-13-scan]].\n")
    );
    assert!(
        p.read("projects/surrogate-model.md")
            .contains("Collecting evidence in [[portfolios/new]].\n")
    );
    let day = p.read("journal/2026/daily/2026-05-09.md");
    assert!(
        day.contains("![[portfolios/new/2026-10-13-scan/scan.pdf]]"),
        "{day}"
    );
    assert!(
        day.contains(
            "[[portfolios/new/2026-10-12-baseline|the baseline]] and [[portfolios/new#Evidence]]"
        ),
        "{day}"
    );
    assert!(p.read(DAILY_TODAY).ends_with(
        "- **10:30**: renamed [[portfolios/new]] \u{2014} How large is the error?\n  was: old\n"
    ));
    // The stub's relative link to its artefact needed nothing.
    assert!(
        p.read("portfolios/new/2026-10-13-scan.md")
            .contains("[scan.pdf](<./2026-10-13-scan/scan.pdf>)")
    );

    // Every referrer resolves through the index, and the old paths are gone.
    let index = vp("portfolios/new/_index.md");
    let baseline = vp("portfolios/new/2026-10-12-baseline.md");
    let mut backlinks = p.index.find_backlinks(&index).unwrap();
    backlinks.sort();
    assert_eq!(
        backlinks,
        vec![
            vp("journal/2026/daily/2026-05-09.md"),
            vp(DAILY_TODAY),
            baseline.clone(),
            vp("projects/surrogate-model.md"),
        ]
    );
    let mut backlinks = p.index.find_backlinks(&baseline).unwrap();
    backlinks.sort();
    assert_eq!(
        backlinks,
        vec![vp("journal/2026/daily/2026-05-09.md"), index.clone()]
    );
    for old in [
        "portfolios/old/_index.md",
        "portfolios/old/2026-10-12-baseline.md",
        "portfolios/old/2026-10-13-scan.md",
    ] {
        assert!(p.index.find_by_path(&vp(old)).unwrap().is_none(), "{old}");
    }
    assert_eq!(p.resolves("portfolios/new/_index"), index);
    assert_eq!(p.resolves("portfolio:new"), index);
    assert_eq!(p.resolves("new"), index);
    assert_eq!(p.resolves("portfolio:old"), index, "the old name resolves");
    assert_eq!(p.resolves("portfolios/new/2026-10-12-baseline"), baseline);
    let contents = p.vault.get_portfolio_contents("new").expect("contents");
    assert_eq!(contents.len(), 2, "{contents:?}");
    assert!(p.vault.get_portfolio_contents("old").unwrap().is_empty());

    // The written paths: the index note first, then its folder's files.
    assert_eq!(outcome.written[0], index);
    for (_, to) in &outcome.plan.moves {
        assert!(outcome.written.contains(to), "{to}");
    }

    let report = p.vault.lint_all_notes(super::support::lint_day()).unwrap();
    assert_eq!(report.error_count(), 0, "{:?}", report.issues);
}

#[test]
fn a_note_without_an_index_row_still_follows_a_folder_rename() {
    // `broken-scan` has no frontmatter, so the rename stages no index row
    // for it; it is still a note in the moved folder, and a link to it
    // resolves to its new path.
    let mut files = portfolio_files(P_PDF, "scan.pdf");
    files.push((
        "portfolios/old/broken-scan.md".into(),
        "# Broken scan\n".into(),
    ));
    files.push((
        "projects/bias-variance-split.md".into(),
        P_PROJECT.replace("Surrogate model", "Bias variance split") + "\nSee [[broken-scan]].\n",
    ));
    let p = portfolio_vault_over(Arc::new(MemoryVaultStore::new()), &files);
    let linker = vp("projects/bias-variance-split.md");
    let resolved = |p: &PortfolioVault| {
        p.index
            .find_outgoing_links(&linker)
            .unwrap()
            .into_iter()
            .find(|l| l.target_raw == "broken-scan")
            .and_then(|l| l.resolved_path)
    };
    assert_eq!(resolved(&p), Some(vp("portfolios/old/broken-scan.md")));

    let preview = p
        .vault
        .rename_preview("portfolio:old", "new", None, at())
        .expect("preview");
    p.vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .expect("rename");

    assert!(
        p.store
            .exists(&vp("portfolios/new/broken-scan.md"))
            .unwrap()
    );
    assert_eq!(resolved(&p), Some(vp("portfolios/new/broken-scan.md")));
}

#[test]
fn a_portfolio_rename_is_refused_while_a_question_shares_its_slug() {
    let p = portfolio_vault();
    p.store
        .write_file(
            &vp("questions/research/old.md"),
            "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# How large is the error?\n",
        )
        .unwrap();
    let err = p.plan("portfolios/old", "new").unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "rename the question"),
        "{err:?}"
    );
    // The question, for its part, renames the pair (T23): not refused.
    let plan = p.plan("questions/research/old", "new").expect("the pair");
    assert!(
        plan.moves
            .iter()
            .any(|(from, _)| *from == vp("portfolios/old/_index.md"))
    );
}

#[test]
fn a_portfolio_rename_hash_covers_artefact_moves_without_their_bytes() {
    // A store that panics on any read of a non-markdown file under
    // `portfolios/`: neither the plan, the preview nor the write reads one.
    let guarded = || {
        let store: Arc<dyn VaultStore> = Arc::new(HookStore {
            no_artefact_reads: true,
            ..HookStore::new()
        });
        store
    };
    let hash = |pdf: &str, name: &str| {
        portfolio_vault_over(guarded(), &portfolio_files(pdf, name))
            .plan("portfolios/old", "new")
            .expect("plan")
            .hash
    };
    let base = hash(P_PDF, "scan.pdf");
    assert_eq!(
        base,
        hash("%PDF-1.4 another scan altogether", "scan.pdf"),
        "an artefact's bytes are not hashed"
    );
    // The stub and the daily embed name the file, so rename only the
    // artefact itself: a second artefact whose name differs.
    let with_extra = |name: &str| {
        let mut files = portfolio_files(P_PDF, "scan.pdf");
        files.push((
            format!("portfolios/old/2026-10-13-scan/{name}"),
            "%PDF-1.4 a second page".into(),
        ));
        portfolio_vault_over(guarded(), &files)
            .plan("portfolios/old", "new")
            .expect("plan")
            .hash
    };
    assert_ne!(
        base,
        with_extra("page-2.pdf"),
        "an artefact's move is hashed"
    );
    assert_ne!(
        with_extra("page-2.pdf"),
        with_extra("page-3.pdf"),
        "an artefact's name is hashed"
    );
    let p = portfolio_vault_over(guarded(), &portfolio_files(P_PDF, "scan.pdf"));
    let preview = p
        .vault
        .rename_preview("portfolios/old", "new", None, at())
        .expect("preview reads no artefact");
    p.vault
        .rename_note(
            "portfolios/old",
            "new",
            None,
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .expect("the write reads no artefact");
    assert!(
        p.store
            .exists(&vp("portfolios/new/2026-10-13-scan/scan.pdf"))
            .unwrap()
    );
}

#[test]
fn a_folder_note_shared_stem_compares_folder_names() {
    // Another folder note with the same folder name (an expanded
    // stewardship): a link by folder name cannot tell them apart.
    let p = portfolio_vault();
    p.store
        .write_file(
            &vp("stewardships/old/_index.md"),
            "---\ntype: stewardship\ncreated: 2026-05-01\n---\n\n# Old\n",
        )
        .unwrap();
    let err = p.plan("portfolios/old", "new").unwrap_err();
    assert!(
        matches!(&err, DomainError::StemShared { others, .. }
            if *others == vec![vp("stewardships/old/_index.md")]),
        "{err:?}"
    );

    // Every folder note's file stem is `_index`, so the sibling portfolio
    // does not share one; a flat note named like the folder does not
    // either (it is the taken check's business).
    let p = portfolio_vault();
    p.store.write_file(&vp("projects/old.md"), PROJECT).unwrap();
    p.plan("portfolios/old", "new")
        .expect("neither the sibling nor the flat note shares the name");

    // Two portfolios cannot share a folder name: the taken check refuses
    // the sibling's, and a flat note's stem likewise.
    let err = p.plan("portfolios/old", "surrogate-emulator").unwrap_err();
    assert!(
        matches!(&err, DomainError::SlugTaken { holder, kind: TakenBy::Folder, .. }
            if *holder == vp("portfolios/surrogate-emulator/_index.md")),
        "{err:?}"
    );
    let err = p.plan("portfolios/old", "surrogate-model").unwrap_err();
    assert!(
        matches!(
            &err,
            DomainError::SlugTaken {
                kind: TakenBy::Stem,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_portfolio_destination_folder_that_exists_is_taken() {
    // A folder of non-notes at the destination: no name in the snapshot
    // holds it, but the move never merges into a folder that is there.
    let p = portfolio_vault();
    p.store
        .write_file(&vp("portfolios/new/stray.txt"), "left over")
        .unwrap();
    p.plan("portfolios/old", "new")
        .expect("the snapshot's names are free");
    for refused in [
        p.vault.rename_preview("portfolios/old", "new", None, at()),
        p.vault
            .rename_note("portfolios/old", "new", None, None, at(), || Ok(()))
            .map(|o| o.plan),
    ] {
        let err = refused.unwrap_err();
        assert!(
            matches!(&err, DomainError::SlugTaken { holder, kind: TakenBy::Folder, .. }
                if *holder == vp("portfolios/new")),
            "{err:?}"
        );
    }
    assert!(p.store.exists(&vp("portfolios/old/_index.md")).unwrap());
}

/// A `MemoryVaultStore` with test hooks: it records every folder the
/// rename asks it to prune (and fails each one), lists `dirs` as its
/// folders (the memory store's are implicit, so an empty one cannot exist),
/// and with `no_artefact_reads` panics on any read of a non-markdown file
/// under `portfolios/`.
struct HookStore {
    inner: Arc<MemoryVaultStore>,
    pruned: std::sync::Mutex<Vec<VaultPath>>,
    dirs: Vec<VaultPath>,
    no_artefact_reads: bool,
    fail_moves: bool,
}

impl HookStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(MemoryVaultStore::new()),
            pruned: std::sync::Mutex::new(Vec::new()),
            dirs: Vec::new(),
            no_artefact_reads: false,
            fail_moves: false,
        }
    }

    fn guard_read(&self, path: &VaultPath) {
        let artefact = path.as_path().starts_with("portfolios")
            && path.as_path().extension() != Some(std::ffi::OsStr::new("md"));
        assert!(
            !(self.no_artefact_reads && artefact),
            "an artefact's bytes were read: {path}"
        );
    }
}

impl VaultStore for HookStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.guard_read(path);
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.guard_read(path);
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        if self.fail_moves {
            return Err(StoreError::PermissionDenied(dest.to_string()));
        }
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn remove_dir_if_empty(&self, dir: &VaultPath) -> Result<bool, StoreError> {
        self.pruned.lock().unwrap().push(dir.clone());
        Err(StoreError::PermissionDenied(dir.to_string()))
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn walk_dirs(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        Ok(self
            .dirs
            .iter()
            .filter(|d| d.as_path().starts_with(path.as_path()) && *d != path)
            .cloned()
            .collect())
    }
    fn metadata(&self, path: &VaultPath) -> Result<cdno_core::file_meta::FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
}

#[test]
fn a_portfolio_rename_prunes_only_at_or_below_its_folder_and_never_fails_on_it() {
    let hooks = Arc::new(HookStore {
        dirs: [
            "portfolios/old/2026-10-13-scan",
            "portfolios/old/scratch",
            "portfolios/old/scratch/deeper",
            "portfolios/surrogate-emulator/figures",
        ]
        .into_iter()
        .map(vp)
        .collect(),
        ..HookStore::new()
    });
    let store: Arc<dyn VaultStore> = hooks.clone();
    let p = portfolio_vault_over(store, &portfolio_files(P_PDF, "scan.pdf"));
    p.vault
        .rename_note("portfolios/old", "new", None, None, at(), || Ok(()))
        .expect("a failed prune does not fail the rename");
    assert_eq!(
        *hooks.pruned.lock().unwrap(),
        vec![
            vp("portfolios/old/scratch/deeper"),
            vp("portfolios/old/2026-10-13-scan"),
            vp("portfolios/old/scratch"),
            vp("portfolios/old"),
        ],
        "every folder under the old one, deepest first, an empty one the \
         move never saw included, then the folder, and nothing above it"
    );

    // A flat rename prunes nothing.
    hooks.pruned.lock().unwrap().clear();
    p.vault
        .rename_note(
            "projects/surrogate-model",
            "surrogate",
            None,
            None,
            at(),
            || Ok(()),
        )
        .expect("flat rename");
    assert!(hooks.pruned.lock().unwrap().is_empty());
}

#[test]
fn a_hidden_note_at_the_top_level_moves_with_its_field_rewritten() {
    let mut files = portfolio_files(P_PDF, "scan.pdf");
    files.push((
        "portfolios/old/.draft.md".into(),
        "---\ntype: evidence\ncreated: 2026-05-04\nsource: \"Draft\"\nportfolio: old\norigin: \"[[projects/surrogate-model]]\"\n---\n\n# Draft\n".into(),
    ));
    files.push(("portfolios/old/.DS_Store".into(), "Bud1".into()));
    let p = portfolio_vault_over(Arc::new(MemoryVaultStore::new()), &files);
    p.vault
        .rename_note("portfolios/old", "new", None, None, at(), || Ok(()))
        .expect("rename");
    assert!(
        !p.store.exists(&vp("portfolios/old")).unwrap(),
        "nothing left"
    );
    assert!(
        p.read("portfolios/new/.draft.md")
            .contains("\nportfolio: new\n")
    );
    assert!(
        p.index
            .find_by_path(&vp("portfolios/new/.draft.md"))
            .unwrap()
            .is_some(),
        "a hidden note is a note: its index row moves"
    );
    assert!(
        p.index
            .find_by_path(&vp("portfolios/old/.draft.md"))
            .unwrap()
            .is_none()
    );
    assert_eq!(p.read("portfolios/new/.DS_Store"), "Bud1");
}

#[test]
fn an_alias_reached_link_is_rewritten_to_the_folder_form() {
    let mut files = portfolio_files(P_PDF, "scan.pdf");
    files[0].1 = P_INDEX.replace(
        "project: \"[[projects/surrogate-model]]\"\n",
        "project: \"[[projects/surrogate-model]]\"\naliases: [error-budget]\n",
    );
    files.push((
        "journal/2026/daily/2026-05-08.md".into(),
        "---\ntype: daily\ndate: 2026-05-08\n---\n\n# Friday, 8 May 2026\n\n## Logs\n- **09:00**: opened [[error-budget|the budget]]\n".into(),
    ));
    let p = portfolio_vault_over(Arc::new(MemoryVaultStore::new()), &files);
    p.vault
        .rename_note("portfolios/old", "new", None, None, at(), || Ok(()))
        .expect("rename");
    let day = p.read("journal/2026/daily/2026-05-08.md");
    assert!(
        day.contains("opened [[portfolios/new|the budget]]\n"),
        "a folder note answers to its folder path, never a bare name: {day}"
    );
    let mut backlinks = p
        .index
        .find_backlinks(&vp("portfolios/new/_index.md"))
        .unwrap();
    backlinks.sort();
    assert!(backlinks.contains(&vp("journal/2026/daily/2026-05-08.md")));
}

#[test]
fn a_custom_type_folder_note_is_refused_as_folder() {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        cdno_core::config::CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    store
        .write_file(
            &vp("concepts/old/_index.md"),
            "---\ntype: concept\ncreated: 2026-05-01\n---\n\n# Surrogate model\n",
        )
        .unwrap();
    store
        .write_file(
            &vp("concepts/flat.md"),
            "---\ntype: concept\ncreated: 2026-05-01\n---\n\n# Surrogate model\n",
        )
        .unwrap();
    let vault = Vault::new(store, Arc::new(MemoryIndex::new()), config)
        .expect("vault opens")
        .0;
    let snapshot = vault.rename_snapshot().unwrap();
    // The type is renamable: a flat note of it is.
    vault
        .plan_rename(&snapshot, "concepts/flat", "new", None, at())
        .expect("a flat custom note is renamable");
    let err = vault
        .plan_rename(&snapshot, "concepts/old", "new", None, at())
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "folder"),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------
// The question-portfolio pair (T23): one plan, one transaction.
// ---------------------------------------------------------------------

const PAIR_QUESTION: &str = "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# How large is the error?\n\n## Related Portfolios\n- [[portfolios/old]]\n";

const PAIR_INDEX: &str = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\nproject: \"[[projects/surrogate-model]]\"\n---\n\n# How large is the error?\n\n## Related Questions\n- [[questions/research/old]]\n\n## Evidence\n- [[portfolios/old/2026-10-12-baseline]]\n- [[portfolios/old/2026-10-13-scan]]\n";

/// Portfolio `old` with its evidence and a same-slug question `old`, each
/// linking the other.
fn pair_vault_over(store: Arc<dyn VaultStore>) -> PortfolioVault {
    let mut files = portfolio_files(P_PDF, "scan.pdf");
    for (path, content) in &mut files {
        if path == "portfolios/old/_index.md" {
            *content = PAIR_INDEX.to_owned();
        }
    }
    files.push(("questions/research/old.md".into(), PAIR_QUESTION.into()));
    portfolio_vault_over(store, &files)
}

fn pair_vault() -> PortfolioVault {
    pair_vault_over(Arc::new(MemoryVaultStore::new()))
}

#[test]
fn a_question_rename_carries_its_portfolio() {
    let p = pair_vault();
    let preview = p
        .vault
        .rename_preview(
            "questions/research/old",
            "new",
            Some("a clearer name"),
            at(),
        )
        .expect("preview");
    // The question's flat move, then the portfolio's index, then its files.
    assert_eq!(
        preview.moves[0],
        (
            vp("questions/research/old.md"),
            vp("questions/research/new.md")
        )
    );
    assert_eq!(
        preview.moves[1],
        (
            vp("portfolios/old/_index.md"),
            vp("portfolios/new/_index.md")
        )
    );
    assert_eq!(preview.moves.len(), 5, "{:?}", preview.moves);
    let outcome = p
        .vault
        .rename_note(
            "questions/research/old",
            "new",
            Some("a clearer name"),
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .expect("rename");
    assert!(!outcome.index_stale);

    // Both moved; neither name is left behind.
    let files: Vec<String> = p
        .store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|f| f.to_string())
        .collect();
    assert!(!files.iter().any(|f| f.starts_with("portfolios/old/")));
    assert!(!files.iter().any(|f| f == "questions/research/old.md"));
    for moved in [
        "questions/research/new.md",
        "portfolios/new/_index.md",
        "portfolios/new/2026-10-12-baseline.md",
        "portfolios/new/2026-10-13-scan/scan.pdf",
    ] {
        assert!(files.iter().any(|f| f == moved), "{moved}: {files:?}");
    }

    // Each links the other by the new name.
    assert!(
        p.read("questions/research/new.md")
            .contains("- [[portfolios/new]]\n")
    );
    assert!(
        p.read("portfolios/new/_index.md")
            .contains("- [[questions/research/new]]\n")
    );
    // The evidence rows follow.
    for evidence in [
        "portfolios/new/2026-10-12-baseline.md",
        "portfolios/new/2026-10-13-scan.md",
    ] {
        assert!(
            p.read(evidence).contains("\nportfolio: new\n"),
            "{evidence}"
        );
    }
    // The correlation `find_question_path(slug)` keys on survives: the
    // question and the portfolio share the new stem, and both resolve.
    assert_eq!(p.resolves("question:new"), vp("questions/research/new.md"));
    assert_eq!(p.resolves("portfolio:new"), vp("portfolios/new/_index.md"));
    assert_eq!(p.vault.get_portfolio_contents("new").unwrap().len(), 2);

    // One log line naming the question, with the continuation last.
    let day = p.read(DAILY_TODAY);
    assert_eq!(day.matches("renamed [[").count(), 1, "{day}");
    assert!(
        day.contains(
            "renamed [[questions/research/new]] \u{2014} How large is the error?\n  was: old\n  reason: a clearer name\n  portfolio: [[portfolios/new]]\n"
        ),
        "{day}"
    );
    let report = p.vault.lint_all_notes(super::support::lint_day()).unwrap();
    assert_eq!(report.error_count(), 0, "{:?}", report.issues);
}

#[test]
fn a_question_without_a_portfolio_renames_alone() {
    let p = portfolio_vault();
    p.store
        .write_file(
            &vp("questions/research/lonely.md"),
            "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# Is it lonely?\n",
        )
        .unwrap();
    // Evidence in another portfolio whose `portfolio:` is not the question's.
    let plan = p.plan("questions/research/lonely", "alone").expect("plan");
    assert_eq!(plan.moves.len(), 1, "{:?}", plan.moves);
    assert!(
        plan.edits
            .iter()
            .all(|e| !e.path.to_string().starts_with("portfolios/")),
        "{:?}",
        plan.edits
            .iter()
            .map(|e| e.path.to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        plan.edits
            .iter()
            .flat_map(|e| &e.changes)
            .all(|c| !matches!(c, Change::Field { key } if key == "portfolio"))
    );
    let day = plan
        .edits
        .iter()
        .find(|e| e.path.to_string().contains("journal/"))
        .expect("the log line");
    assert!(!day.after.contains("portfolio: [["), "{}", day.after);
}

#[test]
fn a_question_rename_is_one_plan_hash() {
    let p = pair_vault();
    let preview = p
        .vault
        .rename_preview("questions/research/old", "new", None, at())
        .expect("preview");
    // Move an artefact of the portfolio's folder between preview and write:
    // the portfolio's moves are in the question's plan hash.
    let from = vp("portfolios/old/2026-10-13-scan/scan.pdf");
    let text = p.read("portfolios/old/2026-10-13-scan/scan.pdf");
    p.store.delete_file(&from).unwrap();
    p.store
        .write_file(&vp("portfolios/old/2026-10-13-scan/figure.pdf"), &text)
        .unwrap();
    let err = p
        .vault
        .rename_note(
            "questions/research/old",
            "new",
            None,
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::StalePlan { .. }), "{err:?}");
    // And nothing was written.
    assert!(p.store.exists(&vp("questions/research/old.md")).unwrap());
    assert!(p.store.exists(&vp("portfolios/old/_index.md")).unwrap());
}

#[test]
fn the_pair_checks_both_destinations() {
    // A folder of files only (no folder note) at `portfolios/new/`: the
    // question's own destination is free, the portfolio's is not.
    let p = pair_vault();
    p.store
        .write_file(&vp("portfolios/new/kept.pdf"), "%PDF")
        .unwrap();
    let err = p
        .vault
        .rename_preview("questions/research/old", "new", None, at())
        .unwrap_err();
    assert!(
        matches!(
            &err,
            DomainError::SlugTaken {
                kind: TakenBy::Folder,
                ..
            }
        ),
        "{err:?}"
    );
    let err = p
        .vault
        .rename_note("questions/research/old", "new", None, None, at(), || Ok(()))
        .unwrap_err();
    assert!(matches!(err, DomainError::SlugTaken { .. }), "{err:?}");
    assert!(p.store.exists(&vp("questions/research/old.md")).unwrap());

    // A question already at the new name, in the other domain, is taken.
    let p = pair_vault();
    p.store
        .write_file(
            &vp("questions/life/new.md"),
            "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# Another\n",
        )
        .unwrap();
    let err = p.plan("questions/research/old", "new").unwrap_err();
    assert!(matches!(err, DomainError::SlugTaken { .. }), "{err:?}");
}

#[test]
fn renamed_pair_entry_places_portfolio_after_was_with_a_blank_reason() {
    use cdno_domain::vault::format_renamed_pair_log_entry;
    let head = "renamed [[questions/research/new]] \u{2014} How large is the error?";
    assert_eq!(
        format_renamed_pair_log_entry(
            "questions/research/new",
            "How large is the error?",
            "old",
            Some(" \n "),
            Some("portfolios/new"),
        ),
        format!("{head}\n  was: old\n  portfolio: [[portfolios/new]]")
    );
    assert_eq!(
        format_renamed_pair_log_entry(
            "questions/research/new",
            "How large is the error?",
            "old",
            Some("clearer"),
            Some("portfolios/new"),
        ),
        format!("{head}\n  was: old\n  reason: clearer\n  portfolio: [[portfolios/new]]")
    );
    assert_eq!(
        format_renamed_pair_log_entry(
            "questions/research/new",
            "How large is the error?",
            "old",
            None,
            None,
        ),
        format!("{head}\n  was: old")
    );
}

#[test]
fn a_bare_slug_names_the_pairs_question() {
    let p = pair_vault();
    let plan = p.plan("old", "new").expect("the pair, by its bare slug");
    assert_eq!(plan.target.path, vp("questions/research/old.md"));
    assert!(
        plan.moves
            .iter()
            .any(|(from, _)| *from == vp("portfolios/old/_index.md"))
    );
    // Any other multi-hit shape stays ambiguous: two questions, one in each
    // domain, answer to the same slug.
    let p = pair_vault();
    p.store
        .write_file(
            &vp("questions/life/old.md"),
            "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# Another\n",
        )
        .unwrap();
    let err = p.plan("old", "new").unwrap_err();
    assert!(matches!(err, DomainError::AmbiguousSlug(_)), "{err:?}");
}

#[test]
fn a_straggler_folder_link_resolves_through_the_question_until_913() {
    // Pins today's resolver (#913): the alias rule ignores the folder
    // segment, so a `[[portfolios/old]]` the rename never saw resolves to
    // the question, and lint names the question. A resolver fix flips this.
    let p = pair_vault();
    p.vault
        .rename_note("questions/research/old", "new", None, None, at(), || Ok(()))
        .expect("rename");
    p.store
        .write_file(
            &vp("journal/2026/daily/2026-05-11.md"),
            "---\ntype: daily\ndate: 2026-05-11\n---\n\n# Monday, 11 May 2026\n\n## Logs\n- **09:00**: filed in [[portfolios/old]]\n",
        )
        .unwrap();
    let index = Arc::new(MemoryIndex::new());
    let vault = open(Arc::clone(&p.store), index.clone());
    assert!(
        index
            .find_backlinks(&vp("questions/research/new.md"))
            .unwrap()
            .contains(&vp("journal/2026/daily/2026-05-11.md"))
    );
    let report = vault.lint_all_notes(super::support::lint_day()).unwrap();
    assert!(
        report.issues.iter().any(|i| i
            .message
            .contains("`[[portfolios/old]]` names a renamed note \u{2014} now `new`")),
        "{:?}",
        report.issues
    );
}

#[test]
fn a_pair_rename_prunes_the_old_folder_and_a_failed_one_the_new() {
    let dirs = |roots: &[&str]| -> Vec<VaultPath> { roots.iter().map(|d| vp(d)).collect() };
    let hooks = Arc::new(HookStore {
        dirs: dirs(&["portfolios/old/2026-10-13-scan"]),
        ..HookStore::new()
    });
    let store: Arc<dyn VaultStore> = hooks.clone();
    let p = pair_vault_over(store);
    p.vault
        .rename_note("questions/research/old", "new", None, None, at(), || Ok(()))
        .expect("a failed prune does not fail the rename");
    assert_eq!(
        *hooks.pruned.lock().unwrap(),
        vec![vp("portfolios/old/2026-10-13-scan"), vp("portfolios/old")],
        "the old portfolio's folders, deepest first, and nothing else"
    );

    // A failing `Move` rolls back, then prunes what the attempt created
    // under the new folder.
    let hooks = Arc::new(HookStore {
        dirs: dirs(&["portfolios/new/2026-10-13-scan"]),
        fail_moves: true,
        ..HookStore::new()
    });
    let store: Arc<dyn VaultStore> = hooks.clone();
    let p = pair_vault_over(store);
    p.vault
        .rename_note("questions/research/old", "new", None, None, at(), || Ok(()))
        .unwrap_err();
    assert_eq!(
        *hooks.pruned.lock().unwrap(),
        vec![vp("portfolios/new/2026-10-13-scan"), vp("portfolios/new")],
    );
    assert!(p.store.exists(&vp("questions/research/old.md")).unwrap());
}

// ---------------------------------------------------------------------
// What a surface shows: the plan's log entry, and which notes it offers.
// ---------------------------------------------------------------------

#[test]
fn the_plan_carries_its_log_line() {
    let f = fixture();
    let plan = f
        .vault
        .plan_rename(&f.snapshot(), "old", "new", Some("a clearer name"), at())
        .expect("plan");
    // The entry the planner appended, without the `- **HH:MM**:` stamp.
    assert_eq!(
        plan.log_line.as_deref(),
        Some(
            format_renamed_log_entry("new", "Surrogate model", "old", Some("a clearer name"))
                .as_str()
        )
    );
    let daily = plan
        .edits
        .iter()
        .find(|e| e.path == vp(DAILY_TODAY))
        .expect("today's note");
    assert!(
        daily
            .after
            .contains(&format!("**10:30**: {}", plan.log_line.as_ref().unwrap())),
        "the entry is what lands in the daily note"
    );
    assert!(!plan.log_line.as_ref().unwrap().contains("**"));

    // Nothing is appended for a rename to the current slug.
    assert_eq!(f.plan("old", "old").unwrap().log_line, None);
}

#[test]
fn is_renamable_matches_the_planners_refusals() {
    let f = fixture();
    f.write(
        "questions/research/how-large-is-the-error.md",
        "---\ntype: question\nstatus: open\ncreated: 2026-05-01\n---\n\n# How large is the error?\n",
    );
    for (path, note_type) in [
        ("projects/old.md", "project"),
        ("actions/fit-the-baseline.md", "action"),
        ("questions/research/how-large-is-the-error.md", "question"),
        ("portfolios/surrogate-error-budget/_index.md", "portfolio"),
    ] {
        assert!(f.vault.is_renamable(&vp(path), note_type), "{path}");
        f.plan(path.trim_end_matches(".md"), "brand-new")
            .unwrap_or_else(|e| panic!("the planner refuses {path}: {e:?}"));
    }
    for (path, note_type, reason) in [
        (DAILY_TODAY, "daily", "type daily"),
        (ARCHIVED, "action", "archived"),
    ] {
        assert!(!f.vault.is_renamable(&vp(path), note_type), "{path}");
        let err = f
            .plan(path.trim_end_matches(".md"), "brand-new")
            .unwrap_err();
        assert!(
            matches!(&err, DomainError::NoteNotRenamable { reason: r, .. } if r == reason),
            "{err:?}"
        );
    }

    // A portfolio whose slug a question shares is renamed through the question.
    let p = pair_vault();
    assert!(
        !p.vault
            .is_renamable(&vp("portfolios/old/_index.md"), "portfolio")
    );
    assert!(
        p.vault
            .is_renamable(&vp("questions/research/old.md"), "question")
    );
}

#[test]
fn is_renamable_refuses_a_custom_types_folder_note() {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        cdno_core::config::CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let body = "---\ntype: concept\ncreated: 2026-05-01\n---\n\n# Surrogate model\n";
    store
        .write_file(&vp("concepts/old/_index.md"), body)
        .unwrap();
    store.write_file(&vp("concepts/flat.md"), body).unwrap();
    let vault = Vault::new(store, Arc::new(MemoryIndex::new()), config)
        .expect("vault opens")
        .0;
    assert!(vault.is_renamable(&vp("concepts/flat.md"), "concept"));
    assert!(!vault.is_renamable(&vp("concepts/old/_index.md"), "concept"));
}

#[test]
fn resolve_rename_target_narrows_to_the_pairs_question() {
    use cdno_domain::frontmatter::QuestionDomain;
    let f = fixture();
    let today = at().date();

    // A question and its same-slug portfolio share a stem: only the
    // question is renamable, so it is the answer, and the flag says the
    // domain chose.
    f.vault
        .create_question_with_vars(
            at(),
            QuestionDomain::Research,
            "Does a surrogate beat the simulator?",
            &Default::default(),
            Some("surrogate-accuracy"),
        )
        .unwrap();
    f.vault
        .create_portfolio(at(), "surrogate-accuracy", None)
        .unwrap();
    assert!(matches!(
        f.vault
            .resolve_note_ref("surrogate-accuracy", today)
            .unwrap(),
        RefResolution::Ambiguous(_)
    ));
    let (resolution, chosen) = f
        .vault
        .resolve_rename_target("surrogate-accuracy", today)
        .unwrap();
    assert!(chosen);
    match resolution {
        RefResolution::Resolved(note) => {
            assert_eq!(note.path, vp("questions/research/surrogate-accuracy.md"));
        }
        other => panic!("{other:?}"),
    }

    // Two renamable notes with one stem stay an ambiguity.
    f.write(
        "actions/old.md",
        "---\ntype: action\nstatus: active\nproject: old\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Old\n",
    );
    let f = {
        drop(f.vault);
        Fixture {
            vault: open(Arc::clone(&f.store), f.index.clone()),
            ..f
        }
    };
    let (resolution, chosen) = f.vault.resolve_rename_target("old", today).unwrap();
    assert!(!chosen);
    assert!(
        matches!(resolution, RefResolution::Ambiguous(ref hits) if hits.len() == 2),
        "{resolution:?}"
    );

    // A single exact hit is not a choice the domain made.
    let (resolution, chosen) = f
        .vault
        .resolve_rename_target("projects/emulator", today)
        .unwrap();
    assert!(!chosen);
    assert!(
        matches!(resolution, RefResolution::Resolved(_)),
        "{resolution:?}"
    );
}

// ---------------------------------------------------------------------
// The relink (T30): the planner in the mode where the note does not move.
// ---------------------------------------------------------------------

/// Project `new` ("Surrogate model"), renamed from `old` earlier: it lists
/// `old` as an alias.
const RELINKED_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\naliases:\n  - old\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n";

/// Today's daily note after a `merge=union` sync (RFC 0007 §5.8 (4)): the
/// rename rewrote the line to `[[new]]` on one clone, the other clone
/// appended beside it, and the union kept both sides, so the old line came
/// back holding the old slug.
const UNION_DAY: &str = "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **08:00**: back on [[old]]\n- **08:00**: back on [[new]]\n";

fn relink_vault(notes: &[(&str, &str)]) -> Vault {
    vault_over(Arc::new(MemoryVaultStore::new()), notes, &[])
}

fn relink_plan(vault: &Vault, target: &str) -> RenamePlan {
    let snapshot = vault.rename_snapshot().unwrap();
    vault
        .plan_relink(&snapshot, target, at())
        .expect("relink plan")
}

/// Preview the relink, then write that plan, as a surface does.
fn relink(vault: &Vault, target: &str) -> RenameOutcome {
    let plan = vault.relink_preview(target, at()).expect("preview");
    vault
        .relink_note(target, Some(&plan.hash), at(), || Ok(()))
        .expect("relink")
}

#[test]
fn relink_rewrites_only_alias_resolved_references() {
    let vault = relink_vault(&[
        ("projects/new.md", RELINKED_PROJECT),
        (DAILY_TODAY, UNION_DAY),
    ]);
    let plan = relink_plan(&vault, "new");
    assert!(plan.relink);
    assert_eq!(plan.new_slug, "new");
    assert_eq!(
        changes_by_path(&plan),
        BTreeMap::from([(
            DAILY_TODAY.to_owned(),
            vec![Change::Address { count: 1 }, Change::LogLine]
        )]),
        "exactly one address changes: the `[[old]]` line; `[[new]]` resolves by stem"
    );
    assert_eq!(plan.reference_totals(), (1, 1));
    let day = &plan.edits[0];
    let address_edits: Vec<_> = day
        .edits
        .iter()
        .filter(|e| !e.replacement.contains("relinked"))
        .collect();
    assert_eq!(address_edits.len(), 1, "{:?}", day.edits);
    assert_eq!(&day.before[address_edits[0].range.clone()], "old");
    assert_eq!(address_edits[0].replacement, "new");
    assert!(
        day.after
            .contains("- **08:00**: back on [[new]]\n- **08:00**: back on [[new]]\n")
    );
    // A relink never plans a no-op: no edit leaves its bytes as they were,
    // so nothing needs filtering out.
    for edit in &plan.edits {
        assert_ne!(edit.before, edit.after, "{}", edit.path);
        for e in &edit.edits {
            assert_ne!(
                &edit.before[e.range.clone()],
                e.replacement,
                "{}",
                edit.path
            );
        }
    }
}

#[test]
fn relink_leaves_a_link_that_resolves_by_stem_even_when_listed_as_an_alias() {
    // A hand edit lists the note's own slug among its aliases: `[[new]]`
    // still resolves by stem, so it is no straggler and is not counted.
    let project = RELINKED_PROJECT.replace("  - old\n", "  - old\n  - new\n");
    let vault = relink_vault(&[("projects/new.md", &project), (DAILY_TODAY, UNION_DAY)]);
    let plan = relink_plan(&vault, "new");
    assert_eq!(plan.reference_totals(), (1, 1));
}

#[test]
fn relink_leaves_a_link_the_stem_sends_elsewhere_alone() {
    // `older` is also an alias of `new`, but a note has that stem now, so
    // `[[older]]` names that note: rewriting it would change its referent.
    let project = RELINKED_PROJECT.replace("  - old\n", "  - old\n  - older\n");
    let other = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Older\n\n## Current State\n\n## Next Actions\n\n## Milestones\n";
    let day = UNION_DAY.replace("back on [[new]]", "and [[older]]");
    let vault = relink_vault(&[
        ("projects/new.md", &project),
        ("projects/older.md", other),
        (DAILY_TODAY, &day),
    ]);
    let plan = relink_plan(&vault, "new");
    assert_eq!(plan.reference_totals(), (1, 1));
    assert!(plan.edits[0].after.contains("and [[older]]"));
    assert!(plan.edits[0].after.contains("back on [[new]]"));
}

#[test]
fn relink_rewrites_a_portfolio_straggler_to_its_folder_form() {
    // A folder note answers to its folder path; a link that already
    // reaches it by path is not a straggler, so it is left as written.
    let index = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\naliases:\n  - old\n---\n\n# How large is the error?\n";
    let day = UNION_DAY.replace("[[new]]", "[[portfolios/new/_index]]");
    let vault = relink_vault(&[("portfolios/new/_index.md", index), (DAILY_TODAY, &day)]);
    let plan = relink_plan(&vault, "portfolios/new");
    assert_eq!(plan.reference_totals(), (1, 1));
    assert!(plan.moves.is_empty());
    let after = &plan.edits[0].after;
    assert!(after.contains("back on [[portfolios/new]]\n"), "{after}");
    assert!(
        after.contains("back on [[portfolios/new/_index]]\n"),
        "{after}"
    );
    assert!(after.contains("relinked [[portfolios/new]] \u{2014} How large is the error?"));
}

#[test]
fn relink_leaves_a_folder_straggler_whose_folder_form_points_elsewhere() {
    // #913: after a pair rename, `[[portfolios/old]]` resolves through the
    // question's alias. Its own folder form, `[[portfolios/new]]`, names the
    // portfolio: a different note, so the relink leaves it.
    let p = pair_vault();
    p.vault
        .rename_note("questions/research/old", "new", None, None, at(), || Ok(()))
        .expect("rename");
    p.store
        .write_file(
            &vp("journal/2026/daily/2026-05-11.md"),
            "---\ntype: daily\ndate: 2026-05-11\n---\n\n# Monday, 11 May 2026\n\n## Logs\n- **09:00**: filed in [[portfolios/old]]\n",
        )
        .unwrap();
    let plan = p
        .vault
        .relink_preview("questions/research/new", at())
        .unwrap();
    assert!(plan.is_empty(), "{:?}", changes_by_path(&plan));
}

#[test]
fn relink_with_no_stragglers_is_an_empty_plan() {
    let day = UNION_DAY.replace("back on [[old]]", "back on [[new]] again");
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[("projects/new.md", RELINKED_PROJECT), (DAILY_TODAY, &day)],
        &[],
    );
    let plan = relink_plan(&vault, "new");
    assert!(plan.is_empty());
    assert!(plan.relink);
    assert_eq!(plan.log_line, None, "nothing relinked, nothing logged");

    let before = files_of(&*store);
    let outcome = relink(&vault, "new");
    assert!(outcome.written.is_empty());
    assert_eq!(files_of(&*store), before);
}

#[test]
fn relink_never_moves_or_extends_aliases() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/new.md", RELINKED_PROJECT),
            (DAILY_TODAY, UNION_DAY),
        ],
        &[],
    );
    let outcome = relink(&vault, "new");
    assert!(outcome.plan.moves.is_empty());
    assert_eq!(outcome.written, vec![vp(DAILY_TODAY)]);
    assert!(
        outcome
            .plan
            .edits
            .iter()
            .all(|e| !e.changes.contains(&Change::Alias))
    );
    // D5: the note is where it was, byte for byte, its aliases still listed.
    assert_eq!(
        store.read_file(&vp("projects/new.md")).unwrap(),
        RELINKED_PROJECT
    );
    assert!(!store.exists(&vp("projects/old.md")).unwrap());
}

#[test]
fn relink_logs_the_count_and_no_was() {
    let emulator = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-02\n---\n\n# Emulator\n\n## Current State\nWaiting on [[old#Current State]].\n\n## Next Actions\n\n## Milestones\n";
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/new.md", RELINKED_PROJECT),
            ("projects/emulator.md", emulator),
            (DAILY_TODAY, UNION_DAY),
        ],
        &[],
    );
    let outcome = relink(&vault, "new");
    let entry = "relinked [[new]] \u{2014} Surrogate model\n  references: 2";
    assert_eq!(outcome.plan.log_line.as_deref(), Some(entry));
    assert_eq!(
        format_relinked_log_entry("new", "Surrogate model", 2),
        entry
    );
    let day = store.read_file(&vp(DAILY_TODAY)).unwrap();
    assert!(day.ends_with(&format!("- **10:30**: {entry}\n")), "{day}");
    assert!(!day.contains("was:"), "{day}");
    assert!(
        store
            .read_file(&vp("projects/emulator.md"))
            .unwrap()
            .contains("Waiting on [[new#Current State]].")
    );
    // The focus fold never reads the line.
    assert_eq!(vault.current_focus(at().date()).unwrap(), None);
}

#[test]
fn relink_rewrites_a_field_holding_an_alias() {
    // A field is read by its bare value, so `project: old` was detached
    // from the note; the relink reattaches it.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("projects/new.md", RELINKED_PROJECT),
            ("actions/fit-the-baseline.md", ACTION),
        ],
        &[],
    );
    let plan = relink_plan(&vault, "new");
    assert_eq!(
        changes_by_path(&plan).get("actions/fit-the-baseline.md"),
        Some(&vec![field("project")])
    );
    relink(&vault, "new");
    let after = store.read_file(&vp("actions/fit-the-baseline.md")).unwrap();
    assert!(after.contains("\nproject: new\n"), "{after}");
    // The tag in the body is not a reference; it is left as written.
    assert!(after.contains("#action/old"), "{after}");
}

#[test]
fn relink_with_to_is_refused() {
    use cdno_domain::vault::RenameRequest;
    let err = RenameRequest::from_args(Some("emulator".to_owned()), true).unwrap_err();
    assert!(
        matches!(&err, DomainError::RelinkTakesNoSlug { to } if to == "emulator"),
        "{err:?}"
    );
    assert_eq!(
        RenameRequest::from_args(None, true).unwrap(),
        Some(RenameRequest::Relink)
    );
    assert_eq!(
        RenameRequest::from_args(Some("emulator".to_owned()), false).unwrap(),
        Some(RenameRequest::To("emulator".to_owned()))
    );
    assert_eq!(RenameRequest::from_args(None, false).unwrap(), None);
}

#[test]
fn a_relinked_straggler_pause_line_is_reachable_by_the_new_slug() {
    // The issue's case (review of #893): a hand-written pause naming the
    // old slug is keyed by the log's text, so `resume_action(Some("new"))`
    // cannot find it until the relink rewrites it.
    let project = RELINKED_PROJECT.replace(
        "## Next Actions\n",
        "## Next Actions\n- [ ] Fit the baseline (deep)\n",
    );
    let day = "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **09:00**: action paused on [[old]] \u{2014} Fit the baseline\n";
    let vault = relink_vault(&[("projects/new.md", &project), (DAILY_TODAY, day)]);
    assert!(
        vault.resume_action(dt(10, 10, 0), Some("new")).is_err(),
        "before the relink the pause is out of reach by the new slug"
    );
    relink(&vault, "new");
    let resumed = vault
        .resume_action(dt(10, 11, 0), Some("new"))
        .expect("the relinked pause resumes by the new slug");
    assert_eq!(resumed.resumed.project, "new");
}

#[test]
fn a_folder_note_listing_its_own_name_relinks_to_an_empty_plan() {
    // Review of #921: a portfolio's stem is `_index`, so its own folder
    // name listed as an alias reaches it through the alias rule. It is
    // still its current name: `portfolio: new` already names it, and there
    // is no straggler to rewrite.
    let index = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\naliases:\n  - new\n---\n\n# How large is the error?\n";
    let evidence = "---\ntype: evidence\ncreated: 2026-05-02\nsource: \"Baseline sweep\"\nportfolio: new\n---\n\n# Baseline sweep\n\nFiled in [[portfolios/new]].\n";
    let day = "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **08:00**: filed in [[portfolios/new]]\n";
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let vault = vault_over(
        Arc::clone(&store),
        &[
            ("portfolios/new/_index.md", index),
            ("portfolios/new/2026-05-02-baseline.md", evidence),
            (DAILY_TODAY, day),
        ],
        &[],
    );
    let before = files_of(&*store);
    for _ in 0..2 {
        let plan = relink_plan(&vault, "portfolios/new");
        assert!(plan.is_empty(), "{:?}", changes_by_path(&plan));
        assert_eq!(plan.log_line, None);
        let outcome = relink(&vault, "portfolios/new");
        assert!(outcome.written.is_empty());
        assert_eq!(files_of(&*store), before, "nothing written, nothing logged");
    }
}

#[test]
fn an_archived_action_can_be_the_target_of_a_relink() {
    // Review of #921: the two-clone sequence for an action. Renamed while
    // active, completed, and a straggler `[[old-act]]` arrives afterwards.
    // The relink changes nothing on the archived note, so it is not refused.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index = Arc::new(MemoryIndex::new());
    store
        .write_file(&vp("projects/surrogate-model.md"), PROJECT)
        .unwrap();
    let vault = open(Arc::clone(&store), index.clone());
    vault
        .add_action_with_note(dt(7, 9, 0), "surrogate-model", "Old act", EnergyLevel::Deep)
        .expect("add action");
    vault
        .rename_note(
            "actions/old-act",
            "new-act",
            None,
            None,
            dt(7, 9, 30),
            || Ok(()),
        )
        .expect("rename the active action");
    vault
        .complete_action(dt(7, 10, 0), "surrogate-model", "new-act")
        .expect("complete action");
    let archived = vp("actions/_done/2026/new-act.md");
    assert!(store.exists(&archived).unwrap());
    let row = index.find_archival_snapshot(&archived).unwrap();
    assert!(row.is_some(), "archival stamps a snapshot");
    let archived_before = store.read_file(&archived).unwrap();

    // Renaming it is still refused.
    let err = vault
        .rename_preview("new-act", "newer-act", None, at())
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "archived"),
        "{err:?}"
    );

    store
        .write_file(
            &vp(DAILY_TODAY),
            "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **08:00**: thought on [[old-act]]\n",
        )
        .unwrap();
    let outcome = relink(&vault, "new-act");
    assert_eq!(outcome.plan.target.path, archived);
    assert_eq!(outcome.plan.reference_totals(), (1, 1));
    assert_eq!(outcome.written, vec![vp(DAILY_TODAY)]);
    let day = store.read_file(&vp(DAILY_TODAY)).unwrap();
    assert!(day.contains("thought on [[new-act]]"), "{day}");
    assert!(day.contains("relinked [[actions/new-act]]"), "{day}");
    assert_eq!(store.read_file(&archived).unwrap(), archived_before);
    assert_eq!(index.find_archival_snapshot(&archived).unwrap(), row);

    let report = vault.lint_all_notes(super::support::lint_day()).unwrap();
    assert!(
        report
            .issues
            .iter()
            .all(|i| i.severity != cdno_domain::LintSeverity::Error
                && !i.message.contains("names a renamed note")
                && !i.message.contains("relinked")),
        "{:?}",
        report.issues
    );
}

#[test]
fn a_bare_link_to_a_folder_notes_own_name_is_relinked_to_its_folder_form() {
    // The link half of the same shape: `[[new]]` reaches the portfolio
    // through the alias rule (its stem is `_index`; the alias outranks the
    // bare folder-name rule), lint reports it, and
    // its folder form is a real change. The `portfolio: new` field is left.
    let index = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\naliases:\n  - new\n---\n\n# How large is the error?\n";
    let evidence = "---\ntype: evidence\ncreated: 2026-05-02\nsource: \"Baseline sweep\"\nportfolio: new\n---\n\n# Baseline sweep\n";
    let day = "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **08:00**: filed in [[new]]\n";
    let vault = relink_vault(&[
        ("portfolios/new/_index.md", index),
        ("portfolios/new/2026-05-02-baseline.md", evidence),
        (DAILY_TODAY, day),
    ]);
    let plan = relink_plan(&vault, "portfolios/new");
    assert_eq!(
        changes_by_path(&plan),
        BTreeMap::from([(
            DAILY_TODAY.to_owned(),
            vec![Change::Address { count: 1 }, Change::LogLine]
        )])
    );
    assert!(plan.edits[0].after.contains("filed in [[portfolios/new]]"));
}

#[test]
fn a_portfolio_rename_rewrites_a_bare_link_to_the_folder_note() {
    // #538: a bare `[[old]]` resolves to the portfolio, so a rename
    // corrects it like any other reference (to the folder form).
    let mut files = portfolio_files(P_PDF, "scan.pdf");
    files.push((
        "journal/2026/daily/2026-05-10.md".into(),
        "---\ntype: daily\ndate: 2026-05-10\n---\n\n# Sunday, 10 May 2026\n\n## Logs\n- **09:00**: filed in [[old]]\n"
            .into(),
    ));
    let p = portfolio_vault_over(Arc::new(MemoryVaultStore::new()), &files);
    let preview = p
        .vault
        .rename_preview("portfolio:old", "new", None, at())
        .expect("preview");
    p.vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            Some(&preview.hash),
            at(),
            || Ok(()),
        )
        .expect("rename");
    assert!(
        p.read("journal/2026/daily/2026-05-10.md")
            .contains("filed in [[portfolios/new]]"),
        "{}",
        p.read("journal/2026/daily/2026-05-10.md")
    );
}

/// The snapshot goes through the classifier, so a reference held in an
/// upper-case `.MD` note is rewritten like any other.
#[test]
fn a_reference_held_in_an_upper_case_md_note_is_rewritten() {
    let f = fixture();
    f.write("concepts/NOTE.MD", "# Note\n\nSee [[old]].\n");
    let plan = f.plan("old", "new").expect("plan");
    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp("concepts/NOTE.MD"))
        .expect("the upper-case note is planned");
    assert!(edit.after.contains("[[new]]"), "after: {}", edit.after);
}

/// A `.typ` attachment is not read as a note by the snapshot.
#[test]
fn a_typst_file_is_not_in_the_snapshot() {
    let f = fixture();
    f.write("concepts/draft.typ", "See [[old]].\n");
    let snapshot = f.snapshot();
    assert!(!snapshot.paths.contains(&vp("concepts/draft.typ")));
    assert!(snapshot.files.contains(&vp("concepts/draft.typ")));
}

/// A loose `.typ` that is not UTF-8 is no note, so the snapshot a rename reads
/// never tries to decode it.
#[test]
fn a_non_utf8_loose_typ_does_not_fail_a_rename() {
    let dir = tempfile::TempDir::new().unwrap();
    let store: Arc<dyn VaultStore> = Arc::new(cdno_core::store::FsVaultStore::new(dir.path()));
    store
        .write_file(
            &VaultPath::new("projects/surrogate-model.md").unwrap(),
            "---\ntype: project\ntitle: Surrogate model\nstatus: active\n---\n# Surrogate model\n",
        )
        .unwrap();
    let loose = dir
        .path()
        .join("portfolios/surrogate-error-budget/loose.typ");
    std::fs::create_dir_all(loose.parent().unwrap()).unwrap();
    std::fs::write(&loose, b"= Loose\n\xff\xfe\n").unwrap();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, report) = Vault::new(store, index, VaultConfig::default()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    vault
        .rename_note(
            "surrogate-model",
            "surrogate-model-v2",
            None,
            None,
            at(),
            || Ok(()),
        )
        .expect("rename");

    assert!(dir.path().join("projects/surrogate-model-v2.md").exists());
}

// ---------------------------------------------------------------------
// A Typst note as a referrer (RFC 0008 T13)
// ---------------------------------------------------------------------

const SURROGATE: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n";

const BUDGET_INDEX: &str = "---\ntype: portfolio\nquestion: \"How large is the error?\"\ncreated: 2026-05-01\nproject: null\n---\n\n# How large is the error?\n";

const TYPST_EVIDENCE_PATH: &str = "portfolios/surrogate-error-budget/2026-11-08-error-budget.typ";

const TYPST_EVIDENCE: &str = "#metadata((
  type: \"evidence\",
  created: \"2026-11-08\",
  source: \"Error budget derivation\",
  portfolio: \"surrogate-error-budget\",
  origin: \"[[projects/surrogate-model]]\",
)) <cdno>

= Setting

The fit lives in [[surrogate-model]] and, under [[surrogate-model\\#Current State|its state]],
is still calibrating. A link in `[[surrogate-model]]` raw text is not one.
";

fn typst_referrer_fixture() -> Fixture {
    let mem = Arc::new(MemoryVaultStore::new());
    let mem_index = Arc::new(MemoryIndex::new());
    let store: Arc<dyn VaultStore> = mem.clone();
    for (path, content) in [
        ("projects/surrogate-model.md", SURROGATE),
        ("portfolios/surrogate-error-budget/_index.md", BUDGET_INDEX),
        (TYPST_EVIDENCE_PATH, TYPST_EVIDENCE),
    ] {
        store.write_file(&vp(path), content).unwrap();
    }
    let index: Arc<dyn VaultIndex> = mem_index.clone();
    let vault = open(Arc::clone(&store), index);
    Fixture {
        store,
        mem,
        index: mem_index,
        vault,
    }
}

#[test]
fn a_typst_referrer_is_read_by_the_snapshot() {
    let f = typst_referrer_fixture();
    let plan = f.plan("surrogate-model", "surrogate-model-v2").unwrap();

    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp(TYPST_EVIDENCE_PATH))
        .expect("the Typst note is a referrer");
    assert_eq!(edit.before, TYPST_EVIDENCE);
    assert_eq!(
        edit.changes,
        vec![Change::Address { count: 3 }],
        "the block string and two markup links; the raw text is not a link"
    );
    assert!(differs_only_by(&edit.before, &edit.after, &edit.edits));
    assert_eq!(
        edit.after,
        TYPST_EVIDENCE
            .replace("[[surrogate-model]] and", "[[surrogate-model-v2]] and")
            .replace(
                "[[surrogate-model\\#Current",
                "[[surrogate-model-v2\\#Current"
            )
            .replace(
                "\"[[projects/surrogate-model]]\"",
                "\"[[projects/surrogate-model-v2]]\""
            ),
    );
}

#[test]
fn a_rename_of_the_project_in_a_typst_origin_rewrites_the_block_string() {
    let f = typst_referrer_fixture();
    let plan = f
        .plan("projects/surrogate-model", "surrogate-model-v2")
        .unwrap();
    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp(TYPST_EVIDENCE_PATH))
        .unwrap();
    assert!(
        edit.after
            .contains("origin: \"[[projects/surrogate-model-v2]]\",\n)) <cdno>"),
        "{}",
        edit.after
    );
    // Only the three addresses moved: every other byte, the block included, is as written.
    assert!(differs_only_by(&edit.before, &edit.after, &edit.edits));
    assert_eq!(edit.edits.len(), 3);
}

#[test]
fn a_typst_note_is_left_alone_when_it_links_nothing_renamed() {
    let f = typst_referrer_fixture();
    f.write("projects/other.md", SURROGATE);
    let plan = f.plan("other", "other-v2").unwrap();
    assert!(
        plan.edits.iter().all(|e| e.path != vp(TYPST_EVIDENCE_PATH)),
        "{:?}",
        plan.edits.iter().map(|e| &e.path).collect::<Vec<_>>()
    );
}

#[test]
fn a_typst_referrer_that_writes_the_slug_only_with_escapes_is_rewritten() {
    let f = typst_referrer_fixture();
    f.write(
        "projects/surrogate_model.md",
        &SURROGATE.replace("Surrogate model", "Surrogate model underscore"),
    );
    f.write("projects/surrogate-model.md", "");
    f.store
        .delete_file(&vp("projects/surrogate-model.md"))
        .unwrap();
    let escaped = "#metadata((\n  type: \"evidence\",\n  created: \"2026-11-08\",\n  source: \"Escaped link\",\n  portfolio: \"surrogate-error-budget\",\n  origin: \"[[projects/emulator]]\",\n)) <cdno>\n\nSee [[surrogate\\_model]].\n";
    let path = "portfolios/surrogate-error-budget/2026-11-09-escaped.typ";
    f.write(path, escaped);
    f.store.delete_file(&vp(TYPST_EVIDENCE_PATH)).unwrap();
    let vault = open(Arc::clone(&f.store), Arc::new(MemoryIndex::new()));
    let plan = vault
        .plan_rename(
            &vault.rename_snapshot().unwrap(),
            "surrogate_model",
            "surrogate-model",
            None,
            at(),
        )
        .unwrap();
    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp(path))
        .expect("the escaped-only referrer is planned");
    assert!(
        edit.after.contains("See [[surrogate-model]]."),
        "{}",
        edit.after
    );
    assert!(differs_only_by(&edit.before, &edit.after, &edit.edits));
}

// ---------------------------------------------------------------------
// A Typst note as the renamed note (RFC 0008 T17)
// ---------------------------------------------------------------------

const TYPST_CONCEPT_PATH: &str = "concepts/bias-variance-split.typ";

const TYPST_CONCEPT: &str = "#metadata((
  type: \"concept\",
  title: \"Bias-variance split\",
  created: \"2026-11-08\",
)) <cdno>

= The split

#image(\"bias-variance-split/figs/a.png\")
#image(\"./bias-variance-split/figs/b.png\")
#bibliography(\"../concepts/bias-variance-split/refs.bib\")
#image(\"bias-variance-split-extra/c.png\")
#image(\"/bias-variance-split/rooted.png\")

Compare [[surrogate-model]].
";

const TYPST_ASSETS: [&str; 3] = ["figs/a.png", "figs/b.png", "refs.bib"];

const CONCEPT_REFERRER_MD: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nSee [[concepts/bias-variance-split]] and [[bias-variance-split]], figure [[concepts/bias-variance-split/figs/a.png]].\n";

const CONCEPT_REFERRER_TYP: &str = "#metadata((
  type: \"evidence\",
  created: \"2026-11-08\",
  source: \"Error budget derivation\",
  portfolio: \"surrogate-error-budget\",
  origin: \"[[concepts/bias-variance-split]]\",
)) <cdno>

Uses [[bias-variance-split]].
";

fn concept_config() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        cdno_core::config::CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    config
}

fn open_concepts(store: Arc<dyn VaultStore>, index: Arc<dyn VaultIndex>) -> Vault {
    Vault::new(store, index, concept_config())
        .expect("vault opens")
        .0
}

/// A Typst concept with three assets, a Markdown referrer and a Typst one.
fn typst_target_fixture() -> Fixture {
    let mem = Arc::new(MemoryVaultStore::new());
    let mem_index = Arc::new(MemoryIndex::new());
    let store: Arc<dyn VaultStore> = mem.clone();
    store
        .write_file(&vp(TYPST_CONCEPT_PATH), TYPST_CONCEPT)
        .unwrap();
    for asset in TYPST_ASSETS {
        store
            .write_file(
                &vp(&format!("concepts/bias-variance-split/{asset}")),
                "bytes",
            )
            .unwrap();
    }
    store
        .write_file(&vp("projects/surrogate-model.md"), CONCEPT_REFERRER_MD)
        .unwrap();
    store
        .write_file(
            &vp("portfolios/surrogate-error-budget/_index.md"),
            BUDGET_INDEX,
        )
        .unwrap();
    store
        .write_file(&vp(TYPST_EVIDENCE_PATH), CONCEPT_REFERRER_TYP)
        .unwrap();
    let index: Arc<dyn VaultIndex> = mem_index.clone();
    let vault = open_concepts(Arc::clone(&store), index);
    Fixture {
        store,
        mem,
        index: mem_index,
        vault,
    }
}

const NEW_CONCEPT: &str = "concepts/error-split.typ";

fn rename_concept(f: &Fixture) -> RenameOutcome {
    f.vault
        .rename_note(
            "bias-variance-split",
            "error-split",
            None,
            None,
            at(),
            || Ok(()),
        )
        .expect("rename")
}

#[test]
fn renaming_a_typst_note_moves_its_assets_folder() {
    let f = typst_target_fixture();
    let plan = f.plan("bias-variance-split", "error-split").unwrap();
    assert_eq!(plan.moves.len(), TYPST_ASSETS.len() + 1);
    assert_eq!(
        plan.moves[0],
        (vp(TYPST_CONCEPT_PATH), vp(NEW_CONCEPT)),
        "the note first"
    );
    assert_eq!(
        plan.log_line.as_deref().map(|l| l.lines().next().unwrap()),
        Some("renamed [[concepts/error-split]] \u{2014} Bias-variance split")
    );

    rename_concept(&f);

    for asset in TYPST_ASSETS {
        assert_eq!(f.read(&format!("concepts/error-split/{asset}")), "bytes");
        assert!(
            !f.store
                .exists(&vp(&format!("concepts/bias-variance-split/{asset}")))
                .unwrap()
        );
    }
    assert!(!f.store.exists(&vp(TYPST_CONCEPT_PATH)).unwrap());
    assert!(
        !f.store
            .walk_dirs(&vp("concepts"))
            .unwrap()
            .contains(&vp("concepts/bias-variance-split")),
        "the old folder is gone"
    );
    // A link into the folder follows, as a portfolio's does.
    assert!(
        f.read("projects/surrogate-model.md")
            .contains("[[concepts/error-split/figs/a.png]]")
    );
}

#[test]
fn renaming_a_typst_note_rewrites_its_own_asset_paths_and_nothing_else() {
    let f = typst_target_fixture();
    let plan = f.plan("bias-variance-split", "error-split").unwrap();
    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp(TYPST_CONCEPT_PATH))
        .expect("the note's own edit");
    assert_eq!(
        edit.changes,
        vec![Change::Alias, Change::AssetPaths { count: 3 }]
    );
    assert_eq!(edit.edits.len(), 4, "one alias edit, three path edits");
    assert!(differs_only_by(&edit.before, &edit.after, &edit.edits));
    assert_eq!(
        edit.after,
        TYPST_CONCEPT
            .replace(
                "#image(\"bias-variance-split/figs",
                "#image(\"error-split/figs"
            )
            .replace(
                "#image(\"./bias-variance-split/figs",
                "#image(\"./error-split/figs"
            )
            .replace(
                "../concepts/bias-variance-split/",
                "../concepts/error-split/"
            )
            .replace(
                "  created: \"2026-11-08\",\n",
                "  created: \"2026-11-08\",\n  aliases: (\"bias-variance-split\",),\n"
            ),
    );
}

#[test]
fn renaming_a_typst_note_appends_the_alias_to_the_block() {
    let f = typst_target_fixture();
    rename_concept(&f);
    assert!(
        f.read(NEW_CONCEPT)
            .contains("aliases: (\"bias-variance-split\",),")
    );
    f.vault
        .rename_note("error-split", "third-name", None, None, at(), || Ok(()))
        .expect("second rename");
    assert!(
        f.read("concepts/third-name.typ")
            .contains("aliases: (\"bias-variance-split\", \"error-split\"),")
    );
    assert_eq!(f.read("concepts/third-name/refs.bib"), "bytes");
}

#[test]
fn a_typst_note_takes_a_former_name_back() {
    let f = typst_target_fixture();
    rename_concept(&f);
    f.vault
        .rename_note(
            "error-split",
            "bias-variance-split",
            None,
            None,
            at(),
            || Ok(()),
        )
        .expect("back");
    let back = f.read(TYPST_CONCEPT_PATH);
    assert!(back.contains("aliases: (\"error-split\",),"), "{back}");
}

#[test]
fn a_markdown_and_a_typst_referrer_are_both_rewritten() {
    let f = typst_target_fixture();
    rename_concept(&f);
    let md = f.read("projects/surrogate-model.md");
    assert!(
        md.contains("[[concepts/error-split]] and [[error-split]]"),
        "{md}"
    );
    let typ = f.read(TYPST_EVIDENCE_PATH);
    assert!(
        typ.contains("origin: \"[[concepts/error-split]]\""),
        "{typ}"
    );
    assert!(typ.contains("Uses [[error-split]]."), "{typ}");
}

#[test]
fn a_failed_asset_move_rolls_the_note_back() {
    let f = typst_target_fixture();
    let before = files_of(&*f.mem);
    let paths_before = f.index.list_all_paths().unwrap();
    // The note's write and delete are the first two ops; the third asset
    // move is the fifth.
    let failing: Arc<dyn VaultStore> = Arc::new(FailingStore::new(Arc::clone(&f.mem), 5));
    let index: Arc<dyn VaultIndex> = f.index.clone();
    let vault = open_concepts(failing, index);
    let err = vault
        .rename_note(
            "bias-variance-split",
            "error-split",
            None,
            None,
            at(),
            || Ok(()),
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::Store(StoreError::PermissionDenied(_))),
        "{err:?}"
    );
    assert_eq!(files_of(&*f.mem), before, "rolled back byte for byte");
    assert_eq!(f.read(TYPST_CONCEPT_PATH), TYPST_CONCEPT);
    assert_eq!(f.index.list_all_paths().unwrap(), paths_before);
}

#[test]
fn a_renamed_typst_note_reindexes_from_scratch_to_the_same_rows() {
    let f = typst_target_fixture();
    rename_concept(&f);
    let fresh: Arc<MemoryIndex> = Arc::new(MemoryIndex::new());
    let as_index: Arc<dyn VaultIndex> = fresh.clone();
    let _rebuilt = open_concepts(Arc::clone(&f.store), as_index);

    let sorted = |mut paths: Vec<VaultPath>| {
        paths.sort();
        paths
    };
    assert_eq!(
        sorted(f.index.list_all_paths().unwrap()),
        sorted(fresh.list_all_paths().unwrap())
    );
    let aliases = |index: &MemoryIndex| {
        let mut rows = index.list_aliases().unwrap();
        rows.sort();
        rows
    };
    assert_eq!(aliases(&f.index), aliases(&fresh));
    assert_eq!(
        sorted(f.index.fts_indexed_paths().unwrap()),
        sorted(fresh.fts_indexed_paths().unwrap())
    );
    for path in f.index.list_all_paths().unwrap() {
        let (a, b) = (
            f.index.find_by_path(&path).unwrap().unwrap(),
            fresh.find_by_path(&path).unwrap().unwrap(),
        );
        assert_eq!(
            (&a.note_type, &a.title, &a.content_hash),
            (&b.note_type, &b.title, &b.content_hash),
            "{path}"
        );
        assert_eq!(
            f.index.find_outgoing_links(&path).unwrap(),
            fresh.find_outgoing_links(&path).unwrap(),
            "{path}"
        );
    }
    assert!(
        f.index
            .find_by_path(&vp("concepts/error-split/figs/a.png"))
            .unwrap()
            .is_none()
    );
}

/// Issue #945's binding comment: the §3.5 field follows through the block.
#[test]
fn a_portfolio_rename_rewrites_a_typst_evidence_portfolio_field() {
    let f = typst_referrer_fixture();
    let new_path = "portfolios/error-budget/2026-11-08-error-budget.typ";
    let plan = f
        .plan("portfolios/surrogate-error-budget", "error-budget")
        .unwrap();
    let edit = plan
        .edits
        .iter()
        .find(|e| e.path == vp(TYPST_EVIDENCE_PATH))
        .expect("the evidence note's field is planned");
    assert_eq!(edit.changes, vec![field("portfolio")]);
    assert!(differs_only_by(&edit.before, &edit.after, &edit.edits));

    f.vault
        .rename_note(
            "portfolios/surrogate-error-budget",
            "error-budget",
            None,
            None,
            at(),
            || Ok(()),
        )
        .expect("rename");
    assert_eq!(
        f.read(new_path),
        TYPST_EVIDENCE.replace(
            "portfolio: \"surrogate-error-budget\"",
            "portfolio: \"error-budget\""
        )
    );
    let contents = f.vault.get_portfolio_contents("error-budget").unwrap();
    assert!(
        format!("{contents:?}").contains("2026-11-08-error-budget"),
        "{contents:?}"
    );
}

#[test]
fn a_typst_rename_refuses_a_destination_folder_holding_files() {
    let f = typst_target_fixture();
    f.write("concepts/error-split/figure.png", "taken");
    let err = f
        .vault
        .rename_note(
            "bias-variance-split",
            "error-split",
            None,
            None,
            at(),
            || Ok(()),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            DomainError::SlugTaken {
                kind: TakenBy::Folder,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(f.read(TYPST_CONCEPT_PATH), TYPST_CONCEPT);
}

#[test]
fn a_typst_evidence_note_keeps_the_markdown_refusal() {
    let f = typst_target_fixture();
    let err = f.plan(TYPST_EVIDENCE_PATH, "renamed-evidence").unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRenamable { reason, .. } if reason == "type evidence"),
        "{err:?}"
    );
}
