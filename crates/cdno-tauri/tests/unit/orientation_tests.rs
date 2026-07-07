//! `get_orientation_impl` against the Memory doubles — the command
//! seam, no Tauri runtime involved.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::commands::orientation::get_orientation_impl;
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

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nCore loop underway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] File receipts (light)\n\n## Milestones\n- [ ] Submit abstract — hard: 2026-05-27\n";

const HEALTH: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Active Habits\n- Swimming 1x/week \u{2014} lapsed since March\n";

#[test]
fn orientation_view_composes_context_actions_and_lapses() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("stewardships/health.md", HEALTH),
    ]);

    let view = get_orientation_impl(&vault, ymd(2026, 5, 26)).unwrap();

    assert_eq!(view.today, ymd(2026, 5, 26));
    // The hard milestone one day out shows in the strip.
    assert_eq!(view.commitments.len(), 1);
    assert_eq!(view.commitments[0].title, "Submit abstract");

    // One project, carrying its life context and EVERY open bullet —
    // the energy selector filters client-side.
    assert_eq!(view.projects.len(), 1);
    let project = &view.projects[0];
    assert_eq!(project.summary.slug, "alpha");
    assert_eq!(project.context, cdno_domain::Context::Work);
    assert_eq!(project.actions.len(), 2);

    assert_eq!(view.lapsed_habits.len(), 1);
    assert_eq!(view.lapsed_habits[0].stewardship, "health");
}

#[test]
fn orientation_view_empty_vault_is_calmly_empty() {
    let vault = vault_with(&[]);
    let view = get_orientation_impl(&vault, ymd(2026, 5, 26)).unwrap();
    assert!(view.commitments.is_empty());
    assert!(view.projects.is_empty());
    assert!(view.lapsed_habits.is_empty());
}
