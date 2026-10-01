//! `Vault::sync_template` (#699): a custom override of a built-in template
//! gains the frontmatter keys the built-in has and it lacks, and nothing
//! else changes.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{LintSeverity, TemplateSyncStatus, Vault};

const OVERRIDE_PATH: &str = ".cuaderno/templates/project.md";

/// A project override ejected before `closed:` existed, with a key of the
/// user's own and a custom body.
const STALE_PROJECT: &str = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question: {{core_question}}\nowner: unassigned\n---\n\n# {{title}}\n\n## Risks\n\nNone yet.\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn vault_with(files: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in files {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

#[test]
fn sync_inserts_the_missing_key_after_its_built_in_neighbour() {
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, STALE_PROJECT)]);

    let report = vault.sync_template("project", true).expect("sync");

    assert_eq!(report.status, TemplateSyncStatus::Synced);
    assert_eq!(report.added, vec!["closed".to_owned()]);
    assert_eq!(report.kept, vec!["owner".to_owned()]);
    let expected = STALE_PROJECT.replace(
        "core_question: {{core_question}}\n",
        "core_question: {{core_question}}\nclosed: null\n",
    );
    assert_eq!(store.read_file(&vp(OVERRIDE_PATH)).unwrap(), expected);
}

#[test]
fn a_second_sync_reports_nothing_to_do_and_writes_nothing() {
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, STALE_PROJECT)]);
    vault.sync_template("project", true).expect("first sync");
    let after_first = store.read_file(&vp(OVERRIDE_PATH)).unwrap();

    let report = vault.sync_template("project", true).expect("second sync");

    assert_eq!(report.status, TemplateSyncStatus::UpToDate);
    assert!(report.added.is_empty());
    assert_eq!(store.read_file(&vp(OVERRIDE_PATH)).unwrap(), after_first);
}

#[test]
fn check_reports_behind_and_writes_nothing() {
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, STALE_PROJECT)]);

    let report = vault.sync_template("project", false).expect("check");

    assert_eq!(report.status, TemplateSyncStatus::Behind);
    assert_eq!(report.added, vec!["closed".to_owned()]);
    assert_eq!(store.read_file(&vp(OVERRIDE_PATH)).unwrap(), STALE_PROJECT);
}

#[test]
fn several_missing_keys_land_in_built_in_order() {
    // Missing `core_question` and `closed`; `owner` sits where
    // `core_question` used to be.
    let stale = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\nowner: unassigned\n---\n# {{title}}\n";
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, stale)]);

    let report = vault.sync_template("project", true).expect("sync");

    assert_eq!(
        report.added,
        vec!["core_question".to_owned(), "closed".to_owned()]
    );
    assert_eq!(
        store.read_file(&vp(OVERRIDE_PATH)).unwrap(),
        "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question: {{core_question}}\nclosed: null\nowner: unassigned\n---\n# {{title}}\n"
    );
}

#[test]
fn a_missing_first_key_goes_before_its_following_neighbour() {
    let stale = "---\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question: {{core_question}}\nclosed: null\n---\n";
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, stale)]);

    vault.sync_template("project", true).expect("sync");

    assert!(
        store
            .read_file(&vp(OVERRIDE_PATH))
            .unwrap()
            .starts_with("---\ntype: project\ncontext: {{context}}\n")
    );
}

#[test]
fn a_continuation_block_stays_with_its_key() {
    // The user's `tags` list keeps its items; `closed` goes after the
    // whole `core_question` block, not between it and its continuation.
    let stale = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question:\n  - a\n  - b\ntags:\n  - x\n---\n";
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, stale)]);

    vault.sync_template("project", true).expect("sync");

    assert_eq!(
        store.read_file(&vp(OVERRIDE_PATH)).unwrap(),
        "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question:\n  - a\n  - b\nclosed: null\ntags:\n  - x\n---\n"
    );
}

#[test]
fn crlf_overrides_get_crlf_lines() {
    let stale = STALE_PROJECT.replace('\n', "\r\n");
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, &stale)]);

    vault.sync_template("project", true).expect("sync");

    let written = store.read_file(&vp(OVERRIDE_PATH)).unwrap();
    assert!(written.contains("core_question: {{core_question}}\r\nclosed: null\r\nowner"));
    assert!(!written.replace("\r\n", "").contains('\n'), "{written:?}");
}

#[test]
fn an_override_without_frontmatter_is_reported_not_rewritten() {
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, "# {{title}}\n")]);

    let report = vault.sync_template("project", true).expect("sync");

    assert_eq!(report.status, TemplateSyncStatus::NoFrontmatter);
    assert_eq!(
        store.read_file(&vp(OVERRIDE_PATH)).unwrap(),
        "# {{title}}\n"
    );
}

#[test]
fn sync_all_with_no_overrides_reports_nothing() {
    let (vault, _store) = vault_with(&[]);

    assert!(vault.sync_all_templates(true).expect("sync all").is_empty());
    let report = vault.sync_template("project", true).expect("sync");
    assert_eq!(report.status, TemplateSyncStatus::NotCustomised);
}

#[test]
fn a_config_custom_type_is_not_syncable() {
    let (vault, _store) = vault_with(&[]);

    assert!(vault.sync_template("person", true).is_err());
}

#[test]
fn list_templates_flags_a_stale_override() {
    let (vault, _store) = vault_with(&[(OVERRIDE_PATH, STALE_PROJECT)]);

    let rows = vault.list_templates().expect("list");

    let project = rows.iter().find(|r| r.note_type == "project").unwrap();
    assert_eq!(project.missing_builtin_keys, vec!["closed".to_owned()]);
    let action = rows.iter().find(|r| r.note_type == "action").unwrap();
    assert!(action.missing_builtin_keys.is_empty());
}

#[test]
fn lint_warns_on_a_stale_override_and_is_clean_after_sync() {
    let (vault, _store) = vault_with(&[(OVERRIDE_PATH, STALE_PROJECT)]);

    let rows: Vec<_> = vault
        .lint_all_notes()
        .unwrap()
        .issues
        .into_iter()
        .filter(|i| i.path.to_string() == OVERRIDE_PATH)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].severity, LintSeverity::Warning);
    assert_eq!(
        rows[0].message,
        "custom template `project` lacks `closed` (run `cdno templates sync project`)"
    );

    vault.sync_template("project", true).expect("sync");
    assert!(
        vault
            .lint_all_notes()
            .unwrap()
            .issues
            .iter()
            .all(|i| i.path.to_string() != OVERRIDE_PATH)
    );
}

#[test]
fn every_built_in_ejected_unchanged_is_up_to_date() {
    // An override that IS the built-in must never be reported behind:
    // otherwise `templates eject --all` would immediately lint dirty.
    let (vault, _store) = vault_with(&[]);
    for nt in cdno_domain::note_type::NoteType::ALL {
        vault
            .eject_template(nt.as_str(), None, false)
            .expect("eject");
    }

    let reports = vault.sync_all_templates(false).expect("check all");

    assert_eq!(reports.len(), cdno_domain::note_type::NoteType::ALL.len());
    for report in reports {
        assert_eq!(report.status, TemplateSyncStatus::UpToDate, "{report:?}");
    }
}

// Review findings on #715.

#[test]
fn a_quoted_or_spaced_key_counts_as_present() {
    for line in ["\"closed\": null", "'closed': null", "closed : null"] {
        let text = STALE_PROJECT.replace("owner: unassigned", line);
        let (vault, store) = vault_with(&[(OVERRIDE_PATH, &text)]);

        let report = vault.sync_template("project", true).expect("sync");

        assert_eq!(report.status, TemplateSyncStatus::UpToDate, "{line}");
        assert_eq!(store.read_file(&vp(OVERRIDE_PATH)).unwrap(), text, "{line}");
    }
}

#[test]
fn a_new_key_is_not_wedged_between_a_comment_and_its_key() {
    let stale = STALE_PROJECT.replace(
        "owner: unassigned",
        "# who answers for it\nowner: unassigned",
    );
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, &stale)]);

    vault.sync_template("project", true).expect("sync");

    assert!(store.read_file(&vp(OVERRIDE_PATH)).unwrap().contains(
        "core_question: {{core_question}}\nclosed: null\n# who answers for it\nowner: unassigned\n"
    ));
}

#[test]
fn line_endings_follow_the_frontmatter_not_the_body() {
    // Split after the closing `---` line, so only the body turns CRLF.
    let (fm, body) = STALE_PROJECT.split_at(STALE_PROJECT.find("---\n\n# ").unwrap() + 4);
    let mixed = format!("{fm}{}", body.replace('\n', "\r\n"));
    let (vault, store) = vault_with(&[(OVERRIDE_PATH, &mixed)]);

    vault.sync_template("project", true).expect("sync");

    assert!(
        store
            .read_file(&vp(OVERRIDE_PATH))
            .unwrap()
            .contains("core_question: {{core_question}}\nclosed: null\nowner")
    );
}

#[test]
fn a_delimiter_with_trailing_spaces_is_not_frontmatter() {
    // `cdno_core::frontmatter` does not accept it either, so sync must not
    // call such a file current.
    let broken = STALE_PROJECT.replacen("---\n", "---   \n", 1);
    let (vault, _store) = vault_with(&[(OVERRIDE_PATH, &broken)]);

    let report = vault.sync_template("project", false).expect("check");

    assert_eq!(report.status, TemplateSyncStatus::NoFrontmatter);
}

#[test]
fn a_url_at_column_zero_is_not_a_key() {
    let text = STALE_PROJECT.replace(
        "owner: unassigned",
        "owner: unassigned\nhttps://example.com",
    );
    let (vault, _store) = vault_with(&[(OVERRIDE_PATH, &text)]);

    let report = vault.sync_template("project", false).expect("check");

    assert_eq!(report.kept, vec!["owner".to_owned()]);
}

#[test]
fn a_hash_or_blank_line_inside_a_multi_line_value_stays_in_it() {
    // In a quoted or block scalar, or a nested mapping holding one, a
    // trailing `#` or blank line belongs to the value: `closed` goes after
    // the whole block, never inside it.
    for value in [
        "\"Default question\n  # see the guide\"",
        "'Default question\n# see the guide'",
        "|\n  Line one\n  # heading",
        ">\n  Line one\n  # heading",
        "|+\n  Line one\n",
        "\n  note: |\n    text\n    # heading",
    ] {
        let block = format!("core_question: {value}\n");
        let stale = STALE_PROJECT.replace("core_question: {{core_question}}\n", &block);
        let (vault, store) = vault_with(&[(OVERRIDE_PATH, &stale)]);

        let report = vault.sync_template("project", true).expect("sync");

        assert_eq!(report.added, vec!["closed".to_owned()], "{value}");
        assert!(
            store
                .read_file(&vp(OVERRIDE_PATH))
                .unwrap()
                .contains(&format!("{block}closed: null\nowner: unassigned\n")),
            "{value}"
        );
    }
}
