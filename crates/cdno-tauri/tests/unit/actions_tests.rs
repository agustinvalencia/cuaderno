//! `list_all_actions_impl` against the Memory doubles — the
//! cross-project Actions view's composition seam, no Tauri runtime.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{Context, Vault};
use cdno_tauri::commands::actions::list_all_actions_impl;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
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

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] File receipts (light)\n";
const BETA: &str = "---\ntype: project\ncontext: personal\nstatus: active\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Next Actions\n- [ ] Book venue (medium)\n";
// Parked — excluded from active_projects, so it never appears here.
const GAMMA: &str = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Gamma\n\n## Next Actions\n- [ ] Someday (deep)\n";

#[test]
fn list_all_actions_groups_active_projects_with_context() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("projects/beta.md", BETA),
        ("projects/_parked/gamma.md", GAMMA),
    ]);

    let groups = list_all_actions_impl(&vault).unwrap();

    // Only the two active projects, never the parked one.
    assert_eq!(groups.len(), 2);

    let alpha = groups.iter().find(|g| g.slug == "alpha").expect("alpha");
    assert_eq!(alpha.context, Context::Work);
    assert_eq!(alpha.actions.len(), 2);

    let beta = groups.iter().find(|g| g.slug == "beta").expect("beta");
    assert_eq!(beta.context, Context::Personal);
    assert_eq!(beta.actions.len(), 1);
}
