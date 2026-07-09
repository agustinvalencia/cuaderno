//! The config-inspector command seams (#365, PR1) against the Memory
//! doubles — the raw read and the dry-run validate, no Tauri runtime.

use std::sync::Arc;

use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::validate_config_str;
use cdno_tauri::commands::config::read_config_impl;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn vault_with(files: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in files {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(store, index, cdno_core::config::VaultConfig::default()).expect("Vault::new");
    vault
}

#[test]
fn read_config_impl_returns_the_document_and_hash() {
    let raw = "[vault]\nname = \"Test\"\nmax_active_projects = 5\n";
    let vault = vault_with(&[(".cuaderno/config.toml", raw)]);

    let doc = read_config_impl(&vault).expect("read_config_impl");
    assert_eq!(doc.content, raw);
    assert_eq!(doc.hash, content_hash(raw));
}

#[test]
fn validate_config_str_ok_for_a_valid_config() {
    // The command body calls `validate_config_str` directly (it needs no
    // vault); exercise the same entry point the command does.
    let raw = "[note_types.person]\nfolder = \"people\"\n";
    assert!(validate_config_str(raw).is_ok());
}

#[test]
fn validate_config_str_surfaces_a_structured_error_for_an_invalid_config() {
    let raw = "[note_types.Project]\nfolder = \"myprojects\"\n";
    let err = validate_config_str(raw).expect_err("should reject a built-in shadow");
    assert!(!err.message.is_empty());
    // A semantic (non-positional) error carries no line/col.
    assert_eq!(err.line, None);
    assert_eq!(err.col, None);
}
