//! Unit tests for `Vault::resolve_wikilink` — single-target wikilink
//! resolution for UI navigation. Mirrors the batch resolver's rules
//! (it delegates to it), so these pin the delegation: full-path match,
//! bare-slug stem match, unresolved, and the ambiguity → `None` rule.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Notes are written before `Vault::new` so startup reconciliation
/// indexes them — `resolve_wikilink` matches against the index's path
/// set and reads note_type from the index row.
fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const ALPHA: &str =
    "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n";
const INBOX_DUP: &str = "---\ntype: inbox\ncreated: 2026-04-01\n---\n\n# Dup\n";
const PORTFOLIO_INDEX: &str =
    "---\ntype: portfolio\nquestion: \"Q?\"\ncreated: 2026-04-01\n---\n\n# Topology\n";
const PROJECT_DUP: &str =
    "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Dup\n";

#[test]
fn resolves_by_full_path_and_carries_note_type() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    let resolved = vault
        .resolve_wikilink("projects/alpha")
        .unwrap()
        .expect("full-path target resolves");

    assert_eq!(resolved.path, vp("projects/alpha.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}

#[test]
fn resolves_by_bare_slug_stem() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    // The bare last-segment (stem) match: `[[alpha]]` finds the note
    // whose filename stem is `alpha`, wherever it lives.
    let resolved = vault
        .resolve_wikilink("alpha")
        .unwrap()
        .expect("bare slug resolves via its stem");

    assert_eq!(resolved.path, vp("projects/alpha.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}

#[test]
fn resolves_a_folder_target_to_its_index_note() {
    // A `[[portfolios/topology]]` link names the portfolio *folder*; it
    // resolves to that folder's `_index.md` and carries its note type, so
    // the UI can route it (here: open the portfolio index in the reader)
    // instead of silently muting a live link.
    let vault = vault_with(&[("portfolios/topology/_index.md", PORTFOLIO_INDEX)]);

    let resolved = vault
        .resolve_wikilink("portfolios/topology")
        .unwrap()
        .expect("a folder target resolves to its _index.md");

    assert_eq!(resolved.path, vp("portfolios/topology/_index.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("portfolio"));
}

#[test]
fn unresolved_target_is_none() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);

    assert!(
        vault.resolve_wikilink("does-not-exist").unwrap().is_none(),
        "a target matching no note resolves to None (the UI mutes it)"
    );
}

#[test]
fn blank_target_is_none_without_touching_the_index() {
    let vault = vault_with(&[("projects/alpha.md", ALPHA)]);
    assert!(vault.resolve_wikilink("   ").unwrap().is_none());
}

#[test]
fn ambiguous_bare_slug_resolves_to_none() {
    // Two notes share the stem `dup`. A bare `[[dup]]` is ambiguous;
    // the batch resolver leaves it unresolved (sound over available),
    // and so does the single-target path we delegate to.
    let vault = vault_with(&[
        ("projects/dup.md", PROJECT_DUP),
        ("inbox/dup.md", INBOX_DUP),
    ]);

    assert!(
        vault.resolve_wikilink("dup").unwrap().is_none(),
        "an ambiguous stem resolves to None, never guessing a note"
    );

    // But the fully-qualified path is unambiguous — it still resolves.
    let resolved = vault
        .resolve_wikilink("projects/dup")
        .unwrap()
        .expect("the exact path disambiguates");
    assert_eq!(resolved.path, vp("projects/dup.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
}

#[test]
fn resolve_wikilink_follows_an_alias() {
    // RFC 0007 §5.5: a note's slug-shaped `aliases:` entry resolves when no
    // note has that stem, through the same core rule the index uses, and
    // says it did. A stem still wins, and a direct hit is not marked.
    const RENAMED: &str = "---\ntype: project\ncontext: work\nstatus: active\n\
         created: 2026-04-01\naliases: [surrogate-model, \"Surrogate model\"]\n---\n\n\
         # Surrogate calibration\n";
    let vault = vault_with(&[("projects/surrogate-calibration.md", RENAMED)]);

    let resolved = vault
        .resolve_wikilink("surrogate-model")
        .unwrap()
        .expect("the old name resolves through the alias");
    assert_eq!(resolved.path, vp("projects/surrogate-calibration.md"));
    assert_eq!(resolved.note_type.as_deref(), Some("project"));
    assert!(resolved.via_alias);

    let direct = vault
        .resolve_wikilink("surrogate-calibration")
        .unwrap()
        .expect("the current slug resolves");
    assert!(!direct.via_alias);

    // A display-name alias is not slug-shaped and resolves nothing.
    assert!(vault.resolve_wikilink("Surrogate model").unwrap().is_none());

    let shadowed = vault_with(&[
        ("projects/surrogate-calibration.md", RENAMED),
        ("inbox/surrogate-model.md", INBOX_DUP),
    ]);
    let stem = shadowed
        .resolve_wikilink("surrogate-model")
        .unwrap()
        .expect("the stem resolves");
    assert_eq!(stem.path, vp("inbox/surrogate-model.md"));
    assert!(!stem.via_alias);
}

// ---------------------------------------------------------------------
// Tags follow aliases (RFC 0007 §5.5, T13).
// ---------------------------------------------------------------------

const RENAMED_ACTION: &str = "---\ntype: action\nstatus: open\ncreated: 2026-04-01\naliases: [old]\n---\n\n# Surrogate model\n";

fn daily(body: &str) -> String {
    format!("---\ntype: daily\ndate: 2026-04-02\n---\n\n{body}\n")
}

#[test]
fn a_tag_query_by_old_name_finds_the_note() {
    let vault = vault_with(&[
        ("actions/new.md", RENAMED_ACTION),
        (
            "journal/2026/daily/2026-04-02.md",
            &daily("Worked on #action/old today."),
        ),
    ]);
    let day = vec![vp("journal/2026/daily/2026-04-02.md")];

    assert_eq!(
        vault.notes_tagged("action/old").unwrap(),
        day,
        "by the old name"
    );
    assert_eq!(
        vault.notes_tagged("action/new").unwrap(),
        day,
        "by the current name"
    );
    assert!(vault.notes_tagged("action/other").unwrap().is_empty());
}

#[test]
fn a_tag_without_a_slash_is_passed_through() {
    let vault = vault_with(&[
        ("actions/new.md", RENAMED_ACTION),
        (
            "journal/2026/daily/2026-04-02.md",
            &daily("A #old thought."),
        ),
    ]);
    assert_eq!(
        vault.notes_tagged("old").unwrap(),
        vec![vp("journal/2026/daily/2026-04-02.md")]
    );
    assert!(vault.notes_tagged("new").unwrap().is_empty());
}

#[test]
fn tag_results_are_deduplicated_in_path_order() {
    let vault = vault_with(&[
        ("actions/new.md", RENAMED_ACTION),
        (
            "journal/2026/daily/2026-04-03.md",
            &daily("#action/new and #action/old"),
        ),
        ("journal/2026/daily/2026-04-02.md", &daily("#action/old")),
    ]);
    assert_eq!(
        vault.notes_tagged("action/old").unwrap(),
        vec![
            vp("journal/2026/daily/2026-04-02.md"),
            vp("journal/2026/daily/2026-04-03.md"),
        ]
    );
}

fn action_with_aliases(aliases: &str) -> String {
    format!(
        "---\ntype: action\nstatus: open\ncreated: 2026-04-01\naliases: [{aliases}]\n---\n\n# Surrogate model\n"
    )
}

const PLAIN_ACTION: &str =
    "---\ntype: action\nstatus: open\ncreated: 2026-04-01\n---\n\n# Surrogate model\n";

#[test]
fn a_shadowed_alias_is_not_expanded() {
    let vault = vault_with(&[
        ("actions/new.md", &action_with_aliases("y")),
        ("actions/y.md", PLAIN_ACTION),
        (
            "journal/2026/daily/2026-04-02.md",
            &daily("About #action/y."),
        ),
    ]);
    assert!(
        vault.notes_tagged("action/new").unwrap().is_empty(),
        "y resolves to actions/y.md, not to new"
    );
    assert_eq!(
        vault.notes_tagged("action/y").unwrap(),
        vec![vp("journal/2026/daily/2026-04-02.md")]
    );
}

#[test]
fn a_shared_alias_is_not_expanded() {
    let vault = vault_with(&[
        ("actions/a.md", &action_with_aliases("old")),
        ("actions/b.md", &action_with_aliases("old")),
        ("journal/2026/daily/2026-04-02.md", &daily("#action/old")),
        ("journal/2026/daily/2026-04-03.md", &daily("#action/a")),
        ("journal/2026/daily/2026-04-04.md", &daily("#action/b")),
    ]);
    assert_eq!(
        vault.notes_tagged("action/old").unwrap(),
        vec![vp("journal/2026/daily/2026-04-02.md")],
        "only the literal tag"
    );
    assert_eq!(
        vault.notes_tagged("action/a").unwrap(),
        vec![vp("journal/2026/daily/2026-04-03.md")],
        "a holder of a shared alias does not answer to it"
    );
}

#[test]
fn a_stem_beats_an_alias_in_tag_expansion() {
    let vault = vault_with(&[
        ("actions/x.md", &action_with_aliases("xx")),
        ("actions/other.md", &action_with_aliases("x, other-old")),
        ("journal/2026/daily/2026-04-02.md", &daily("#action/xx")),
        (
            "journal/2026/daily/2026-04-03.md",
            &daily("#action/other-old"),
        ),
    ]);
    assert_eq!(
        vault.notes_tagged("action/x").unwrap(),
        vec![vp("journal/2026/daily/2026-04-02.md")],
        "x names the stem note only"
    );
}

#[test]
fn a_shared_stem_is_not_expanded() {
    let vault = vault_with(&[
        ("actions/dup.md", &action_with_aliases("p-old")),
        ("inbox/dup.md", &action_with_aliases("i-old")),
        ("journal/2026/daily/2026-04-02.md", &daily("#action/dup")),
        ("journal/2026/daily/2026-04-03.md", &daily("#action/p-old")),
        ("journal/2026/daily/2026-04-04.md", &daily("#action/i-old")),
    ]);
    assert_eq!(
        vault.notes_tagged("action/dup").unwrap(),
        vec![vp("journal/2026/daily/2026-04-02.md")],
        "only the literal tag"
    );
}

#[test]
fn resolves_a_bare_folder_note_name() {
    // #538: a portfolio answers to its folder name.
    let vault = vault_with(&[("portfolios/topology/_index.md", PORTFOLIO_INDEX)]);

    let resolved = vault
        .resolve_wikilink("topology")
        .unwrap()
        .expect("bare folder name resolves");
    assert_eq!(resolved.path, vp("portfolios/topology/_index.md"));
    assert!(vault.resolve_wikilink("other/topology").unwrap().is_none());
}
