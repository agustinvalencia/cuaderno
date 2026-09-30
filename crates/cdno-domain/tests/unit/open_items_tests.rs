//! Tests for `Vault::open_items_report` and `OpenItemsReport` (RFC 0004
//! §5.3, plan section T8): the report is parsed from the map, never from
//! the index, and its hash identifies exactly the list it describes.

use std::sync::Arc;

use chrono::NaiveDate;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::frontmatter::ActionStatus;
use cdno_domain::{LinkedCommitment, OpenAction, OpenItemsReport, OpenMilestone, Vault};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn day(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

/// A vault seeded with `notes`, reconciled at construction, plus the
/// store and index handles so a test can write behind the index's back.
fn vault_with(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        VaultConfig::default(),
    )
    .expect("Vault::new");
    (vault, store, index)
}

fn map(status: &str, next_actions: &str, milestones: &str) -> String {
    format!(
        "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-04-01\ncore_question: null\nclosed: null\n---\n\n# Surrogate model\n\n## Current State\nWriting up.\n\n## Next Actions\n{next_actions}\n## Milestones\n{milestones}"
    )
}

const ACTIONS: &str = "- [ ] Run feature set B (deep)\n- [ ] [[actions/characterise-kan]] (deep)\n- [x] Old done bullet (light)\n";
const MILESTONES: &str =
    "- [ ] ICML paper submitted \u{2014} hard: 2026-05-22\n- [x] Kickoff \u{2014} 2026-04-02\n";

fn action_note(status: &str) -> String {
    format!(
        "---\ntype: action\nstatus: {status}\nproject: surrogate-model\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2026-05-20\ncompleted: null\nblocker: null\ncriteria: null\ntags: []\n---\n\n# Characterise KAN\n"
    )
}

fn commitment(status: &str, due: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: {status}\ndue: {due}\ncreated: 2026-05-01\ncompleted: null\ncontext: work\nproject: surrogate-model\n---\n\n# A promise\n"
    )
}

/// The report the fixtures above must yield, whatever folder the map is in.
fn expected() -> OpenItemsReport {
    OpenItemsReport {
        actions: vec![
            OpenAction {
                text: "Run feature set B (deep)".into(),
                note: None,
                note_status: None,
            },
            OpenAction {
                text: "[[actions/characterise-kan]] (deep)".into(),
                note: Some("characterise-kan".into()),
                note_status: Some(ActionStatus::Active),
            },
        ],
        milestones: vec![OpenMilestone {
            title: "ICML paper submitted".into(),
            date: Some(day(5, 22)),
            hard: true,
        }],
        untouched_commitments: vec![LinkedCommitment {
            slug: "reviewer-report".into(),
            due: day(10, 15),
        }],
    }
}

fn report_for(vault: &Vault, raw: &str) -> OpenItemsReport {
    let doc = MarkdownDocument::parse(raw.to_owned()).expect("map parses");
    vault
        .open_items_report(&doc, "surrogate-model")
        .expect("report")
}

#[test]
fn report_lists_open_bullets_open_milestones_and_active_linked_commitments() {
    // Excluded on purpose: the `- [x]` bullet, the `- [x]` milestone and
    // the dropped commitment.
    let raw = map("active", ACTIONS, MILESTONES);
    let (vault, _store, _index) = vault_with(&[
        ("projects/surrogate-model.md", &raw),
        ("actions/characterise-kan.md", &action_note("active")),
        (
            "commitments/reviewer-report.md",
            &commitment("active", "2026-10-15"),
        ),
        (
            "commitments/old-promise.md",
            &commitment("dropped", "2026-10-01"),
        ),
    ]);

    let report = report_for(&vault, &raw);

    assert_eq!(report, expected());
    assert!(!report.is_empty());
    assert_eq!(report.open_count(), 3);
}

#[test]
fn report_on_a_parked_map_without_index_rows_matches_the_active_one() {
    // The parked map is written after the vault was built, so reconcile
    // never indexed it: no note row, no milestone rows. That is the state
    // RFC §3.3 G4 leaves a parked map in, and the reason the report must
    // read the document, not `open_milestones`.
    let (vault, store, index) = vault_with(&[
        ("actions/characterise-kan.md", &action_note("active")),
        (
            "commitments/reviewer-report.md",
            &commitment("active", "2026-10-15"),
        ),
    ]);
    let parked = vp("projects/_parked/surrogate-model.md");
    let raw = map("parked", ACTIONS, MILESTONES);
    store.write_file(&parked, &raw).unwrap();
    assert!(
        index.milestones_for_project(&parked).unwrap().is_empty(),
        "the fixture must have no milestone rows"
    );

    assert_eq!(report_for(&vault, &raw), expected());
}

#[test]
fn an_attached_note_missing_on_disk_has_no_status() {
    let raw = map("active", "- [ ] [[actions/gone]] (light)\n", "");
    let (vault, _store, _index) = vault_with(&[("projects/surrogate-model.md", &raw)]);

    let report = report_for(&vault, &raw);

    assert_eq!(
        report.actions,
        vec![OpenAction {
            text: "[[actions/gone]] (light)".into(),
            note: Some("gone".into()),
            note_status: None,
        }]
    );
}

#[test]
fn an_attached_note_reports_its_own_status() {
    // A bullet left open over a note already completed elsewhere: the
    // report says so, so the cascade can skip restamping it.
    let raw = map("active", "- [ ] [[actions/characterise-kan]] (deep)\n", "");
    let (vault, _store, _index) = vault_with(&[
        ("projects/surrogate-model.md", &raw),
        ("actions/characterise-kan.md", &action_note("completed")),
    ]);

    let report = report_for(&vault, &raw);

    assert_eq!(report.actions[0].note_status, Some(ActionStatus::Completed));
}

#[test]
fn soft_and_undated_open_milestones_are_listed_with_hard_false() {
    let raw = map(
        "active",
        "",
        "- [ ] Draft \u{2014} target: 2026-06-01\n- [ ] Someday\n",
    );
    let (vault, _store, _index) = vault_with(&[("projects/surrogate-model.md", &raw)]);

    let report = report_for(&vault, &raw);

    assert_eq!(
        report.milestones,
        vec![
            OpenMilestone {
                title: "Draft".into(),
                date: Some(day(6, 1)),
                hard: false,
            },
            OpenMilestone {
                title: "Someday".into(),
                date: None,
                hard: false,
            },
        ]
    );
}

#[test]
fn linked_commitments_alone_do_not_block_a_close() {
    // RFC §1.1: the project closes with its reviewer report still open.
    let raw = map(
        "active",
        "- [x] Done (light)\n",
        "- [x] Kickoff \u{2014} 2026-04-02\n",
    );
    let (vault, _store, _index) = vault_with(&[
        ("projects/surrogate-model.md", &raw),
        (
            "commitments/reviewer-report.md",
            &commitment("active", "2026-10-15"),
        ),
    ]);

    let report = report_for(&vault, &raw);

    assert_eq!(report.untouched_commitments.len(), 1);
    assert!(report.is_empty());
    assert_eq!(report.open_count(), 0);
}

#[test]
fn a_map_without_the_sections_has_nothing_open() {
    let raw =
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Bare\n";
    let (vault, _store, _index) = vault_with(&[("projects/surrogate-model.md", raw)]);

    assert!(report_for(&vault, raw).is_empty());
}

#[test]
fn the_hash_is_stable_and_changes_with_any_change_to_the_list() {
    let base = expected();
    assert_eq!(
        base.hash(),
        expected().hash(),
        "identical reports hash alike"
    );

    let mut added = expected();
    added.actions.push(OpenAction {
        text: "A new bullet (light)".into(),
        note: None,
        note_status: None,
    });

    let mut removed = expected();
    removed.actions.remove(0);

    let mut reordered = expected();
    reordered.actions.reverse();

    let mut edited = expected();
    edited.milestones[0].hard = false;

    let mut new_commitment = expected();
    new_commitment.untouched_commitments.push(LinkedCommitment {
        slug: "another".into(),
        due: day(11, 1),
    });

    for (label, other) in [
        ("a bullet added", added),
        ("a bullet removed", removed),
        ("bullets reordered", reordered),
        ("a milestone edited", edited),
        ("a commitment linked", new_commitment),
    ] {
        assert_ne!(base.hash(), other.hash(), "{label} must change the hash");
    }
}
