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

#[test]
fn read_note_raw_returns_the_whole_file_verbatim() {
    let vault = vault_with(&[("projects/alpha.md", PROJECT)]);
    // The editor needs the exact bytes — frontmatter block and all — not
    // the display-split body.
    assert_eq!(
        vault.read_note_raw(&vp("projects/alpha.md")).unwrap(),
        PROJECT
    );
}

#[test]
fn write_note_raw_writes_and_indexes_a_fresh_note() {
    let vault = vault_with(&[]);
    vault
        .write_note_raw(&vp("zettels/idea.md"), "---\ntype: zettel\n---\n\n# Idea\n")
        .unwrap();
    // Bytes round-trip…
    assert_eq!(
        vault.read_note_raw(&vp("zettels/idea.md")).unwrap(),
        "---\ntype: zettel\n---\n\n# Idea\n"
    );
    // …and the reconcile that follows the write indexed the new file, so
    // the display read now has its type + title from the index/body.
    let view = vault.read_note(&vp("zettels/idea.md")).unwrap();
    assert_eq!(view.note_type.as_deref(), Some("zettel"));
    assert_eq!(view.title.as_deref(), Some("Idea"));
}

#[test]
fn write_note_raw_overwrites_existing_content() {
    let vault = vault_with(&[("projects/alpha.md", PROJECT)]);
    let edited = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nShipped.\n";
    vault
        .write_note_raw(&vp("projects/alpha.md"), edited)
        .unwrap();
    let view = vault.read_note(&vp("projects/alpha.md")).unwrap();
    assert!(view.body.contains("Shipped."));
    assert!(!view.body.contains("Going well."));
}

#[test]
fn write_note_raw_accepts_a_free_edit_the_schema_would_reject() {
    // The point of posture B: free editing writes exactly what it's given,
    // no schema gate. A frontmatter-less "project" that a structured create
    // would reject is written anyway (lint is the separate guardrail); the
    // reconcile after the write must tolerate it, not fail the write.
    let vault = vault_with(&[]);
    let rough = "# Rough\n\nno frontmatter, not a valid project\n";
    let result = vault.write_note_raw(&vp("projects/rough.md"), rough);
    assert!(result.is_ok(), "free edit must not be gated: {result:?}");
    assert_eq!(
        vault.read_note_raw(&vp("projects/rough.md")).unwrap(),
        rough
    );
}
