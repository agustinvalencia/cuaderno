//! `get_project_impl` against the Memory doubles — the composed
//! Project Detail view-model, no Tauri runtime involved. Covers the
//! rich active project and the parked-project tolerance rule.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{Context, ProjectStatus, Vault};
use cdno_tauri::commands::projects::get_project_impl;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

// An active project with actions, a hard milestone, waiting-on items,
// and a core question — the rich fixture the detail page renders.
const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\ncore_question: \"[[questions/research/alpha-q]]\"\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] File receipts (light)\n\n## Waiting On\n- Compute allocation\n\n## Milestones\n- [ ] Submit abstract \u{2014} hard: 2026-05-27\n- [x] Kickoff \u{2014} 2026-04-02\n";

// A question that backlinks Alpha via a body section — a body-level
// wikilink the index picks up (frontmatter links aren't indexed).
const QUESTION: &str = "---\ntype: question\nstatus: open\ncreated: 2026-04-01\n---\n\n# Alpha question\n\n## Related Projects\n- [[projects/alpha]]\n";

// A parked project — get_project must tolerate it (empty actions,
// status carried through) rather than erroring on list_actions.
const PARKED: &str = "---\ntype: project\ncontext: personal\nstatus: parked\ncreated: 2026-03-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n\n## Next Actions\n- [ ] Resume later (deep)\n\n## Milestones\n- [ ] Ship \u{2014} hard: 2026-09-01\n";

#[test]
fn get_project_composes_the_rich_detail_view() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("questions/research/alpha-q.md", QUESTION),
    ]);

    let detail = get_project_impl(&vault, "alpha", ymd(2026, 5, 26)).unwrap();

    assert_eq!(detail.slug, "alpha");
    assert_eq!(detail.status, ProjectStatus::Active);
    assert_eq!(detail.context, Context::Work);
    assert_eq!(detail.created, ymd(2026, 4, 1));
    assert_eq!(
        detail.core_question.as_deref(),
        Some("[[questions/research/alpha-q]]")
    );
    assert!(detail.body_markdown.contains("## Current State"));

    // Both open bullets, none of the (none here) closed ones.
    assert_eq!(detail.actions.len(), 2);

    // Only the open milestone; the completed Kickoff is filtered out.
    assert_eq!(detail.open_milestones.len(), 1);
    assert_eq!(detail.open_milestones[0].name, "Submit abstract");
    assert_eq!(
        detail.open_milestones[0].date.as_deref(),
        Some("2026-05-27")
    );
    assert!(detail.open_milestones[0].is_hard);

    // The question's body wikilink is a backlink grouped under questions.
    let questions: Vec<&str> = detail
        .backlinks
        .questions
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(questions, vec!["questions/research/alpha-q.md"]);
}

#[test]
fn get_project_tolerates_a_parked_project() {
    let vault = vault_with(&[("projects/_parked/beta.md", PARKED)]);

    let detail = get_project_impl(&vault, "beta", ymd(2026, 5, 26)).unwrap();

    assert_eq!(detail.status, ProjectStatus::Parked);
    // Actions fold to empty rather than erroring (list_actions refuses
    // parked projects) — the page renders read-only.
    assert!(
        detail.actions.is_empty(),
        "a parked project lists no actions: {:?}",
        detail.actions
    );
    // Milestones still read from the index for a parked project.
    assert_eq!(detail.open_milestones.len(), 1);
    assert_eq!(detail.open_milestones[0].name, "Ship");
}

#[test]
fn get_project_unknown_slug_is_not_found() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);
    let err = get_project_impl(&vault, "ghost", ymd(2026, 5, 26)).unwrap_err();
    // Neither active nor parked → the domain's NotFound maps through.
    assert!(
        matches!(err, cdno_tauri::error::CmdError::NotFound(_)),
        "unknown project slug is NotFound, got {err:?}"
    );
}
