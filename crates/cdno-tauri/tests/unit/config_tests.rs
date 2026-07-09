//! The config command seams (#365) against the Memory doubles — the raw
//! read + dry-run validate (PR1) and the validate → compare-and-swap →
//! write save (PR3), no Tauri runtime.

use std::sync::Arc;

use cdno_core::config::{FieldType, VaultConfig};
use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::{ConfigSaveError, validate_config_str};
use cdno_tauri::commands::config::{
    load_vault_and_ignore, read_config_impl, read_config_model_impl, save_config_impl,
};

const CONFIG_PATH: &str = ".cuaderno/config.toml";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Build a vault plus keep a handle on its store, so a test can read the
/// on-disk config back after a save to assert byte-level fidelity.
fn vault_and_store(files: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in files {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(store.clone(), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn vault_with(files: &[(&str, &str)]) -> Vault {
    vault_and_store(files).0
}

/// The raw config text currently on disk (empty string if absent).
fn on_disk(store: &Arc<dyn VaultStore>) -> String {
    store.read_file(&vp(CONFIG_PATH)).unwrap_or_default()
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

// --- save_config (#365, PR3): the validate → CAS → write gate ---

/// A valid baseline config carrying a comment and a custom type, so the
/// never-brick and round-trip tests can prove the file is left EXACTLY as
/// it was (comment + ordering included) on a rejected save.
const BASELINE: &str =
    "# vault config — hand-annotated\n[note_types.person]\nfolder = \"people\"\n";

/// The never-brick proof. Each candidate violates a distinct reserved
/// constraint; every one MUST be rejected as a validation error AND leave
/// the on-disk config byte-identical to the baseline — no candidate that
/// fails to reopen can ever touch the file.
#[test]
fn save_config_rejects_every_invalid_candidate_and_never_touches_the_file() {
    // (label, candidate that must fail validate_config_str).
    let cases: &[(&str, &str)] = &[
        // A custom type shadowing a built-in, case-insensitively.
        (
            "builtin shadow",
            "[note_types.Project]\nfolder = \"myprojects\"\n",
        ),
        // A folder that collides with a reserved top-level folder.
        (
            "reserved folder",
            "[note_types.widget]\nfolder = \"projects\"\n",
        ),
        // Redeclaring a built-in's period/identity key.
        (
            "period-key redeclare",
            "[schemas.daily.fields.date]\ntype = \"string\"\n",
        ),
        // `values` on a non-string field.
        (
            "values on non-string",
            "[schemas.project.fields.grade]\ntype = \"integer\"\nvalues = [\"a\", \"b\"]\n",
        ),
        // The reserved-but-unimplemented `list = true`.
        (
            "list = true",
            "[schemas.project.fields.tags]\ntype = \"string\"\nlist = true\n",
        ),
        // An unknown field-spec key (deny_unknown_fields at deserialize).
        (
            "unknown key",
            "[schemas.project.fields.foo]\ntype = \"string\"\nbogus = 1\n",
        ),
        // An unknown field type (rejected by the FieldType enum).
        (
            "unknown type",
            "[schemas.project.fields.foo]\ntype = \"colour\"\n",
        ),
        // A `title_field` naming a field that isn't declared.
        (
            "dangling title_field",
            "[note_types.widget]\nfolder = \"widgets\"\ntitle_field = \"nope\"\n",
        ),
        // A raw TOML syntax error (unterminated table header).
        (
            "toml syntax error",
            "[note_types.widget\nfolder = \"widgets\"\n",
        ),
    ];

    for (label, candidate) in cases {
        let (vault, store) = vault_and_store(&[(CONFIG_PATH, BASELINE)]);
        let hash = content_hash(BASELINE);

        let err = save_config_impl(&vault, candidate, &hash)
            .expect_err(&format!("`{label}` should be rejected"));
        // A candidate that would not reopen is a Validation rejection with
        // a non-empty, targeted message.
        match err {
            ConfigSaveError::Validation(inner) => {
                assert!(!inner.message.is_empty(), "`{label}` message empty");
            }
            other => panic!("`{label}` expected Validation, got {other:?}"),
        }
        // The load-bearing assertion: the file was NOT touched.
        assert_eq!(
            on_disk(&store),
            BASELINE,
            "`{label}` must leave the config byte-identical"
        );
    }
}

/// Happy path: a valid save that adds a custom type updates the file
/// verbatim, the returned hash matches a fresh read, and a rebuild from
/// the persisted config (what the live reload does) sees the new type —
/// proving the edit applies without a restart.
#[test]
fn save_config_writes_verbatim_and_the_new_type_is_visible_after_reload() {
    let (vault, store) = vault_and_store(&[(CONFIG_PATH, BASELINE)]);
    let hash = content_hash(BASELINE);

    let new_config = "# vault config — hand-annotated\n[note_types.widget]\nfolder = \"widgets\"\n";
    let doc = save_config_impl(&vault, new_config, &hash).expect("valid save");

    // The file is exactly the buffer we wrote — comment and ordering kept.
    assert_eq!(on_disk(&store), new_config);
    // The returned hash is authoritative: it matches a fresh read.
    assert_eq!(doc.content, new_config);
    assert_eq!(doc.hash, content_hash(&on_disk(&store)));

    // Rebuild the vault from the persisted config on the SAME store — this
    // is exactly what the live reload does (VaultConfig::load + Vault::new).
    // A type-registry-dependent command (list_templates) must now see the
    // new `widget` type, with no restart.
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let reloaded_config: VaultConfig =
        toml::from_str(&on_disk(&store)).expect("parse persisted config");
    let (reloaded, _report) = Vault::new(store, index, reloaded_config).expect("reload rebuild");
    let types = reloaded.list_templates().expect("list_templates");
    assert!(
        types.iter().any(|t| t.note_type == "widget"),
        "the reloaded vault should recognise the new custom type"
    );
}

/// Compare-and-swap: a save carrying a STALE `expected_hash` (the file
/// changed underneath, simulated by writing a different config first) is
/// rejected as a conflict and leaves the on-disk file untouched.
#[test]
fn save_config_rejects_a_stale_hash_without_writing() {
    let (vault, store) = vault_and_store(&[(CONFIG_PATH, BASELINE)]);
    // The editor's baseline hash, captured before the concurrent edit.
    let stale_hash = content_hash(BASELINE);

    // A concurrent hand-edit lands: the on-disk config is now different.
    let concurrent = "# someone edited this by hand\n[note_types.person]\nfolder = \"folks\"\n";
    store.write_file(&vp(CONFIG_PATH), concurrent).unwrap();

    // The candidate itself is valid, so only the CAS can reject it.
    let candidate = "[note_types.widget]\nfolder = \"widgets\"\n";
    let err = save_config_impl(&vault, candidate, &stale_hash)
        .expect_err("a stale hash must be rejected");
    assert!(matches!(err, ConfigSaveError::Conflict));
    // The concurrent edit is preserved, not clobbered.
    assert_eq!(on_disk(&store), concurrent);
}

/// Round-trip fidelity: saving the SAME content back (with the matching
/// hash) leaves the file byte-identical — a verbatim write, comments and
/// ordering preserved.
#[test]
fn save_config_round_trips_identical_content() {
    let (vault, store) = vault_and_store(&[(CONFIG_PATH, BASELINE)]);
    let hash = content_hash(BASELINE);

    let doc = save_config_impl(&vault, BASELINE, &hash).expect("re-save is valid");
    assert_eq!(on_disk(&store), BASELINE);
    assert_eq!(doc.content, BASELINE);
    // An unchanged file re-hashes to the same value it started with.
    assert_eq!(doc.hash, hash);
}

// --- load_vault_and_ignore (#365, PR4): the rebuild core, proven over the
//     Memory doubles + a real on-disk config, no Tauri AppHandle ---

/// Write `config` to `<root>/.cuaderno/config.toml`, mirroring what the
/// rebuild core re-reads from disk (`VaultConfig::load`).
fn write_config_at(root: &std::path::Path, config: &str) {
    let cuaderno = root.join(".cuaderno");
    std::fs::create_dir_all(&cuaderno).expect("create .cuaderno");
    std::fs::write(cuaderno.join("config.toml"), config).expect("write config.toml");
}

/// A valid on-disk config rebuilds a sound vault whose type registry sees
/// the newly declared custom type — the happy path the watcher's live
/// reload takes.
#[test]
fn load_vault_and_ignore_builds_from_a_valid_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_config_at(tmp.path(), "[note_types.person]\nfolder = \"people\"\n");

    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _ignore) =
        load_vault_and_ignore(store, index, tmp.path()).expect("a valid config must build");

    let types = vault.list_templates().expect("list_templates");
    assert!(
        types.iter().any(|t| t.note_type == "person"),
        "the rebuilt vault should recognise the custom type"
    );
}

// --- read_config_model (#365, PR5a): the structured projection, proven
//     over the Memory doubles from an in-memory parsed config ---

/// Build a vault whose live config is parsed from `raw` — the source
/// `read_config_model_impl` projects (`vault.config()`), no filesystem.
fn vault_from_config(raw: &str) -> Vault {
    let config: VaultConfig = toml::from_str(raw).expect("parse config");
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    Vault::new(store, index, config).expect("Vault::new").0
}

/// The projection carries the vault meta, every note type (with folder,
/// required/optional, template), and every schema field (with its
/// FieldType, default, and values) — and both lists come back sorted by
/// name, not in the config's `HashMap` iteration order.
#[test]
fn read_config_model_projects_meta_note_types_and_schemas_sorted() {
    // `reading` is declared before `demo`, so a correct projection must
    // reorder them (`demo` < `reading`) rather than echo insertion order.
    let raw = r#"
[vault]
name = "Demo Vault"
max_active_projects = 3

[note_types.reading]
folder = "reading"
required = ["author"]
optional = ["rating"]
template = "reading.md"
append_only = false

[note_types.demo]
folder = "demo"

[schemas.project.fields.stage]
type = "string"
default = "idea"
values = ["idea", "active", "done"]
required = true
"#;
    let vault = vault_from_config(raw);
    let model = read_config_model_impl(&vault).expect("read_config_model_impl");

    // Vault meta rides through verbatim.
    assert_eq!(model.vault.name, "Demo Vault");
    assert_eq!(model.vault.max_active_projects, 3);

    // Note types are sorted by name: `demo` before `reading`.
    let names: Vec<&str> = model.note_types.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["demo", "reading"]);

    let reading = &model.note_types[1];
    assert_eq!(reading.note_type.folder, "reading");
    assert_eq!(reading.note_type.required, ["author"]);
    assert_eq!(reading.note_type.optional, ["rating"]);
    assert_eq!(reading.note_type.template.as_deref(), Some("reading.md"));

    // The schema field carries its FieldType, default, and allowed values.
    assert_eq!(model.schemas.len(), 1);
    let schema = &model.schemas[0];
    assert_eq!(schema.name, "project");
    let stage = schema.schema.fields.get("stage").expect("stage field");
    assert_eq!(stage.ty, FieldType::String);
    assert_eq!(
        stage.default.as_ref().and_then(|v| v.as_str()),
        Some("idea")
    );
    assert_eq!(
        stage.values.as_deref(),
        Some(["idea".to_string(), "active".to_string(), "done".to_string()].as_slice())
    );
    assert!(stage.required);
}

/// An empty config projects to empty note-type and schema lists — the
/// structured view's empty state, not an error.
#[test]
fn read_config_model_is_empty_for_a_default_config() {
    let vault = vault_from_config("");
    let model = read_config_model_impl(&vault).expect("read_config_model_impl");
    assert!(model.note_types.is_empty());
    assert!(model.schemas.is_empty());
}

/// Serialize sanity: a `FieldSpec` renders its `ty` under the wire key
/// `type` (the `#[serde(rename)]` round-trips through Serialize) and a
/// scalar default rides as its bare JSON scalar, matching the ts-rs
/// `string | number | boolean | null` shape the frontend consumes.
#[test]
fn field_spec_serialises_type_key_and_scalar_default() {
    let raw = "[schemas.project.fields.stage]\ntype = \"string\"\ndefault = \"idea\"\n";
    let config: VaultConfig = toml::from_str(raw).expect("parse config");
    let spec = config
        .schemas
        .get("project")
        .and_then(|s| s.fields.get("stage"))
        .expect("stage field");

    let json = serde_json::to_value(spec).expect("serialise FieldSpec");
    assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("string"));
    assert_eq!(json.get("default").and_then(|v| v.as_str()), Some("idea"));
}

/// A syntactically broken config surfaces an error — nothing to swap, so
/// the never-brick guarantee holds (the caller keeps the old vault live).
#[test]
fn load_vault_and_ignore_rejects_a_broken_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Unterminated table header — a raw TOML syntax error.
    write_config_at(tmp.path(), "[note_types.person\nfolder = \"people\"\n");

    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    assert!(
        load_vault_and_ignore(store, index, tmp.path()).is_err(),
        "a broken config must error, leaving nothing to swap"
    );
}
