//! `Vault::plan_rename` (RFC 0007 §5.2–§5.4, §6.2; T18): the pure planner
//! over a store snapshot. Every probe plans against a snapshot read from a
//! `MemoryVaultStore`; nothing here writes through the vault.

use std::collections::BTreeMap;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::extractors::{differs_only_by, wikilink_spans};
use cdno_core::frontmatter::split_frontmatter;
use cdno_core::frontmatter_edit::frontmatter_scalar_spans;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::vault::{
    Change, RenamePlan, RenameWarning, VaultSnapshot, format_renamed_log_entry,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n";

const ACTION: &str = "---\ntype: action\nstatus: active\nproject: old\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Fit the baseline\n\n## Notes\nTagged #action/old in the log.\n";

const COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-06-01\ncreated: 2026-05-01\ncontext: work\nproject: \"old\"\nstewardship: null\n---\n\n# Send the error budget\n";

const INBOX: &str =
    "---\ntype: inbox\ncreated: 2026-05-08\ncaptured_during: old\n---\n\nTry a smaller kernel.\n";

const EMULATOR: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-02\n---\n\n# Emulator\n\n## Current State\nWaiting on [[old]].\n\n## Next Actions\n";

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
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store.write_file(&vp("projects/old.md"), PROJECT).unwrap();
    let vault = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        VaultConfig::default(),
    )
    .expect("vault opens")
    .0;
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
    let fixture = Fixture { store, vault };
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
        fixture.write(path, content);
    }
    fixture
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
        ("projects/emulator.md", vec![Change::Address { count: 1 }]),
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
    // The parser reads `project` as `old`, but the value is not on the key's
    // line, so the text rewrite finds nothing to rewrite.
    f.write(
        "actions/fit-the-baseline.md",
        &ACTION.replace("project: old\n", "project:\n  old\n"),
    );
    let err = f.plan("old", "new").unwrap_err();
    assert!(
        matches!(&err, DomainError::ReferenceUnaddressable { path, key }
            if *path == vp("actions/fit-the-baseline.md") && key == "project"),
        "{err:?}"
    );
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
