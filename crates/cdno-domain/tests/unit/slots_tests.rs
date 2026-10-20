//! `slot_usage` (RFC 0006 §8.0): the active-project cap count every cap
//! site reads.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
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

#[test]
fn ensure_slot_follows_the_one_rule() {
    // RFC 0006 §7.2: the cap is checked whenever a map that did not count
    // begins to count, and only then. At the cap, not→counts is refused,
    // listing the counted maps; every other transition is allowed. Below
    // the cap, all four are allowed.
    let cap = usize::from(VaultConfig::default().vault.max_active_projects);
    let maps: Vec<(String, String)> = (1..=cap)
        .map(|i| {
            (
                format!("projects/own-{i}.md"),
                project_body("active", &format!("Own {i}")),
            )
        })
        .collect();
    let held = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n\
                held_by: \"the harbour office\"\n---\n# Harbour\n";
    let mut at_cap: Vec<(&str, &str)> =
        maps.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect();
    at_cap.push(("projects/harbour.md", held));
    let full = vault_with_notes(&at_cap).slot_usage().unwrap();
    assert_eq!((full.active, full.counted), (cap + 1, cap));

    let below = vault_with_notes(&at_cap[1..]).slot_usage().unwrap();
    assert_eq!(below.counted, cap - 1);

    let table = [
        // (counts_before, counts_after, refused at the cap)
        (false, true, true),
        (true, true, false),
        (true, false, false),
        (false, false, false),
    ];
    for (before, after, refused) in table {
        match full.ensure_slot(before, after) {
            Err(DomainError::ProjectCapReached {
                current,
                max,
                active_projects,
            }) => {
                assert!(refused, "{before}->{after} must be allowed");
                assert_eq!((current, max), (cap, cap));
                assert_eq!(active_projects, full.counted_slugs);
                assert!(!active_projects.contains(&"harbour".to_owned()));
            }
            Err(other) => panic!("{before}->{after}: unexpected {other:?}"),
            Ok(()) => assert!(!refused, "{before}->{after} must be refused at the cap"),
        }
        assert!(
            below.ensure_slot(before, after).is_ok(),
            "{before}->{after} below the cap"
        );
    }
}
