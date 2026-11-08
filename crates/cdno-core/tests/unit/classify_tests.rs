use std::collections::BTreeSet;
use std::sync::Arc;

use cdno_core::classify::{NoteScope, classify};
use cdno_core::config::IgnoreSet;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::{MemoryVaultStore, VaultStore};

const STUB: &str = "---\ntype: evidence\nkind: pdf\ntitle: Error budget\n---\n# Error budget\n";
const NOTE: &str = "---\ntype: evidence\ntitle: Error budget\n---\n# Error budget\n";
const DOCUMENT: &str = "# A filed document, no frontmatter\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn paths(items: &[&str]) -> BTreeSet<VaultPath> {
    items.iter().map(|p| vp(p)).collect()
}

fn store_with(files: &[(&str, &str)]) -> Arc<dyn VaultStore> {
    let store = MemoryVaultStore::new();
    for (path, content) in files {
        store.write_file(&vp(path), content).unwrap();
    }
    Arc::new(store)
}

fn ignoring(glob: &str) -> NoteScope {
    NoteScope::new(IgnoreSet::compile(&[glob.to_string()]).unwrap())
}

#[test]
fn an_ignored_stub_still_owns_its_folder() {
    let stub = "portfolios/surrogate-model/surrogate-error-budget.md";
    let inner = "portfolios/surrogate-model/surrogate-error-budget/appendix.md";
    let store = store_with(&[(stub, STUB), (inner, DOCUMENT)]);

    let classified = classify(&store, &ignoring(stub)).unwrap();

    assert!(classified.notes.is_empty(), "{:?}", classified.notes);
    assert_eq!(classified.artefacts, 1);
    assert_eq!(classified.ignored, 1);
}

#[test]
fn a_note_sharing_a_name_with_a_sibling_folder_owns_nothing() {
    let namesake = "portfolios/surrogate-model/surrogate-error-budget.md";
    let inner = "portfolios/surrogate-model/surrogate-error-budget/bias-variance-split.md";
    let store = store_with(&[(namesake, NOTE), (inner, NOTE)]);

    let classified = classify(&store, &NoteScope::empty()).unwrap();

    assert_eq!(classified.notes, paths(&[namesake, inner]));
    assert_eq!(classified.artefacts, 0);
}

#[test]
fn classify_lists_every_file_outside_cuaderno_dir() {
    let note = "concepts/bias-variance-split.md";
    let pdf = "portfolios/surrogate-model/surrogate-error-budget.pdf";
    let store = store_with(&[
        (note, NOTE),
        (pdf, "%PDF"),
        (".cuaderno/templates/daily.md", NOTE),
        (".cuaderno/config.toml", ""),
    ]);

    let classified = classify(&store, &NoteScope::empty()).unwrap();

    assert_eq!(classified.files, paths(&[note, pdf]));
    assert_eq!(classified.notes, paths(&[note]));
}

#[test]
fn classify_counts_match_reconcile_report() {
    let stub = "portfolios/surrogate-model/surrogate-error-budget.md";
    let store = store_with(&[
        (stub, STUB),
        (
            "portfolios/surrogate-model/surrogate-error-budget/a.md",
            DOCUMENT,
        ),
        (
            "portfolios/surrogate-model/surrogate-error-budget/b.md",
            DOCUMENT,
        ),
        (
            "portfolios/surrogate-model/surrogate-error-budget/c.pdf",
            "%PDF",
        ),
        ("concepts/bias-variance-split.md", NOTE),
        ("README.md", DOCUMENT),
    ]);
    let scope = ignoring("README.md");

    let classified = classify(&store, &scope).unwrap();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let report = reconcile(&store, &index, &scope).unwrap();

    assert_eq!(classified.notes.len(), 2);
    assert_eq!(classified.artefacts, 2);
    assert_eq!(classified.ignored, 1);
    assert_eq!(report.scanned, classified.notes.len());
    assert_eq!(report.artefacts, classified.artefacts);
    assert_eq!(report.ignored, classified.ignored);
}

#[test]
fn an_uppercase_markdown_extension_is_a_note() {
    let store = store_with(&[("concepts/SURROGATE.MD", NOTE)]);

    let classified = classify(&store, &NoteScope::empty()).unwrap();

    assert_eq!(classified.notes, paths(&["concepts/SURROGATE.MD"]));
}

#[test]
fn a_typst_file_is_listed_but_is_no_note_yet() {
    let store = store_with(&[("concepts/bias-variance-split.typ", "#metadata(()) <cdno>\n")]);

    let classified = classify(&store, &NoteScope::empty()).unwrap();

    assert!(classified.notes.is_empty());
    assert_eq!(
        classified.files,
        paths(&["concepts/bias-variance-split.typ"])
    );
    assert_eq!(classified.artefacts + classified.ignored, 0);
}
