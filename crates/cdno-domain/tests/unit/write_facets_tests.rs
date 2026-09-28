//! Domain writers stage a note's `links` and `tags` facets in the same
//! commit as its `notes` row (#646).
//!
//! `VaultTransaction::commit` stamps the file's real mtime and size on the
//! row, so reconcile's fast path skips every note cdno wrote itself. A link
//! or tag added by an in-tool write therefore has to reach the index on the
//! write, or it never does until the file is edited outside cuaderno. These
//! tests check each writer's facets straight after the call, with no
//! reconcile in between. `revise_note` is covered in `revise_tests`.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::config::{CustomNoteType, FieldSpec, FieldType, SchemaExtension, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

const DAILY: &str = "journal/2026/daily/2026-09-27.md";
const CONCEPT: &str = "concepts/woodbury-identity.md";
const OTHER: &str = "concepts/other.md";
const PROJECT: &str = "projects/foo.md";

const CONCEPT_NOTE: &str = "---\ntype: concept\ncreated: 2026-09-01\norigin: \"[[concepts/seed]]\"\n---\n\n# Woodbury identity\n\nThe inverse of a rank-k correction.\n";
const OTHER_NOTE: &str = "---\ntype: concept\ncreated: 2026-09-01\n---\n\n# Other\n";
const SEED_NOTE: &str = "---\ntype: concept\ncreated: 2026-09-01\n---\n\n# Seed\n";
const PROJECT_NOTE: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 27)
        .unwrap()
        .and_hms_opt(14, 32, 0)
        .unwrap()
}

/// `concept` declared as a custom type with a settable `origin` string, so
/// `set_frontmatter` can rewrite a wikilink-carrying field.
fn config() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec!["tags".to_owned(), "origin".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let mut schema = SchemaExtension::default();
    schema.fields.insert(
        "origin".to_owned(),
        FieldSpec {
            ty: FieldType::String,
            default: None,
            required: false,
            values: None,
            list: None,
            settable: Some(true),
            log_on_change: None,
        },
    );
    config.schemas.insert("concept".to_owned(), schema);
    config
}

fn vault() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    for (path, content) in [
        (CONCEPT, CONCEPT_NOTE),
        (OTHER, OTHER_NOTE),
        ("concepts/seed.md", SEED_NOTE),
        (PROJECT, PROJECT_NOTE),
    ] {
        store.write_file(&vp(path), content).unwrap();
    }
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config()).expect("Vault::new");
    (vault, store, index)
}

fn backlinks(vault: &Vault, path: &str) -> Vec<VaultPath> {
    vault.read_note(&vp(path)).unwrap().backlinks
}

#[test]
fn set_frontmatter_on_a_wikilink_field_moves_the_backlink() {
    let (vault, _store, _index) = vault();
    assert!(backlinks(&vault, "concepts/seed.md").contains(&vp(CONCEPT)));
    assert!(!backlinks(&vault, OTHER).contains(&vp(CONCEPT)));

    vault
        .set_frontmatter(at(), CONCEPT, "origin", "[[concepts/other]]")
        .expect("set origin");

    assert!(
        backlinks(&vault, OTHER).contains(&vp(CONCEPT)),
        "the new origin is a backlink on write"
    );
    assert!(
        !backlinks(&vault, "concepts/seed.md").contains(&vp(CONCEPT)),
        "the replaced origin's edge is gone"
    );
}

#[test]
fn update_project_state_with_a_link_is_a_backlink_without_reconcile() {
    let (vault, _store, _index) = vault();

    vault
        .update_project_state(at(), "foo", "Reading up on [[concepts/other]].")
        .expect("update state");

    let back = backlinks(&vault, OTHER);
    assert!(back.contains(&vp(PROJECT)), "project map links: {back:?}");
    // The `state on` log entry carries the new state into the daily note,
    // so the daily's facets were staged too.
    assert!(back.contains(&vp(DAILY)), "daily log links: {back:?}");
}

#[test]
fn create_custom_note_with_origin_links_both_ways_on_write() {
    let (vault, _store, index) = vault();
    vault
        .note_to_daily(at(), "Woodbury identity", "Cheap low-rank update.")
        .expect("note_to_daily");

    let path = vault
        .create_custom_note_with_vars(
            at(),
            "concept",
            "Sherman Morrison",
            &HashMap::new(),
            &HashMap::new(),
            None,
            Some("[[journal/2026/daily/2026-09-27#Woodbury identity]]"),
        )
        .expect("create concept");
    assert_eq!(path, vp("concepts/sherman-morrison.md"));

    // The concept's `origin:` resolves to the daily note (anchor stripped).
    assert!(
        backlinks(&vault, DAILY).contains(&path),
        "origin is a backlink on the daily note"
    );
    // The `concept created [[…]]` log line links the note created in the
    // same commit, so the path set the link resolves against includes it.
    let from_daily = index.find_outgoing_links(&vp(DAILY)).unwrap();
    assert!(
        from_daily
            .iter()
            .any(|l| l.resolved_path.as_ref() == Some(&path)),
        "the creation log line resolves to the new note: {from_daily:?}"
    );
}

#[test]
fn note_to_daily_stages_the_pointer_and_body_links() {
    let (vault, _store, index) = vault();

    vault
        .note_to_daily(
            at(),
            "Woodbury identity",
            "Cheap low-rank update; used on [[projects/foo]].",
        )
        .expect("note_to_daily");

    let links = index.find_outgoing_links(&vp(DAILY)).unwrap();
    // The pointer line links the daily note's own `## Notes` entry.
    assert!(
        links.iter().any(|l| {
            l.target_raw == "journal/2026/daily/2026-09-27#Woodbury identity"
                && l.resolved_path.as_ref() == Some(&vp(DAILY))
        }),
        "self pointer staged: {links:?}"
    );
    // And the body's link, carried by both the entry and the pointer.
    assert!(
        backlinks(&vault, PROJECT).contains(&vp(DAILY)),
        "body link staged: {links:?}"
    );
}
