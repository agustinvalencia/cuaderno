//! Unit tests for `Vault::resolve_wikilink` — single-target wikilink
//! resolution for UI navigation. Mirrors the batch resolver's rules
//! (it delegates to it), so these pin the delegation: full-path match,
//! bare-slug stem match, unresolved, and the ambiguity → `None` rule.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Notes are written before `Vault::new` so startup reconciliation
/// indexes them — `resolve_wikilink` matches against the index's path
/// set and reads note_type from the index row.
fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const ALPHA: &str =
    "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n";
const INBOX_DUP: &str = "---\ntype: inbox\ncreated: 2026-04-01\n---\n\n# Dup\n";
const PORTFOLIO_INDEX: &str =
    "---\ntype: portfolio\nquestion: \"Q?\"\ncreated: 2026-04-01\n---\n\n# Topology\n";
const PROJECT_DUP: &str =
    "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Dup\n";

#[test]
fn resolves_by_full_path_and_carries_note_type() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    let resolved = vault
        .resolve_wikilink("projects/alpha")
        .unwrap()
        .expect("full-path target resolves");

    assert_eq!(resolved.path, vp("projects/alpha.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}

#[test]
fn resolves_by_bare_slug_stem() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    // The bare last-segment (stem) match: `[[alpha]]` finds the note
    // whose filename stem is `alpha`, wherever it lives.
    let resolved = vault
        .resolve_wikilink("alpha")
        .unwrap()
        .expect("bare slug resolves via its stem");

    assert_eq!(resolved.path, vp("projects/alpha.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}

#[test]
fn resolves_a_folder_target_to_its_index_note() {
    // A `[[portfolios/topology]]` link names the portfolio *folder*; it
    // resolves to that folder's `_index.md` and carries its note type, so
    // the UI can route it (here: open the portfolio index in the reader)
    // instead of silently muting a live link.
    let vault = vault_with(&[("portfolios/topology/_index.md", PORTFOLIO_INDEX)]);

    let resolved = vault
        .resolve_wikilink("portfolios/topology")
        .unwrap()
        .expect("a folder target resolves to its _index.md");

    assert_eq!(resolved.path, vp("portfolios/topology/_index.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("portfolio"));
}

#[test]
fn unresolved_target_is_none() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    assert!(
        vault.resolve_wikilink("does-not-exist").unwrap().is_none(),
        "a target matching no note resolves to None (the UI mutes it)"
    );
}

#[test]
fn blank_target_is_none_without_touching_the_index() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);
    assert!(vault.resolve_wikilink("   ").unwrap().is_none());
}

#[test]
fn ambiguous_bare_slug_resolves_to_none() {
    // Two notes share the stem `dup`. A bare `[[dup]]` is ambiguous;
    // the batch resolver leaves it unresolved (sound over available),
    // and so does the single-target path we delegate to.
    let vault = vault_with(&[
        ("projects/dup.md", PROJECT_DUP),
        ("inbox/dup.md", INBOX_DUP),
    ]);

    assert!(
        vault.resolve_wikilink("dup").unwrap().is_none(),
        "an ambiguous stem resolves to None, never guessing a note"
    );

    // But the fully-qualified path is unambiguous — it still resolves.
    let resolved = vault
        .resolve_wikilink("projects/dup")
        .unwrap()
        .expect("the exact path disambiguates");
    assert_eq!(resolved.path, vp("projects/dup.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}
