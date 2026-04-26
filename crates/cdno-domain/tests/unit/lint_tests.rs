use std::sync::Arc;

use cdno_core::config::{SchemaExtension, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Build a vault containing the given `(path, body)` notes. Reconciliation
/// runs as part of `Vault::new` so the index reflects the seeded files.
fn vault_with_notes(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new succeeded");
    vault
}

#[test]
fn lint_returns_empty_report_for_empty_vault() {
    let vault = vault_with_notes(&[], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_passes_for_a_valid_note_with_a_known_type() {
    let body = "---\ntype: daily\ntitle: A clean note\n---\n# Body\n";
    let vault = vault_with_notes(&[("note.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_flags_a_note_with_an_unknown_type() {
    let body = "---\ntype: bogus\ntitle: Mystery\n---\n# Body\n";
    let vault = vault_with_notes(&[("strange.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].path, vp("strange.md"));
    assert!(
        report.issues[0].message.contains("unknown note type"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_flags_a_missing_extra_required_field() {
    let body = "---\ntype: project\ntitle: A project without an owner\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].path, vp("projects/foo.md"));
    assert!(
        report.issues[0]
            .message
            .contains("missing required field `owner`"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_passes_when_extra_required_field_is_present() {
    let body = "---\ntype: project\ntitle: A project\nowner: alice\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_skips_extra_required_check_when_type_is_unknown() {
    // The note has both an unknown type AND would be missing a
    // required field if its declared type were valid. Only the
    // type issue should appear — chaining further checks against an
    // unknown type adds noise without telling the user anything new.
    let body = "---\ntype: bogus\ntitle: confused\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "bogus".to_string(),
        SchemaExtension {
            extra_required: vec!["irrelevant".to_string()],
        },
    );
    let vault = vault_with_notes(&[("note.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert!(report.issues[0].message.contains("unknown note type"));
}

#[test]
fn lint_treats_explicit_null_value_as_missing() {
    // YAML `owner: ~` round-trips to JSON `null`. From a schema
    // perspective the field is unset, so lint should flag it.
    let body = "---\ntype: project\ntitle: nulled out\nowner: ~\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert!(
        report.issues[0]
            .message
            .contains("missing required field `owner`")
    );
}
