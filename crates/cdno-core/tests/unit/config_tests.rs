use cdno_core::config::VaultConfig;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_config(dir: &Path, content: &str) {
    let config_dir = dir.join(".cuaderno");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), content).unwrap();
}

#[test]
fn load_returns_defaults_when_no_config_file() {
    let dir = TempDir::new().unwrap();
    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "My Vault");
    assert_eq!(config.vault.max_active_projects, 5);
    assert!(config.schemas.is_empty());
    assert!(config.variables.static_vars.is_empty());
    assert!(config.variables.prompt.is_empty());
}

#[test]
fn load_parses_full_config() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Research Lab"
max_active_projects = 3

[schemas.project]
extra_required = ["collaborators", "funding_source"]

[schemas.evidence]
extra_required = []

[variables]
author = "A. Researcher"
institution = "University of Examples"
orcid = "0000-0000-0000-0000"

[variables.prompt]
collaborators = "Who are the collaborators?"
experiment_id = "Experiment identifier?"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "Research Lab");
    assert_eq!(config.vault.max_active_projects, 3);

    let project_schema = config.schema_for("project").unwrap();
    assert_eq!(
        project_schema.extra_required,
        vec!["collaborators", "funding_source"]
    );

    let evidence_schema = config.schema_for("evidence").unwrap();
    assert!(evidence_schema.extra_required.is_empty());

    assert_eq!(config.resolve_variable("author"), Some("A. Researcher"));
    assert_eq!(
        config.resolve_variable("institution"),
        Some("University of Examples")
    );
    assert_eq!(config.resolve_variable("nonexistent"), None);

    assert_eq!(
        config.prompt_for_variable("collaborators"),
        Some("Who are the collaborators?")
    );
    assert_eq!(config.prompt_for_variable("author"), None);
}

#[test]
fn load_parses_minimal_config() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Minimal"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "Minimal");
    assert_eq!(config.vault.max_active_projects, 5);
    assert!(config.schemas.is_empty());
}

#[test]
fn load_returns_error_for_invalid_toml() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "this is not valid toml [[[");

    let result = VaultConfig::load(dir.path());
    assert!(result.is_err());
}

#[test]
fn extra_required_fields_returns_empty_for_unknown_type() {
    let config = VaultConfig::default();
    assert!(config.extra_required_fields("project").is_empty());
}
