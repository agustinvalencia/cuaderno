//! `Catalog` — the note listings over a store alone (#1007).
//!
//! `Catalog` takes no index, so "it never opens one" holds by construction.
//! What these tests pin is that it lists exactly what `Vault` lists from a
//! freshly reconciled index over the same store and config, including the
//! notes reconciliation leaves out (no `type`, a malformed `tags:`, an
//! `ignore` glob) and a note of a type outside that type's folder, and that it
//! refuses a config `Vault::new` refuses.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, QuestionDomain};
use cdno_domain::{Catalog, Vault};
use chrono::{NaiveDate, NaiveDateTime};

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 7, 13)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()
}

fn today() -> NaiveDate {
    at().date()
}

fn path(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn config() -> VaultConfig {
    VaultConfig {
        ignore: vec!["scratch/**".to_string()],
        ..VaultConfig::default()
    }
}

/// A vault holding one of everything the completers list, written by the
/// verbs, plus hand-made files that test the edges of "what the index
/// holds".
fn seeded_store() -> Arc<dyn VaultStore> {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");

    for title in ["Surrogate model", "Old idea"] {
        vault
            .create_project(at(), title, Context::Work, None)
            .expect("create project");
    }
    vault.park_project(at(), "old-idea").expect("park");
    vault
        .create_portfolio(at(), "Why does the surrogate plateau?", None)
        .expect("create portfolio");
    vault
        .file_evidence(
            at(),
            "why-does-the-surrogate-plateau",
            "A baseline run",
            "projects/surrogate-model",
            "The loss flattens after the first epoch.\n",
        )
        .expect("file evidence");
    vault
        .create_stewardship_flat(at(), "Garden", Context::Personal)
        .expect("flat stewardship");
    vault
        .create_stewardship_expanded(at(), "Health", Context::Personal)
        .expect("expanded stewardship");
    vault
        .create_question(
            at(),
            QuestionDomain::Research,
            "How accurate can a surrogate get?",
        )
        .expect("create question");

    // Hand-made maps, from the one the verb wrote.
    let map = store
        .read_file(&path("projects/surrogate-model.md"))
        .unwrap();
    assert!(map.starts_with("---\n"), "{map}");
    let writes = [
        // Closed, in the archive.
        (
            "projects/_done/2026/finished-thing.md",
            "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2026-04-01\n\
             closed: 2026-07-01\n---\n# Finished thing\n"
                .to_string(),
        ),
        // A project outside the projects folder: the index lists it.
        ("notes/misfiled.md", map.clone()),
        // No `type`: reconciliation refuses it.
        (
            "projects/no-type.md",
            "---\nstatus: active\n---\n\n# No type\n".to_string(),
        ),
        // A malformed `tags:`: reconciliation refuses it.
        (
            "projects/bad-tags.md",
            map.replacen("---\n", "---\ntags: 3\n", 1),
        ),
        // Frontmatter that does not parse.
        (
            "projects/broken.md",
            "---\n: [unclosed\n---\n\n# Broken\n".to_string(),
        ),
        // Matched by the `ignore` glob.
        ("scratch/ignored.md", map),
    ];
    for (p, body) in writes {
        store.write_file(&path(p), &body).unwrap();
    }
    store
}

fn slugs<T>(items: &[(VaultPath, T)]) -> Vec<String> {
    items
        .iter()
        .map(|(p, _)| {
            p.as_path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn a_catalog_lists_what_a_reconciled_vault_lists() {
    let store = seeded_store();
    // A fresh index, reconciled from the files: the "up-to-date vault".
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");
    let catalog = Catalog::new(Arc::clone(&store), config()).expect("Catalog::new");

    let active = catalog.active_projects().unwrap();
    assert_eq!(active, vault.active_projects().unwrap());
    assert_eq!(slugs(&active), ["misfiled", "surrogate-model"]);

    let parked = catalog.parked_projects().unwrap();
    assert_eq!(parked, vault.parked_projects().unwrap());
    assert_eq!(slugs(&parked), ["old-idea"]);

    let closed = catalog.closed_projects().unwrap();
    assert_eq!(closed, vault.closed_projects().unwrap());
    assert_eq!(slugs(&closed), ["finished-thing"]);

    let portfolios = catalog.list_portfolios(today()).unwrap();
    assert_eq!(portfolios, vault.list_portfolios(today()).unwrap());
    assert_eq!(portfolios.len(), 1);
    assert_eq!(portfolios[0].evidence_count, 1);

    let stewardships = catalog.list_stewardships(today()).unwrap();
    assert_eq!(stewardships, vault.list_stewardships(today()).unwrap());
    let names: Vec<&str> = stewardships.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(names, ["garden", "health"]);

    let questions = catalog.list_questions().unwrap();
    assert_eq!(questions, vault.list_questions().unwrap());
    assert_eq!(questions.len(), 1);

    // Every note: the index orders candidates by mtime, so compare as sets.
    let mut indexed: Vec<VaultPath> = vault
        .list_note_candidates()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    indexed.sort_by_cached_key(|p| p.to_string());
    let notes = catalog.note_paths().unwrap();
    assert_eq!(notes, indexed);
    for refused in [
        "projects/no-type.md",
        "projects/bad-tags.md",
        "projects/broken.md",
        "scratch/ignored.md",
    ] {
        assert!(
            !notes.contains(&path(refused)),
            "{refused} is not a note the index holds"
        );
    }
    assert!(notes.contains(&path("notes/misfiled.md")));

    assert_eq!(
        catalog.type_registry().all_names(),
        vault.type_registry().all_names()
    );
}

#[test]
fn a_catalog_over_an_empty_store_lists_nothing() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let catalog = Catalog::new(store, VaultConfig::default()).expect("Catalog::new");
    assert!(catalog.active_projects().unwrap().is_empty());
    assert!(catalog.parked_projects().unwrap().is_empty());
    assert!(catalog.closed_projects().unwrap().is_empty());
    assert!(catalog.list_portfolios(today()).unwrap().is_empty());
    assert!(catalog.list_stewardships(today()).unwrap().is_empty());
    assert!(catalog.list_questions().unwrap().is_empty());
    assert!(catalog.note_paths().unwrap().is_empty());
}

#[test]
fn a_catalog_refuses_a_config_the_vault_refuses() {
    let bad = VaultConfig {
        ignore: vec!["a[".to_string()],
        ..VaultConfig::default()
    };
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    assert!(Vault::new(Arc::clone(&store), index, bad.clone()).is_err());
    let err = Catalog::new(store, bad)
        .err()
        .expect("Catalog::new refuses");
    assert!(matches!(err, DomainError::Config(_)), "{err:?}");
}
