//! Unit tests for `Vault::orientation_context` — the composed daily
//! orient snapshot. `MemoryVaultStore` / `MemoryIndex` keep it fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::ProjectStatus;
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

/// Active project with a state snippet, a top action, and a hard
/// milestone one day out.
const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nMaking progress on the core loop.\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n\n## Milestones\n- [ ] Submit abstract — hard: 2026-05-27\n";

const PARKED_BETA: &str = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n\n## Next Actions\n- [ ] Resume someday (light)\n";

fn commitment(due: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: active\ndue: {due}\ncreated: 2026-05-01\ncompleted: null\ncontext: work\n---\n\n# Pay invoice\n"
    )
}

#[test]
fn orientation_context_composes_active_projects_and_commitments() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("projects/_parked/beta.md", PARKED_BETA),
        ("commitments/pay-invoice.md", &commitment("2026-05-27")),
    ]);

    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();

    // Only the active project, summarised with its state + top action.
    assert_eq!(ctx.projects.len(), 1, "parked beta is excluded");
    assert_eq!(ctx.projects[0].slug, "alpha");
    assert_eq!(ctx.projects[0].status, ProjectStatus::Active);
    assert!(!ctx.projects[0].state_snippet.is_empty());
    let top = ctx.projects[0].top_action.as_ref().expect("a top action");
    assert!(top.text.contains("Draft the methods section"));

    // Both the milestone (2026-05-27) and the commitment (2026-05-27)
    // fall in the 48h window.
    let titles: Vec<&str> = ctx.commitments.iter().map(|c| c.title.as_str()).collect();
    assert!(titles.contains(&"Submit abstract"), "milestone: {titles:?}");
    assert!(titles.contains(&"Pay invoice"), "commitment: {titles:?}");

    // Lapsed habits are empty until the stewardship layer (Phase 3).
    assert!(ctx.lapsed_habits.is_empty());
}

#[test]
fn orientation_context_commitments_honour_the_48h_window() {
    let vault = vault_with(&[
        // 1 day out — inside the 48h window.
        ("commitments/soon.md", &commitment("2026-05-27")),
        // 4 days out — beyond it.
        ("commitments/later.md", &commitment("2026-05-30")),
    ]);

    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();
    assert_eq!(
        ctx.commitments.len(),
        1,
        "only the near commitment: {:?}",
        ctx.commitments
    );
    assert_eq!(ctx.commitments[0].date, ymd(2026, 5, 27));
}

#[test]
fn orientation_context_is_empty_on_a_fresh_vault() {
    let vault = vault_with(&[]);
    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();
    assert!(ctx.commitments.is_empty());
    assert!(ctx.projects.is_empty());
    assert!(ctx.lapsed_habits.is_empty());
}
