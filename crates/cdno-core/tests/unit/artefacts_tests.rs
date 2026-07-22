// Attachment-artefact ownership (#451). A markdown file inside a folder
// owned by an evidence stub is a filed document, not a note, so
// reconciliation must not try to index it and lint must not call its
// folder an orphan. Both sides resolve ownership through these helpers.

use std::collections::HashSet;
use std::sync::Arc;

use cdno_core::artefacts::{attachment_stubs, is_attachment_stub, owning_artefact_stub};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Resolve against a fixed set of paths already known to be attachment
/// stubs, as reconciliation does once it has read them.
fn owner(path: &str, stubs: &[&str]) -> Option<String> {
    owning_artefact_stub(&vp(path), |stub| stubs.iter().any(|s| vp(s) == *stub))
        .map(|s| s.to_string())
}

#[test]
fn artefact_beside_its_stub_is_owned() {
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-paper/paper.pdf",
            &["portfolios/demo/2026-06-13-paper.md"],
        ),
        Some("portfolios/demo/2026-06-13-paper.md".to_string()),
    );
}

#[test]
fn markdown_artefact_is_owned_just_like_any_other_file() {
    // The whole point of #451: filing a `.md` document produces the same
    // stub-plus-folder pair as filing a PDF, and the artefact has no
    // frontmatter, so indexing it can only ever fail.
    assert_eq!(
        owner(
            "portfolios/demo/2026-07-03-review-panel/02-reviewer-b.md",
            &["portfolios/demo/2026-07-03-review-panel.md"],
        ),
        Some("portfolios/demo/2026-07-03-review-panel.md".to_string()),
    );
}

#[test]
fn evidence_note_at_the_portfolio_root_is_not_an_artefact() {
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-paper.md",
            &["portfolios/demo/2026-06-13-paper.md", "portfolios.md"],
        ),
        None,
    );
}

#[test]
fn portfolio_index_is_not_an_artefact() {
    assert_eq!(
        owner("portfolios/demo/_index.md", &["portfolios/demo.md"]),
        None
    );
}

#[test]
fn folder_without_a_stub_owns_nothing() {
    assert_eq!(owner("portfolios/demo/assets/pasted.png", &[]), None);
}

#[test]
fn ownership_survives_an_intervening_grouping_folder() {
    // Depth-independence, the constraint from #454: a portfolio that
    // grows grouping subfolders must not need this rule rewritten.
    assert_eq!(
        owner(
            "portfolios/demo/sweep/2026-06-13-run-07/metrics.json",
            &["portfolios/demo/sweep/2026-06-13-run-07.md"],
        ),
        Some("portfolios/demo/sweep/2026-06-13-run-07.md".to_string()),
    );
}

#[test]
fn ownership_reaches_through_nesting_inside_the_artefact_folder() {
    // A filed directory tree keeps its internal structure; every file in
    // it resolves to the same owning stub as its siblings.
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-bundle/src/deep/main.rs",
            &["portfolios/demo/2026-06-13-bundle.md"],
        ),
        Some("portfolios/demo/2026-06-13-bundle.md".to_string()),
    );
}

#[test]
fn nearest_owning_ancestor_wins() {
    // Both an inner and an outer candidate stub exist; the inner one is
    // the owner, so the artefact is attributed to the closest stub.
    assert_eq!(
        owner(
            "portfolios/demo/outer/inner/file.txt",
            &["portfolios/demo/outer.md", "portfolios/demo/outer/inner.md"],
        ),
        Some("portfolios/demo/outer/inner.md".to_string()),
    );
}

#[test]
fn a_dotted_folder_name_pairs_with_the_right_stub() {
    // `with_extension` would rewrite `run-v1.2` to the stub `run-v1.md`
    // and pair the artefact with an unrelated note; the stub name is
    // built by appending, not replacing.
    assert_eq!(
        owner(
            "portfolios/demo/run-v1.2/out.log",
            &["portfolios/demo/run-v1.md"],
        ),
        None,
    );
    assert_eq!(
        owner(
            "portfolios/demo/run-v1.2/out.log",
            &["portfolios/demo/run-v1.2.md"],
        ),
        Some("portfolios/demo/run-v1.2.md".to_string()),
    );
}

#[test]
fn the_rule_is_confined_to_portfolios() {
    // An expanded stewardship is `stewardships/<slug>/` and a flat one is
    // `stewardships/<slug>.md`. If the pairing applied vault-wide, a
    // vault holding both spellings would see the expanded folder's notes
    // silently vanish from the index.
    assert_eq!(
        owner("stewardships/health/_index.md", &["stewardships/health.md"],),
        None,
    );
    assert_eq!(
        owner("projects/alpha/notes.md", &["projects/alpha.md"]),
        None,
    );
}

// ---------------------------------------------------------------------
// Positive stub identification. Ownership must never be inferred from the
// path shape alone: a plain note that merely shares a name with a sibling
// folder would otherwise swallow that folder's real notes out of the
// index, silently and with the files untouched on disk.
// ---------------------------------------------------------------------

const STUB_MD: &str = "---\ntype: evidence\ncreated: 2026-07-03\nsource: A filed document\nportfolio: demo\norigin: \"[[projects/foo]]\"\nkind: pdf\n---\n# A filed document\n";

const PLAIN_EVIDENCE_MD: &str = "---\ntype: evidence\ncreated: 2026-07-03\nsource: A quarter summary\nportfolio: demo\norigin: \"[[projects/foo]]\"\n---\n# A quarter summary\n";

const PROJECT_MD: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-07-03\nkind: pdf\n---\n# Not evidence\n";

fn store_with(files: &[(&str, &str)]) -> Arc<dyn VaultStore> {
    let store = MemoryVaultStore::new();
    for (path, body) in files {
        store.write_file(&vp(path), body).unwrap();
    }
    Arc::new(store)
}

#[test]
fn an_evidence_note_carrying_kind_is_a_stub() {
    let store = store_with(&[("portfolios/demo/2026-07-03-paper.md", STUB_MD)]);
    assert!(is_attachment_stub(
        &store,
        &vp("portfolios/demo/2026-07-03-paper.md")
    ));
}

#[test]
fn evidence_without_kind_is_not_a_stub() {
    // The case that carries the risk: an ordinary evidence note sitting
    // beside a folder of its own.
    let store = store_with(&[("portfolios/demo/2026-Q2.md", PLAIN_EVIDENCE_MD)]);
    assert!(!is_attachment_stub(
        &store,
        &vp("portfolios/demo/2026-Q2.md")
    ));
}

#[test]
fn a_non_evidence_note_is_not_a_stub_even_with_kind() {
    let store = store_with(&[("portfolios/demo/2026-Q2.md", PROJECT_MD)]);
    assert!(!is_attachment_stub(
        &store,
        &vp("portfolios/demo/2026-Q2.md")
    ));
}

#[test]
fn an_unreadable_or_frontmatterless_file_is_not_a_stub() {
    // Wrong in this direction merely leaves a file indexed, where a parse
    // error is reported normally. Wrong the other way removes notes from
    // the index without a word.
    let store = store_with(&[("portfolios/demo/loose.md", "# No frontmatter\n")]);
    assert!(!is_attachment_stub(&store, &vp("portfolios/demo/loose.md")));
    assert!(!is_attachment_stub(
        &store,
        &vp("portfolios/demo/absent.md")
    ));
}

#[test]
fn a_plain_note_never_owns_its_namesake_folder() {
    // The regression this rule exists for. `2026-Q2.md` is a quarter
    // summary, not an attachment stub, so the eight evidence notes filed
    // under `2026-Q2/` must stay notes.
    let store = store_with(&[
        ("portfolios/demo/2026-Q2.md", PLAIN_EVIDENCE_MD),
        ("portfolios/demo/2026-Q2/first.md", PLAIN_EVIDENCE_MD),
    ]);
    let md: HashSet<VaultPath> = [
        "portfolios/demo/2026-Q2.md",
        "portfolios/demo/2026-Q2/first.md",
    ]
    .iter()
    .map(|p| vp(p))
    .collect();
    let dirs: HashSet<VaultPath> = ["portfolios/demo", "portfolios/demo/2026-Q2"]
        .iter()
        .map(|p| vp(p))
        .collect();

    let stubs = attachment_stubs(&store, &md, &dirs);
    assert!(stubs.is_empty(), "stubs: {stubs:?}");
    assert_eq!(
        owning_artefact_stub(&vp("portfolios/demo/2026-Q2/first.md"), |s| stubs
            .contains(s)),
        None,
    );
}

#[test]
fn attachment_stubs_finds_only_folder_owning_evidence_stubs() {
    let store = store_with(&[
        ("portfolios/demo/2026-07-03-paper.md", STUB_MD),
        ("portfolios/demo/2026-07-03-paper/paper.pdf", "%PDF"),
        // A stub with no folder of its own owns nothing, so it is not
        // probed and not returned.
        ("portfolios/demo/2026-07-04-other.md", STUB_MD),
        ("portfolios/demo/2026-Q2.md", PLAIN_EVIDENCE_MD),
        ("portfolios/demo/2026-Q2/first.md", PLAIN_EVIDENCE_MD),
    ]);
    let md: HashSet<VaultPath> = [
        "portfolios/demo/2026-07-03-paper.md",
        "portfolios/demo/2026-07-04-other.md",
        "portfolios/demo/2026-Q2.md",
        "portfolios/demo/2026-Q2/first.md",
    ]
    .iter()
    .map(|p| vp(p))
    .collect();
    let dirs: HashSet<VaultPath> = [
        "portfolios/demo",
        "portfolios/demo/2026-07-03-paper",
        "portfolios/demo/2026-Q2",
    ]
    .iter()
    .map(|p| vp(p))
    .collect();

    let stubs = attachment_stubs(&store, &md, &dirs);
    assert_eq!(
        stubs,
        [vp("portfolios/demo/2026-07-03-paper.md")]
            .into_iter()
            .collect::<HashSet<_>>(),
    );
}

#[test]
fn an_outer_stub_still_owns_past_a_non_stub_candidate() {
    // A candidate that is not a stub does not stop the walk — the file is
    // still inside the outer stub's artefact folder.
    assert_eq!(
        owner(
            "portfolios/demo/bundle/inner/file.txt",
            &["portfolios/demo/bundle.md"],
        ),
        Some("portfolios/demo/bundle.md".to_string()),
    );
}
