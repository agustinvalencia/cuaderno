//! Config inspector domain seams (#365, PR1): `Vault::read_config_raw`
//! (verbatim read + stable hash) and the `validate_config_str` dry-run,
//! which must catch exactly what `Vault::new` would reject.

use std::sync::Arc;

use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::validate_config_str;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// A vault over the Memory doubles with `files` pre-written. The config
/// passed to `Vault::new` is the default (a stored `config.toml` file is
/// independent of it — `read_config_raw` reads the file, not the parsed
/// config).
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

// --- read_config_raw ---------------------------------------------------

#[test]
fn read_config_raw_returns_verbatim_content_and_a_stable_hash() {
    let raw = "[vault]\nname = \"Test\"\nmax_active_projects = 5\n\n# a comment\n";
    let vault = vault_with(&[(".cuaderno/config.toml", raw)]);

    let doc = vault.read_config_raw().expect("read_config_raw");
    // Verbatim — comments and ordering preserved.
    assert_eq!(doc.content, raw);
    // The hash is the shared content hash of the content.
    assert_eq!(doc.hash, content_hash(raw));

    // Same content on a second read yields the same hash (stability).
    let again = vault.read_config_raw().expect("second read");
    assert_eq!(again.hash, doc.hash);
    assert_eq!(again.content, doc.content);
}

#[test]
fn read_config_raw_is_defensive_about_a_missing_file() {
    // A vault with no config file on the store: the read hands back an
    // empty document rather than erroring.
    let vault = vault_with(&[]);
    let doc = vault.read_config_raw().expect("read_config_raw");
    assert_eq!(doc.content, "");
    assert_eq!(doc.hash, content_hash(""));
}

// --- validate_config_str: acceptance -----------------------------------

#[test]
fn validate_accepts_a_well_formed_config() {
    let raw = "\
[vault]
name = \"Test\"
max_active_projects = 5

[note_types.person]
folder = \"people\"
required = [\"name\"]
title_field = \"name\"

[schemas.daily.fields.mood]
type = \"string\"
values = [\"good\", \"ok\", \"low\"]
";
    assert!(validate_config_str(raw).is_ok());
}

#[test]
fn validate_accepts_an_empty_config() {
    // Every field is `#[serde(default)]`, so an empty string parses to
    // the default config and passes every check.
    assert!(validate_config_str("").is_ok());
}

// --- validate_config_str: rejection ------------------------------------

#[test]
fn validate_rejects_a_toml_syntax_error_with_line_and_col() {
    // An unterminated table header — a hard TOML parse error.
    let raw = "[vault\nname = \"x\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(!err.message.is_empty());
    // TOML reports a span, so the structured line/col are populated.
    assert_eq!(err.line, Some(1));
    assert!(err.col.is_some());
}

#[test]
fn validate_rejects_a_custom_type_shadowing_a_builtin_case_insensitively() {
    // `Project` shadows the built-in `project` (case-insensitive).
    let raw = "[note_types.Project]\nfolder = \"myprojects\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(
        err.message.to_lowercase().contains("project"),
        "message names the offending type: {}",
        err.message
    );
    // A semantic error, not positional.
    assert_eq!(err.line, None);
    assert_eq!(err.col, None);
}

#[test]
fn validate_rejects_a_folder_colliding_with_a_reserved_top_level_folder() {
    // `journal` is a built-in top-level folder.
    let raw = "[note_types.person]\nfolder = \"journal\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(
        err.message.contains("journal"),
        "message names the folder: {}",
        err.message
    );
}

#[test]
fn validate_rejects_a_nested_folder_under_a_reserved_top_level_folder() {
    // The top-level segment is what collides — `projects/people` is
    // rejected even though the leaf differs.
    let raw = "[note_types.person]\nfolder = \"projects/people\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(err.message.contains("projects"), "{}", err.message);
}

#[test]
fn validate_rejects_a_schema_field_redeclaring_the_period_key() {
    // `date` is daily's engine-owned period identity key — hard reserved.
    let raw = "[schemas.daily.fields.date]\ntype = \"date\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(err.message.contains("date"), "{}", err.message);
}

#[test]
fn validate_rejects_values_on_a_non_string_field() {
    let raw = "[schemas.daily.fields.count]\ntype = \"int\"\nvalues = [\"a\", \"b\"]\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(err.message.contains("values"), "{}", err.message);
}

#[test]
fn validate_rejects_list_true() {
    let raw = "[schemas.daily.fields.tags]\ntype = \"string\"\nlist = true\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(err.message.contains("list"), "{}", err.message);
}

#[test]
fn validate_rejects_an_unknown_field_spec_key() {
    // `deny_unknown_fields` on `FieldSpec` turns a stray key into a hard
    // deserialize error — caught in the parse step.
    let raw = "[schemas.daily.fields.mood]\ntype = \"string\"\nbogus = 1\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(!err.message.is_empty());
}

#[test]
fn validate_rejects_a_dangling_title_field() {
    // `title_field` names a field absent from `required`/`optional`.
    let raw = "[note_types.person]\nfolder = \"people\"\ntitle_field = \"name\"\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(err.message.contains("title_field"), "{}", err.message);
}

#[test]
fn validate_rejects_a_malformed_ignore_glob() {
    // A bad glob surfaces from `ignore_set()`, the same step `Vault::new`
    // runs between parse and type-registry validation.
    let raw = "ignore = [\"[unterminated\"]\n";
    let err = validate_config_str(raw).expect_err("should reject");
    assert!(!err.message.is_empty());
}
