//! Tests for template-engine wiring (#212): note creation resolves a
//! custom `.cuaderno/templates/` override through the `VaultStore`, and
//! falls back to the built-in default when none exists.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::Context;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 26).unwrap()
}

fn vault_with(custom: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in custom {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

#[test]
fn create_uses_a_custom_type_template_override_from_the_store() {
    // A custom project template in the vault (read via the store, not
    // the filesystem) takes precedence over the built-in default.
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\n---\n# {{title}}\n\nCUSTOM PROJECT BODY\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/project.md", custom)]);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM PROJECT BODY"),
        "the custom template should be used:\n{content}"
    );
    // Built-in template variables still resolve inside the custom one.
    assert!(content.contains("# My Proj"), "{content}");
    assert!(content.contains("context: work"), "{content}");
    assert!(content.contains("status: active"), "{content}");
    // The built-in body must NOT appear.
    assert!(
        !content.contains("No work done yet"),
        "built-in body leaked:\n{content}"
    );
}

#[test]
fn create_uses_a_custom_variant_template_override_from_the_store() {
    // The store-backed loader must also honour the variant tier:
    // a custom `tracking-gym.md` wins over the built-in `tracking-gym`
    // default for a gym entry. (The core engine tests cover variant
    // precedence with the *filesystem* loader; this exercises the
    // *store* loader wired in by this PR — the new code path.)
    let custom = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: {{activity}}\ndate: {{date}}\n---\n# {{title}}\n\nCUSTOM GYM BODY\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/tracking-gym.md", custom)]);
    vault
        .create_stewardship_expanded(
            today().and_hms_opt(9, 0, 0).unwrap(),
            "Health",
            Context::Personal,
        )
        .expect("create stewardship");

    let path = vault
        .add_tracking_entry(
            today().and_hms_opt(19, 0, 0).unwrap(),
            "health",
            "gym",
            None,
            "Energy was good.",
        )
        .expect("add tracking entry");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM GYM BODY"),
        "the custom variant template should be used:\n{content}"
    );
    assert!(content.contains("activity: gym"), "{content}");
    // The built-in gym body (its exercise table) must NOT leak through.
    assert!(
        !content.contains("| Exercise | Sets | Reps"),
        "built-in gym body leaked:\n{content}"
    );
}

#[test]
fn create_falls_back_to_the_builtin_template_when_no_custom_exists() {
    // No custom template → the built-in default is used (its body text).
    let (vault, store) = vault_with(&[]);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("No work done yet"),
        "built-in template expected:\n{content}"
    );
}

#[test]
fn created_notes_have_no_unsubstituted_placeholders() {
    // The project/action/commitment creation tests assert *parsed*
    // frontmatter (order-insensitive) rather than raw output, so a ctx
    // that missed a `{{placeholder}}` would slip past them. Guard the
    // wiring directly, exercising the optional/link paths (core_question,
    // milestone/due nulls, commitment project link) most likely to drift.
    use cdno_domain::frontmatter::EnergyLevel;

    let (vault, store) = vault_with(&[]);
    let at = today().and_hms_opt(9, 0, 0).unwrap();

    let project = vault
        .create_project(today(), "Proj", Context::Work, Some("questions/q"))
        .expect("project");
    let action = vault
        .add_action_with_note(at, "proj", "Do the thing", EnergyLevel::Deep)
        .expect("action");
    let commitment = vault
        .create_commitment(at, "Promise", today(), Context::Work, Some("proj"), None)
        .expect("commitment");

    for path in [project, action, commitment] {
        let content = store.read_file(&path).unwrap();
        assert!(
            !content.contains("{{") && !content.contains("}}"),
            "unsubstituted placeholder in {path}:\n{content}"
        );
    }
}
