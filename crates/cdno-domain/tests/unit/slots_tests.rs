//! `slot_usage` (RFC 0006 §8.0): the active-project cap count every cap
//! site reads.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{SlotUsage, Vault};

fn vault_with_notes(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(*path).unwrap(), body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

fn project_body(status: &str, title: &str) -> String {
    format!(
        "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-01-01\n---\n# {title}\n"
    )
}

#[test]
fn slot_usage_counts_active_maps_only() {
    // One map in each state: only the active one holds a slot. A parked
    // map waits on the shelf and a closed one is never read.
    let active = project_body("active", "Alpha");
    let parked = project_body("parked", "Beta");
    let closed = "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2025-01-01\nclosed: 2026-01-01\n---\n# Gamma\n";
    let vault = vault_with_notes(&[
        ("projects/alpha.md", active.as_str()),
        ("projects/_parked/beta.md", parked.as_str()),
        ("projects/_done/2026/gamma.md", closed),
    ]);

    assert_eq!(
        vault.slot_usage().unwrap(),
        SlotUsage {
            active: 1,
            counted: 1,
            cap: 5,
            counted_slugs: vec!["alpha".to_owned()],
        }
    );
}
