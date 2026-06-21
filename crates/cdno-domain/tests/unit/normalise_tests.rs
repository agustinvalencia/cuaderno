//! Tests for frontmatter normalisation (#233): `Vault::normalise_notes`
//! reorders frontmatter to the canonical per-type key order, and the
//! canonical order stays in sync with the creation templates.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::note_type::NoteType;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn vault_with_notes(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

/// The top-level frontmatter keys of a note, in order.
fn frontmatter_keys(raw: &str) -> Vec<String> {
    raw.split("---")
        .nth(1)
        .expect("frontmatter block")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(char::is_whitespace) && l.contains(':'))
        .map(|l| l.split(':').next().unwrap().trim().to_owned())
        .collect()
}

// `status`/`created`/`type`/`context` scrambled, plus a hand-added key
// the normaliser doesn't know about.
const SCRAMBLED_PROJECT: &str = "---\nstatus: active\ncreated: 2026-04-01\ntype: project\ncontext: work\nextra: keepme\ncore_question: \"[[questions/q]]\"\n---\n# Foo\n\n## Current State\n";

#[test]
fn normalise_reorders_to_canonical_and_keeps_unknown_keys_last() {
    let (vault, store) = vault_with_notes(&[("projects/foo.md", SCRAMBLED_PROJECT)]);

    let report = vault.normalise_notes(false).expect("normalise");
    assert_eq!(report.changed, vec![vp("projects/foo.md")]);

    let out = store.read_file(&vp("projects/foo.md")).unwrap();
    assert_eq!(
        frontmatter_keys(&out),
        // canonical project order, then the unknown key appended
        vec![
            "type",
            "context",
            "status",
            "created",
            "core_question",
            "extra"
        ]
    );
    // Values are preserved verbatim (quoting kept), as is the body.
    assert!(out.contains("core_question: \"[[questions/q]]\""), "{out}");
    assert!(out.contains("extra: keepme"), "{out}");
    assert!(out.contains("## Current State"), "{out}");
}

#[test]
fn normalise_is_idempotent() {
    let (vault, store) = vault_with_notes(&[("projects/foo.md", SCRAMBLED_PROJECT)]);
    vault.normalise_notes(false).unwrap();
    let once = store.read_file(&vp("projects/foo.md")).unwrap();

    let report = vault.normalise_notes(false).unwrap();
    assert!(
        report.changed.is_empty(),
        "an already-canonical note is not changed again"
    );
    assert_eq!(store.read_file(&vp("projects/foo.md")).unwrap(), once);
}

#[test]
fn normalise_dry_run_reports_without_writing() {
    let (vault, store) = vault_with_notes(&[("projects/foo.md", SCRAMBLED_PROJECT)]);
    let before = store.read_file(&vp("projects/foo.md")).unwrap();

    let report = vault.normalise_notes(true).expect("dry run");
    assert_eq!(report.changed, vec![vp("projects/foo.md")]);
    assert_eq!(
        store.read_file(&vp("projects/foo.md")).unwrap(),
        before,
        "dry run must not write"
    );
}

#[test]
fn normalise_moves_a_multiline_value_as_a_unit() {
    // A block list value: its continuation lines must travel with the
    // `tags:` key when it's reordered to the end of the action order.
    let note = "---\nstatus: active\ntype: action\nproject: foo\ntags:\n  - urgent\n  - review\ncreated: 2026-04-01\n---\n# A\n";
    let (vault, store) = vault_with_notes(&[("actions/a.md", note)]);

    vault.normalise_notes(false).unwrap();
    let out = store.read_file(&vp("actions/a.md")).unwrap();

    assert_eq!(
        frontmatter_keys(&out),
        vec!["type", "status", "project", "created", "tags"]
    );
    assert!(
        out.contains("tags:\n  - urgent\n  - review"),
        "the list value moved intact:\n{out}"
    );
}

#[test]
fn normalise_skips_a_note_with_an_unknown_type() {
    let note = "---\ntype: nonsense\nb: 2\na: 1\n---\n# x\n";
    let (vault, store) = vault_with_notes(&[("inbox/x.md", note)]);
    let before = store.read_file(&vp("inbox/x.md")).unwrap();

    let report = vault.normalise_notes(false).unwrap();
    assert!(report.changed.is_empty(), "unknown type is left alone");
    assert_eq!(store.read_file(&vp("inbox/x.md")).unwrap(), before);
}

#[test]
fn canonical_frontmatter_order_matches_the_templates() {
    // The templates are the source of truth for field order; this pins
    // `NoteType::frontmatter_order` to them so they can't drift.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");
    let cases = [
        (NoteType::Action, "action.md"),
        (NoteType::Commitment, "commitment.md"),
        (NoteType::Evidence, "evidence.md"),
        (NoteType::Portfolio, "portfolio.md"),
        (NoteType::Project, "project.md"),
        (NoteType::Question, "question.md"),
        (NoteType::Stewardship, "stewardship.md"),
    ];
    for (note_type, file) in cases {
        let raw = std::fs::read_to_string(format!("{dir}/{file}"))
            .unwrap_or_else(|e| panic!("read template {file}: {e}"));
        assert_eq!(
            frontmatter_keys(&raw),
            note_type.frontmatter_order(),
            "template {file} frontmatter order drifted from NoteType::frontmatter_order"
        );
    }
}

#[test]
fn fresh_weekly_scaffold_matches_canonical_frontmatter_order() {
    // The weekly scaffold lives in code (`weekly.rs`), not a template
    // file — pin the note it produces to `NoteType::Weekly`'s order.
    use cdno_domain::WeeklySection;
    use chrono::NaiveDate;

    let (vault, store) = vault_with_notes(&[]);
    let date = NaiveDate::from_ymd_opt(2026, 4, 26).unwrap();
    let path = vault
        .upsert_weekly_section(date, WeeklySection::Wins, "shipped", false)
        .expect("create weekly note");
    let content = store.read_file(&path).unwrap();

    assert_eq!(
        frontmatter_keys(&content),
        NoteType::Weekly.frontmatter_order(),
        "weekly scaffold drifted from NoteType::Weekly::frontmatter_order:\n{content}"
    );
}

#[test]
fn fresh_inbox_scaffold_matches_canonical_frontmatter_order() {
    // The inbox capture scaffold lives in code (`capture.rs`).
    use chrono::{NaiveDate, NaiveTime};

    let (vault, store) = vault_with_notes(&[]);
    let at = NaiveDate::from_ymd_opt(2026, 4, 26)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    let path = vault
        .capture_to_inbox(at, "a fleeting thought")
        .expect("capture");
    let content = store.read_file(&path).unwrap();

    assert_eq!(
        frontmatter_keys(&content),
        NoteType::Inbox.frontmatter_order(),
        "inbox scaffold drifted from NoteType::Inbox::frontmatter_order:\n{content}"
    );
}
