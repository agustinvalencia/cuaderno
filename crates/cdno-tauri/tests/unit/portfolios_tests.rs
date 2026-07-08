//! The Portfolio Browser seams against the Memory doubles — the
//! composed detail view-model and the wikilink-stripping helper it
//! leans on, no Tauri runtime involved. The IPC round-trips live in
//! `ipc_tests.rs`; these exercise the composition rules directly:
//! frontmatter-link lowering (alias form), the body question-scan
//! filter, and the newest-first evidence ordering.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::commands::portfolios::{get_portfolio_impl, strip_wikilink};

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

// A portfolio `_index.md` whose frontmatter `project` link is in the
// alias form (`[[target|Label]]`) — the composer must lower it to the
// bare navigable target. Its body's `## Related Questions` mixes a real
// question link with two non-question links (a project and a
// stewardship) that the question-scan must filter out.
const SURROGATE_INDEX: &str = "---\ntype: portfolio\nquestion: How does the surrogate behave?\ncreated: 2026-06-01\nproject: \"[[projects/alpha|Alpha Project]]\"\n---\n\n# How does the surrogate behave?\n\n## Related Questions\n- [[questions/research/surrogate-fidelity]]\n- [[projects/alpha]]\n- [[stewardships/health]]\n\n## Evidence\n";

// Two evidence notes filed out of chronological order in the fixture
// vector — the composer must return them newest-first regardless.
const EVIDENCE_OLD: &str = "---\ntype: evidence\ncreated: 2026-07-01\nsource: Smith 2024\nportfolio: surrogate\norigin: \"[[projects/alpha]]\"\n---\n\n# Smith 2024\n\nThe error stayed bounded.\n";
const EVIDENCE_NEW: &str = "---\ntype: evidence\ncreated: 2026-07-05\nsource: Lab notebook\nportfolio: surrogate\norigin: \"[[projects/alpha]]\"\n---\n\n# Lab notebook\n\nReran the sweep.\n";

fn fixture() -> Vec<(&'static str, &'static str)> {
    vec![
        ("portfolios/surrogate/_index.md", SURROGATE_INDEX),
        (
            "portfolios/surrogate/2026-07-01-smith-2024.md",
            EVIDENCE_OLD,
        ),
        (
            "portfolios/surrogate/2026-07-05-lab-notebook.md",
            EVIDENCE_NEW,
        ),
    ]
}

#[test]
fn get_portfolio_lowers_the_alias_form_project_link() {
    let vault = vault_with(&fixture());
    let detail = get_portfolio_impl(&vault, "surrogate").unwrap();
    // The frontmatter link `[[projects/alpha|Alpha Project]]` is lowered
    // to the bare target the frontend routes on — the label is dropped.
    assert_eq!(detail.project.as_deref(), Some("projects/alpha"));
    assert_eq!(detail.question, "How does the surrogate behave?");
}

#[test]
fn get_portfolio_question_scan_keeps_only_questions_links() {
    let vault = vault_with(&fixture());
    let detail = get_portfolio_impl(&vault, "surrogate").unwrap();
    // Of the three body links, only the `questions/` one survives; the
    // project and stewardship links are filtered out.
    assert_eq!(
        detail.questions,
        vec!["questions/research/surrogate-fidelity"]
    );
}

#[test]
fn get_portfolio_returns_evidence_newest_first() {
    let vault = vault_with(&fixture());
    let detail = get_portfolio_impl(&vault, "surrogate").unwrap();
    let dates: Vec<String> = detail
        .evidence
        .iter()
        .map(|e| e.created.to_string())
        .collect();
    // Seeded old-then-new, returned new-then-old.
    assert_eq!(
        dates,
        vec!["2026-07-05".to_owned(), "2026-07-01".to_owned()]
    );
    // The origin chip is lowered to a bare target too.
    assert_eq!(detail.evidence[0].source, "Lab notebook");
    assert_eq!(detail.evidence[0].origin, "projects/alpha");
}

#[test]
fn strip_wikilink_handles_the_composer_edge_cases() {
    // Bare bracketed link.
    assert_eq!(strip_wikilink("[[projects/foo]]"), "projects/foo");
    // Alias form drops the label.
    assert_eq!(strip_wikilink("[[projects/foo|Foo]]"), "projects/foo");
    // Idempotent on an already-bare target (hand-edited frontmatter).
    assert_eq!(strip_wikilink("projects/foo"), "projects/foo");
    // Surrounding and inner whitespace is trimmed on both sides of the pipe.
    assert_eq!(
        strip_wikilink("  [[ projects/foo | Foo ]]  "),
        "projects/foo"
    );
    // An empty label collapses to the bare target.
    assert_eq!(strip_wikilink("[[projects/foo|]]"), "projects/foo");
}
