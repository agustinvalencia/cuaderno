//! `list_questions_impl` against the Memory doubles — the command seam,
//! no Tauri runtime involved.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::commands::questions::list_questions_impl;

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

fn question(domain: &str, status: &str, h1: &str) -> String {
    format!(
        "---\ntype: question\ndomain: {domain}\nstatus: {status}\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# {h1}\n"
    )
}

#[test]
fn list_questions_is_empty_for_a_vault_with_none() {
    let vault = vault_with(&[]);

    assert!(list_questions_impl(&vault).unwrap().is_empty());
}

#[test]
fn list_questions_returns_every_status_not_only_active() {
    // The Strategic grid shows only active questions. This view is where
    // the questions live, so a parked or answered one must still be
    // findable — otherwise answering a question makes it vanish.
    let vault = vault_with(&[
        (
            "questions/research/open.md",
            &question("research", "active", "Does the sparse variant hold up?"),
        ),
        (
            "questions/research/settled.md",
            &question("research", "answered", "Is the encoder worth its cost?"),
        ),
        (
            "questions/life/rest.md",
            &question("life", "parked", "What does a sustainable week look like?"),
        ),
    ]);

    let rows = list_questions_impl(&vault).unwrap();

    assert_eq!(rows.len(), 3, "{rows:?}");
    let statuses: Vec<String> = rows
        .iter()
        .map(|r| format!("{:?}", r.summary.status))
        .collect();
    assert!(
        statuses.iter().any(|s| s.contains("Answered")),
        "{statuses:?}"
    );
    assert!(
        statuses.iter().any(|s| s.contains("Parked")),
        "{statuses:?}"
    );
}

#[test]
fn a_question_carries_its_text_and_domain_for_grouping() {
    // The question is phrased as a question in its H1 — that, not the
    // slug, is what the view shows.
    let vault = vault_with(&[(
        "questions/research/sparse.md",
        &question("research", "active", "Does the sparse variant hold up?"),
    )]);

    let rows = list_questions_impl(&vault).unwrap();

    assert_eq!(
        rows[0].summary.question_text,
        "Does the sparse variant hold up?"
    );
    assert_eq!(format!("{:?}", rows[0].summary.domain), "Research");
}

#[test]
fn a_question_carries_the_projects_and_portfolios_that_reference_it() {
    // The links are the point: a question with nothing pointing at it is
    // one nobody is working on, and that should be visible.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\ncore_question: \"[[questions/research/sparse]]\"\n---\n\n# Alpha\n";
    let vault = vault_with(&[
        (
            "questions/research/sparse.md",
            &question("research", "active", "Does the sparse variant hold up?"),
        ),
        ("projects/alpha.md", project),
    ]);

    let rows = list_questions_impl(&vault).unwrap();

    assert_eq!(
        rows[0].backlinks.projects,
        vec!["projects/alpha.md".to_owned()],
        "a project's core_question is a frontmatter wikilink and counts"
    );
}

#[test]
fn a_backlink_lookup_failure_leaves_the_rest_of_the_list_intact() {
    // A hand-edited vault with the same slug in both domains resolves as
    // ambiguous. One bad question must not blank the page.
    let vault = vault_with(&[
        (
            "questions/research/dup.md",
            &question("research", "active", "Research phrasing"),
        ),
        (
            "questions/life/dup.md",
            &question("life", "active", "Life phrasing"),
        ),
    ]);

    let rows = list_questions_impl(&vault).unwrap();

    assert_eq!(rows.len(), 2, "both questions still listed: {rows:?}");
}
