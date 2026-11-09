// Attachment-artefact ownership (#451). A markdown file inside a folder
// owned by an evidence stub is a filed document, not a note, so
// reconciliation must not try to index it and lint must not call its
// folder an orphan. Both sides resolve ownership through these helpers.

use std::collections::HashSet;
use std::sync::Arc;

use cdno_core::artefacts::{folder_owners, is_attachment_stub, owning_note};
use cdno_core::classify::NoteScope;
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Resolve against a fixed set of paths already known to be attachment
/// stubs, as reconciliation does once it has read them.
fn owner(path: &str, stubs: &[&str]) -> Option<String> {
    owning_note(&vp(path), &NoteScope::empty(), |stub| {
        stubs.iter().any(|s| vp(s) == *stub)
    })
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
fn outermost_owning_ancestor_wins() {
    // Owners resolve shallow-first: an inner candidate inside an owned
    // folder owns nothing, so it is not even probed.
    assert_eq!(
        owner(
            "portfolios/demo/outer/inner/file.txt",
            &["portfolios/demo/outer.md", "portfolios/demo/outer/inner.md"],
        ),
        Some("portfolios/demo/outer.md".to_string()),
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

    let stubs = folder_owners(&store, &md, &dirs, &NoteScope::empty());
    assert!(stubs.is_empty(), "stubs: {stubs:?}");
    assert_eq!(
        owning_note(
            &vp("portfolios/demo/2026-Q2/first.md"),
            &NoteScope::empty(),
            |s| stubs.contains(s)
        ),
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

    let stubs = folder_owners(&store, &md, &dirs, &NoteScope::empty());
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

// ---------------------------------------------------------------------
// A Typst note owns its sibling folder (RFC 0008 §5.7).
// ---------------------------------------------------------------------

const TYPST_NOTE: &str = "#metadata((type: \"evidence\")) <cdno>\n\nBody.\n";
const TYPST_UNLABELLED: &str = "= A loose sheet\n\nNo metadata here.\n";

/// Owners as the classifier resolves them, then ownership of every file.
fn scope_of(admitted: &[&str], excluded: &[&str]) -> NoteScope {
    let mut scope = NoteScope::empty();
    scope.typst_folders = admitted.iter().map(|f| f.to_string()).collect();
    scope.typst_excluded_folders = excluded.iter().map(|f| f.to_string()).collect();
    scope
}

fn owners_of(
    files: &[(&str, &str)],
    typst_folders: &[&str],
    probe: &[&str],
) -> Vec<Option<String>> {
    owners_in(files, &scope_of(typst_folders, &[]), probe)
}

fn owners_in(files: &[(&str, &str)], scope: &NoteScope, probe: &[&str]) -> Vec<Option<String>> {
    let store = store_with(files);
    let all: HashSet<VaultPath> = files.iter().map(|(p, _)| vp(p)).collect();
    let dirs: HashSet<VaultPath> = all
        .iter()
        .flat_map(|p| p.as_path().ancestors().skip(1))
        .filter_map(|d| VaultPath::new(d).ok())
        .collect();
    let owners = folder_owners(&store, &all, &dirs, scope);
    probe
        .iter()
        .map(|p| owning_note(&vp(p), scope, |o| owners.contains(o)).map(|o| o.to_string()))
        .collect()
}

#[test]
fn a_typst_note_owns_its_sibling_folder() {
    let got = owners_of(
        &[
            ("portfolios/demo/x.typ", TYPST_NOTE),
            ("portfolios/demo/x/figs/a.png", "png"),
            ("portfolios/demo/x/refs.bib", "@article{}"),
            ("portfolios/demo/x/notes.md", "# Not a note\n"),
        ],
        &["portfolios"],
        &[
            "portfolios/demo/x/figs/a.png",
            "portfolios/demo/x/refs.bib",
            "portfolios/demo/x/notes.md",
        ],
    );
    assert_eq!(got, vec![Some("portfolios/demo/x.typ".to_string()); 3]);
}

#[test]
fn a_concept_folder_can_be_owned_by_a_typst_note() {
    let got = owners_of(
        &[
            ("concepts/bias-variance-split.typ", TYPST_NOTE),
            ("concepts/bias-variance-split/fig.svg", "<svg/>"),
        ],
        &["concepts"],
        &["concepts/bias-variance-split/fig.svg"],
    );
    assert_eq!(
        got,
        vec![Some("concepts/bias-variance-split.typ".to_string())]
    );
}

#[test]
fn an_owner_above_a_declared_type_root_does_not_take_it() {
    // `areas/y.typ` is labelled and `areas/y/z` is a declared type's folder: the notes there
    // stay notes.
    let got = owners_of(
        &[
            ("areas/y.typ", TYPST_NOTE),
            ("areas/y/z/n.typ", TYPST_NOTE),
            ("areas/y/z/n.md", "# A note\n"),
        ],
        &["areas", "areas/y/z"],
        &["areas/y/z/n.typ", "areas/y/z/n.md"],
    );
    assert_eq!(got, vec![None, None]);
}

#[test]
fn an_unlabelled_typ_owns_nothing() {
    // Load-bearing: owning by path shape would drop the folder's notes
    // from the index silently.
    let got = owners_of(
        &[
            ("portfolios/demo/x.typ", TYPST_UNLABELLED),
            ("portfolios/demo/x/notes.md", "# A note\n"),
        ],
        &["portfolios"],
        &["portfolios/demo/x/notes.md"],
    );
    assert_eq!(got, vec![None]);
}

#[test]
fn a_typ_in_a_folder_outside_the_scope_owns_nothing() {
    let got = owners_of(
        &[
            ("concepts/x.typ", TYPST_NOTE),
            ("concepts/x/notes.md", "# A note\n"),
        ],
        &["portfolios"],
        &["concepts/x/notes.md"],
    );
    assert_eq!(got, vec![None]);
}

#[test]
fn a_labelled_file_inside_an_owned_folder_is_never_opened() {
    // Shallow-first: `x/y.typ` is inside the folder `x.typ` owns, so it
    // is not read as a candidate owner of `x/y/`.
    let store = store_with(&[
        ("portfolios/demo/x.typ", TYPST_NOTE),
        ("portfolios/demo/x/y.typ", TYPST_NOTE),
        ("portfolios/demo/x/y/a.png", "png"),
    ]);
    let all: HashSet<VaultPath> = [
        "portfolios/demo/x.typ",
        "portfolios/demo/x/y.typ",
        "portfolios/demo/x/y/a.png",
    ]
    .iter()
    .map(|p| vp(p))
    .collect();
    let dirs: HashSet<VaultPath> = [
        "portfolios/demo",
        "portfolios/demo/x",
        "portfolios/demo/x/y",
    ]
    .iter()
    .map(|p| vp(p))
    .collect();
    let owners = folder_owners(&store, &all, &dirs, &scope_of(&["portfolios"], &[]));
    assert_eq!(
        owners,
        [vp("portfolios/demo/x.typ")]
            .into_iter()
            .collect::<HashSet<_>>(),
    );
}

#[test]
fn a_stub_and_a_typst_note_sharing_a_stem_both_claim_the_folder() {
    // The §5.7 clash. Both are owners; the stub is asked first, so it is
    // the one reported, whichever order the files were written in.
    let got = owners_of(
        &[
            ("portfolios/demo/x.typ", TYPST_NOTE),
            ("portfolios/demo/x.md", STUB_MD),
            ("portfolios/demo/x/a.png", "png"),
        ],
        &["portfolios"],
        &["portfolios/demo/x/a.png"],
    );
    assert_eq!(got, vec![Some("portfolios/demo/x.md".to_string())]);

    let store = store_with(&[
        ("portfolios/demo/x.typ", TYPST_NOTE),
        ("portfolios/demo/x.md", STUB_MD),
    ]);
    let all: HashSet<VaultPath> = ["portfolios/demo/x.typ", "portfolios/demo/x.md"]
        .iter()
        .map(|p| vp(p))
        .collect();
    let dirs: HashSet<VaultPath> = ["portfolios/demo/x"].iter().map(|p| vp(p)).collect();
    assert_eq!(
        folder_owners(&store, &all, &dirs, &scope_of(&["portfolios"], &[])).len(),
        2
    );
}

#[test]
fn a_typ_under_a_nested_excluded_type_owns_nothing() {
    // Deepest wins: `concepts/drafts` is excluded inside admitted `concepts`.
    let scope = scope_of(&["concepts"], &["concepts/drafts"]);
    let got = owners_in(
        &[
            ("concepts/drafts/x.typ", TYPST_NOTE),
            ("concepts/drafts/x/notes.md", "# A note\n"),
            ("concepts/y.typ", TYPST_NOTE),
            ("concepts/y/fig.svg", "<svg/>"),
        ],
        &scope,
        &["concepts/drafts/x/notes.md", "concepts/y/fig.svg"],
    );
    assert_eq!(got, vec![None, Some("concepts/y.typ".to_string())]);
}

#[test]
fn another_types_root_folder_is_never_owned() {
    // `areas/x.typ` is labelled, but `areas/x` is the root of another type.
    for scope in [
        scope_of(&["areas", "areas/x"], &[]),
        scope_of(&["areas"], &["areas/x"]),
    ] {
        let got = owners_in(
            &[
                ("areas/x.typ", TYPST_NOTE),
                ("areas/x/notes.md", "# A note\n"),
            ],
            &scope,
            &["areas/x/notes.md"],
        );
        assert_eq!(got, vec![None]);
    }
}
