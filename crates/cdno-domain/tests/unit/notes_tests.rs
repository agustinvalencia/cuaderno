//! Unit tests for `Vault::read_note` — the display-oriented note
//! reader. `MemoryVaultStore` / `MemoryIndex` keep it fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;

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

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nGoing well.\n";

#[test]
fn read_note_splits_frontmatter_title_and_body() {
    let vault = vault_with(&[("projects/alpha.md", PROJECT)]);

    let view = vault.read_note(&vp("projects/alpha.md")).unwrap();

    assert_eq!(view.note_type.as_deref(), Some("project"));
    assert_eq!(view.title.as_deref(), Some("Alpha"));
    assert_eq!(view.frontmatter["type"], "project");
    assert_eq!(view.frontmatter["context"], "work");
    assert!(view.body.contains("## Current State"));
    assert!(!view.body.contains("---"), "frontmatter stripped from body");
}

#[test]
fn read_note_tolerates_frontmatterless_files() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    // Written after Vault::new, so it's also unindexed — both fallback
    // paths (no frontmatter, no index row) in one fixture.
    store
        .write_file(&vp("inbox/scratch.md"), "just a bare thought\n")
        .unwrap();

    let view = vault.read_note(&vp("inbox/scratch.md")).unwrap();

    assert_eq!(view.note_type, None);
    assert_eq!(view.title, None);
    assert_eq!(view.frontmatter, serde_json::Value::Null);
    assert_eq!(view.body, "just a bare thought\n");
}

#[test]
fn read_note_missing_file_is_not_found() {
    let vault = vault_with(&[]);
    let err = vault.read_note(&vp("projects/ghost.md")).unwrap_err();
    assert!(matches!(err, DomainError::Store(_)));
}
