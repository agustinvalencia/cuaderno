//! Tests for `Vault::revise_note` (RFC 0002 §5.3, §6.2): the logged,
//! hash-guarded in-place edit for mutable custom notes.

use std::sync::{Arc, Mutex};

use cdno_core::config::{CustomNoteType, VaultConfig};
use cdno_core::error::{ManipulationError, StoreError};
use cdno_core::file_meta::FileMeta;
use cdno_core::frontmatter::parse_note_spanned;
use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::Revision;
use chrono::{NaiveDate, NaiveDateTime};

use super::support::FailingStore;

const CONCEPT_PATH: &str = "concepts/woodbury-identity.md";
const DAILY_PATH: &str = "journal/2026/daily/2026-09-27.md";

const FRONTMATTER: &str = "---\ntype: concept\ncreated: 2026-09-01\ntags: [linear-algebra]\norigin: \"[[journal/2026/daily/2026-09-01]]\"\n---\n";

const BODY: &str = "\n# Woodbury identity\n\n## Statement\n\nThe inverse of a rank-k correction.\n\n## Why it matters\n\nCheap updates.\n\n## See also\n\n- [[concepts/sherman-morrison]]\n";

fn concept_note() -> String {
    format!("{FRONTMATTER}{BODY}")
}

const PERSON_NOTE: &str =
    "---\ntype: person\ncreated: 2026-09-01\n---\n\n# Ada\n\n## Log\n\nMet.\n";

const ACTIVE_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 27)
        .unwrap()
        .and_hms_opt(14, 32, 0)
        .unwrap()
}

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
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![],
            template: None,
            append_only: true,
            title_field: None,
            date_field: None,
        },
    );
    config
}

fn seed(store: &dyn VaultStore) {
    store
        .write_file(&vp(CONCEPT_PATH), &concept_note())
        .unwrap();
    store.write_file(&vp("people/ada.md"), PERSON_NOTE).unwrap();
    store
        .write_file(&vp("projects/foo.md"), ACTIVE_PROJECT)
        .unwrap();
}

fn vault() -> (Vault, Arc<dyn VaultStore>) {
    let (vault, store, _index) = vault_with_index();
    (vault, store)
}

/// As [`vault`], keeping a handle on the index so a test can observe the
/// index half of the transaction.
fn vault_with_index() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    seed(&*store);
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config()).expect("Vault::new");
    (vault, store, index)
}

/// `content_hash` on the index row for `path`.
fn indexed_hash(index: &dyn VaultIndex, path: &VaultPath) -> String {
    index
        .find_by_path(path)
        .expect("index query")
        .expect("index row")
        .content_hash
}

/// A vault whose concept note at [`CONCEPT_PATH`] has `body` after the
/// usual frontmatter.
fn vault_with_concept_body(body: &str) -> (Vault, Arc<dyn VaultStore>) {
    let (vault, store) = vault();
    store
        .write_file(&vp(CONCEPT_PATH), &format!("{FRONTMATTER}{body}"))
        .unwrap();
    (vault, store)
}

fn section(heading: &str, content: &str) -> Revision {
    Revision::Section {
        heading: heading.to_owned(),
        content: content.to_owned(),
    }
}

/// The `## Logs` lines of today's daily note that record a revision.
fn revised_lines(store: &dyn VaultStore) -> Vec<String> {
    match store.read_file(&vp(DAILY_PATH)) {
        Ok(daily) => daily
            .lines()
            .filter(|l| l.contains("revised [["))
            .map(str::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn body_revision_logs_one_revised_line() {
    let (vault, store, index) = vault_with_index();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let new_body = "\n# Woodbury identity\n\n## Statement\n\nA better account.\n";

    let outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body(new_body.to_owned()),
            "rewrote the\n  statement",
            at(),
        )
        .expect("revise");

    let written = store.read_file(&path).unwrap();
    // Frontmatter preserved byte-for-byte; the body is exactly what was given.
    assert_eq!(written, format!("{FRONTMATTER}{new_body}"));
    assert!(outcome.changed);
    assert_eq!(outcome.new_hash, content_hash(&written));
    assert_eq!(
        outcome.log_line.as_deref(),
        Some("revised [[concepts/woodbury-identity]] \u{2014} rewrote the statement")
    );
    // A whole-body revision has no anchored target.
    assert_eq!(outcome.section_target, None);
    assert_eq!(
        revised_lines(&*store),
        vec![
            "- **14:32**: revised [[concepts/woodbury-identity]] \u{2014} rewrote the statement"
                .to_owned()
        ]
    );
    // The hash `read_note` now reports is the one the outcome returned.
    assert_eq!(
        vault.read_note(&path).unwrap().content_hash,
        outcome.new_hash
    );
    // The index row is part of the same commit.
    assert_eq!(indexed_hash(&*index, &path), outcome.new_hash);
}

#[test]
fn section_revision_logs_the_anchored_form() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();

    let outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Why it matters".to_owned(),
                content: "Cheap updates: O(k^3) instead of O(n^3).".to_owned(),
            },
            "added the cost comparison",
            at(),
        )
        .expect("revise");

    let written = store.read_file(&path).unwrap();
    assert_eq!(
        written,
        concept_note().replace(
            "Cheap updates.\n",
            "Cheap updates: O(k^3) instead of O(n^3).\n"
        )
    );
    assert!(outcome.changed);
    assert_eq!(
        revised_lines(&*store),
        vec![
            "- **14:32**: revised [[concepts/woodbury-identity#Why it matters]] \u{2014} added the cost comparison"
                .to_owned()
        ]
    );
}

#[test]
fn identical_revision_writes_nothing_and_logs_nothing() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();

    let body_outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body(view.body.clone()),
            "no change",
            at(),
        )
        .expect("revise body");
    let section_outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "The inverse of a rank-k correction.".to_owned(),
            },
            "no change",
            at(),
        )
        .expect("revise section");

    for outcome in [body_outcome, section_outcome] {
        assert!(!outcome.changed);
        assert_eq!(outcome.log_line, None);
        assert_eq!(outcome.section_target, None);
        assert_eq!(outcome.new_hash, view.content_hash);
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(
        !store.exists(&vp(DAILY_PATH)).unwrap(),
        "an identical revision must not create or touch the daily note"
    );
}

#[test]
fn stale_hash_is_refused_and_file_is_untouched() {
    let (vault, store, index) = vault_with_index();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let hash_before = indexed_hash(&*index, &path);

    // An editor saves in between.
    let edited = concept_note().replace("Cheap updates.", "Edited elsewhere.");
    store.write_file(&path, &edited).unwrap();

    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body("\n# Clobber\n".to_owned()),
            "would lose the edit",
            at(),
        )
        .unwrap_err();

    match err {
        DomainError::StaleRevision {
            path: p,
            expected,
            actual,
        } => {
            assert_eq!(p, CONCEPT_PATH);
            assert_eq!(expected, view.content_hash);
            assert_eq!(actual, content_hash(&edited));
        }
        other => panic!("expected StaleRevision, got {other:?}"),
    }
    assert_eq!(store.read_file(&path).unwrap(), edited);
    assert!(revised_lines(&*store).is_empty());
    assert_eq!(
        indexed_hash(&*index, &path),
        hash_before,
        "a refused revision must not touch the index row"
    );
}

#[test]
fn none_hash_skips_the_check() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let edited = concept_note().replace("Cheap updates.", "Edited elsewhere.");
    store.write_file(&path, &edited).unwrap();

    let outcome = vault
        .revise_note(
            &path,
            None,
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "Restated.".to_owned(),
            },
            "restated",
            at(),
        )
        .expect("revise without a hash");

    assert!(outcome.changed);
    let written = store.read_file(&path).unwrap();
    assert!(written.contains("Restated."), "{written}");
    assert!(written.contains("Edited elsewhere."), "{written}");
    assert_eq!(revised_lines(&*store).len(), 1);
}

#[test]
fn builtin_type_is_refused() {
    let (vault, store) = vault();
    let path = vp("projects/foo.md");
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Section {
                heading: "Current State".to_owned(),
                content: "Bypassing the state log.".to_owned(),
            },
            "sneaky",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRevisable { path, .. } if path == "projects/foo.md"),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), ACTIVE_PROJECT);
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn append_only_custom_type_is_refused() {
    let (vault, store) = vault();
    let path = vp("people/ada.md");
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Body("\n# Ada\n".to_owned()),
            "rewrite history",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRevisable { reason, .. } if reason.contains("append-only")),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), PERSON_NOTE);
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn empty_reason_is_refused() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    for reason in ["", "  \n\t "] {
        let err = vault
            .revise_note(
                &path,
                None,
                Revision::Body("\n# Changed\n".to_owned()),
                reason,
                at(),
            )
            .unwrap_err();
        assert!(
            matches!(err, DomainError::EmptyField { field: "reason" }),
            "got {err:?}"
        );
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn missing_section_is_upserted_as_a_level_two_section_at_the_end() {
    let (vault, store, index) = vault_with_index();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            section("Proof", "By expansion."),
            "added a proof",
            at(),
        )
        .expect("a missing section is an upsert");

    assert!(outcome.changed);
    assert_eq!(
        store.read_file(&path).unwrap(),
        format!("{}\n## Proof\n\nBy expansion.\n", concept_note())
    );
    assert_eq!(
        outcome.log_line.as_deref(),
        Some("revised [[concepts/woodbury-identity#Proof]] \u{2014} added a proof")
    );
    // The outcome carries the anchored target itself, so no caller has to
    // parse it back out of the log line.
    assert_eq!(
        outcome.section_target.as_deref(),
        Some("concepts/woodbury-identity#Proof")
    );
    assert_eq!(revised_lines(&*store).len(), 1);
    assert_eq!(indexed_hash(&*index, &path), outcome.new_hash);
}

#[test]
fn ambiguous_section_is_refused_and_nothing_is_written() {
    let body = "\n# X\n\n## Notes\n\nOne.\n\n## Notes\n\nTwo.\n";
    let (vault, store) = vault_with_concept_body(body);
    let path = vp(CONCEPT_PATH);
    let err = vault
        .revise_note(&path, None, section("Notes", "Three."), "which one", at())
        .unwrap_err();
    assert!(
        matches!(
            &err,
            DomainError::Manipulation(ManipulationError::AmbiguousSection(h)) if h == "Notes"
        ),
        "got {err:?}"
    );
    assert_eq!(
        store.read_file(&path).unwrap(),
        format!("{FRONTMATTER}{body}")
    );
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn unknown_note_type_is_refused() {
    let (vault, store) = vault();
    let path = vp("concepts/stray.md");
    let note = "---\ntype: widget\ncreated: 2026-09-01\n---\n\n# Stray\n";
    store.write_file(&path, note).unwrap();
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Body("\n# Changed\n".to_owned()),
            "unknown type",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::UnknownNoteType { note_type } if note_type == "widget"),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), note);
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn section_heading_must_be_a_valid_anchor() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    for heading in [
        "A | B",
        "Uses of [[x]]",
        "",
        "  ",
        "Two\nlines",
        "a#b",
        "a]b",
        "^block",
    ] {
        let err = vault
            .revise_note(&path, None, section(heading, "Text."), "anchor", at())
            .unwrap_err();
        assert!(
            matches!(err, DomainError::RevisionInvalid { .. }),
            "heading {heading:?}: got {err:?}"
        );
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn section_content_may_not_add_a_heading_at_or_above_the_section_level() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let refused = [
        // Existing level-2 section: a same-level heading would split it.
        ("Why it matters", "x\n\n## Statement\n\ndup"),
        ("Why it matters", "# Top"),
        ("Why it matters", "Setext\n------"),
        // A new section is level 2, so the same rule applies to it.
        ("Proof", "x\n\n## Lemma"),
    ];
    for (heading, content) in refused {
        let err = vault
            .revise_note(&path, None, section(heading, content), "restructure", at())
            .unwrap_err();
        assert!(
            matches!(err, DomainError::RevisionInvalid { .. }),
            "{heading:?} / {content:?}: got {err:?}"
        );
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());

    // Deeper headings, and `#` lines inside fenced code, are content.
    let allowed = "Cheap updates.\n\n### Cost\n\nO(k^3).\n\n```\n## not a heading\n```";
    let outcome = vault
        .revise_note(
            &path,
            None,
            section("Why it matters", allowed),
            "deeper",
            at(),
        )
        .expect("deeper headings are allowed");
    assert!(outcome.changed);
    let headings = vault.read_note(&path).unwrap().headings;
    assert_eq!(
        headings,
        vec![
            "Woodbury identity",
            "Statement",
            "Why it matters",
            "Cost",
            "See also"
        ]
    );
}

#[test]
fn section_revision_replaces_its_sub_sections() {
    // Documented, not refused: a section runs to the next heading of
    // equal or higher level, so a level-1 section owns every `##` below.
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    vault
        .revise_note(
            &path,
            None,
            section("Woodbury identity", "Intro."),
            "collapsed",
            at(),
        )
        .expect("revise");
    assert_eq!(
        store.read_file(&path).unwrap(),
        format!("{FRONTMATTER}\n# Woodbury identity\n\nIntro.\n")
    );
}

#[test]
fn stale_hash_is_reported_before_the_frontmatter_is_parsed() {
    // A concurrent edit that broke the frontmatter must read as
    // "changed since read", which the caller can act on, not as a
    // parse error.
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let broken = "---\ntype: [unclosed\n---\n\n# Broken\n";
    store.write_file(&path, broken).unwrap();

    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            section("Statement", "Restated."),
            "restated",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::StaleRevision { .. }),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), broken);
}

#[test]
fn filling_a_blank_last_section_leaves_one_blank_line_after_the_heading() {
    for body in [
        // No final newline: the heading line itself is unterminated.
        "\n# X\n\n## A",
        "\n# X\n\n## A\n",
    ] {
        let (vault, store) = vault_with_concept_body(body);
        let path = vp(CONCEPT_PATH);
        vault
            .revise_note(&path, None, section("A", "filled"), "filled", at())
            .expect("revise");
        assert_eq!(
            store.read_file(&path).unwrap(),
            format!("{FRONTMATTER}\n# X\n\n## A\n\nfilled\n"),
            "body {body:?}"
        );
    }
}

#[test]
fn clearing_a_section_leaves_one_blank_line_before_the_next_heading() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    vault
        .revise_note(&path, None, section("Why it matters", ""), "cleared", at())
        .expect("revise");
    assert_eq!(
        store.read_file(&path).unwrap(),
        concept_note().replace(
            "## Why it matters\n\nCheap updates.\n\n",
            "## Why it matters\n\n"
        )
    );
}

#[test]
fn clearing_the_last_section_leaves_no_trailing_blank_line() {
    // Pins the trailing-blank-line fixup in `revise_note`: without it the
    // note would end `## See also\n\n`, a blank line the original did not
    // end with.
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    vault
        .revise_note(&path, None, section("See also", ""), "cleared", at())
        .expect("revise");
    assert_eq!(
        store.read_file(&path).unwrap(),
        concept_note().replace(
            "## See also\n\n- [[concepts/sherman-morrison]]\n",
            "## See also\n"
        )
    );
}

#[test]
fn revision_and_log_line_are_one_commit() {
    // The commit writes the note first, then the daily note. Failing
    // the second write must roll the note back.
    let backing = Arc::new(MemoryVaultStore::new());
    seed(&*backing);
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(Arc::clone(&backing), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");

    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body("\n# Woodbury identity\n\nShorter.\n".to_owned()),
            "shortened",
            at(),
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::Transaction(_)), "got {err:?}");
    assert_eq!(backing.read_file(&path).unwrap(), concept_note());
    assert!(!backing.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn hash_compare_happens_under_the_lock() {
    // Simulates an editor whose save lands after the caller read the
    // note but before `revise_note` takes the vault write lock: the
    // store performs the edit at the moment the lock is acquired. With
    // the read + compare under the lock, the revision sees the edited
    // bytes and is refused. Were the compare done before taking the
    // lock, it would pass on the stale bytes and the revision would
    // overwrite the editor's save.
    let backing = Arc::new(MemoryVaultStore::new());
    seed(&*backing);
    let racing = Arc::new(EditorRaceStore::new(Arc::clone(&backing)));
    let store: Arc<dyn VaultStore> = racing.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");

    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let edited = concept_note().replace("Cheap updates.", "Saved by the editor.");
    racing.save_on_next_lock(path.clone(), edited.clone());

    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "Restated.".to_owned(),
            },
            "restated",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::StaleRevision { .. }),
        "got {err:?}"
    );
    assert_eq!(backing.read_file(&path).unwrap(), edited);
    assert!(!backing.exists(&vp(DAILY_PATH)).unwrap());
}

/// A store that performs one pending write the moment the vault write
/// lock is next acquired — a concurrent editor's save racing the
/// caller's read-then-revise.
struct EditorRaceStore {
    inner: Arc<MemoryVaultStore>,
    pending: Mutex<Option<(VaultPath, String)>>,
}

impl EditorRaceStore {
    fn new(inner: Arc<MemoryVaultStore>) -> Self {
        Self {
            inner,
            pending: Mutex::new(None),
        }
    }

    fn save_on_next_lock(&self, path: VaultPath, content: String) {
        *self.pending.lock().unwrap() = Some((path, content));
    }
}

impl VaultStore for EditorRaceStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        if let Some((path, content)) = self.pending.lock().unwrap().take() {
            self.inner.write_file(&path, &content)?;
        }
        self.inner.acquire_write_lock()
    }
}

// ---------------------------------------------------------------------
// Link and tag facets are staged on the write (#646)
// ---------------------------------------------------------------------

const OTHER_PATH: &str = "concepts/other.md";

/// A vault seeded as [`vault_with_index`] plus a second concept note at
/// [`OTHER_PATH`] for the revised note to link to.
fn vault_with_other_concept() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    seed(&*store);
    store
        .write_file(
            &vp(OTHER_PATH),
            "---\ntype: concept\ncreated: 2026-09-01\n---\n\n# Other\n",
        )
        .unwrap();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config()).expect("Vault::new");
    (vault, store, index)
}

#[test]
fn revision_adding_a_link_is_a_backlink_without_reconcile() {
    let (vault, _store, _index) = vault_with_other_concept();
    let path = vp(CONCEPT_PATH);
    let other = vp(OTHER_PATH);
    assert!(
        !vault.read_note(&other).unwrap().backlinks.contains(&path),
        "no edge before the revision"
    );
    let view = vault.read_note(&path).unwrap();

    vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "See also".to_owned(),
                content: "- [[concepts/other]]".to_owned(),
            },
            "linked the other concept",
            at(),
        )
        .expect("revise");

    let backlinks = vault.read_note(&other).unwrap().backlinks;
    assert!(
        backlinks.contains(&path),
        "the revised note links to the other concept: {backlinks:?}"
    );
}

#[test]
fn revision_adding_an_inline_tag_lands_in_the_tags_facet() {
    let (vault, _store, index) = vault_with_index();
    let path = vp(CONCEPT_PATH);
    assert!(!index.find_by_tag("concept").unwrap().contains(&path));
    let view = vault.read_note(&path).unwrap();

    vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Why it matters".to_owned(),
                content: "Cheap updates. #concept".to_owned(),
            },
            "tagged it",
            at(),
        )
        .expect("revise");

    assert!(
        index.find_by_tag("concept").unwrap().contains(&path),
        "the body tag is indexed on write"
    );
    // The frontmatter `tags:` list still merges in.
    assert!(index.find_by_tag("linear-algebra").unwrap().contains(&path));
}

// ---------------------------------------------------------------------
// Typst notes (RFC 0008 §5.8)
// ---------------------------------------------------------------------

const TYPST_PATH: &str = "concepts/bias-variance-split.typ";
const TYPST_BLOCK: &str = "#metadata((type: \"concept\", title: \"Bias-variance split\", created: \"2026-09-01\")) <cdno>";
const TYPST_SEPARATOR: &str = "\n\n";
const TYPST_BODY: &str = "= Statement\n\nThe error splits as $ sigma^2 $.\n";

fn typst_note() -> String {
    format!("// preamble\n{TYPST_BLOCK}{TYPST_SEPARATOR}{TYPST_BODY}")
}

fn seeded_vault(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    seed(&*store);
    for (path, content) in notes {
        store.write_file(&vp(path), content).unwrap();
    }
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config()).expect("Vault::new");
    (vault, store)
}

fn typst_vault() -> (Vault, Arc<dyn VaultStore>) {
    seeded_vault(&[(TYPST_PATH, &typst_note())])
}

fn revise_typst_body(
    vault: &Vault,
    body: &str,
) -> Result<cdno_domain::vault::ReviseOutcome, DomainError> {
    let hash = vault.read_note(&vp(TYPST_PATH)).unwrap().content_hash;
    vault.revise_note(
        &vp(TYPST_PATH),
        Some(&hash),
        Revision::Body(body.to_owned()),
        "tightened the statement",
        at(),
    )
}

fn assert_refused_unchanged(store: &dyn VaultStore) {
    assert_eq!(store.read_file(&vp(TYPST_PATH)).unwrap(), typst_note());
    assert!(revised_lines(store).is_empty());
}

#[test]
fn whole_body_revision_of_a_typst_concept_keeps_the_block_byte_for_byte() {
    let (vault, store) = typst_vault();
    let before = store.read_file(&vp(TYPST_PATH)).unwrap();
    let span_end = parse_note_spanned(&vp(TYPST_PATH), &before)
        .unwrap()
        .metadata_span
        .end;
    let new_body = "= Statement\n\nA better account of [[surrogate-model]].\n";

    let outcome = revise_typst_body(&vault, new_body).expect("revise");

    let after = store.read_file(&vp(TYPST_PATH)).unwrap();
    assert_eq!(after[..span_end], before[..span_end]);
    assert_eq!(after[span_end..], format!("{TYPST_SEPARATOR}{new_body}"));
    assert!(outcome.changed);
    assert_eq!(outcome.new_hash, content_hash(&after));
    assert_eq!(
        vault.read_note(&vp(TYPST_PATH)).unwrap().content_hash,
        outcome.new_hash
    );
}

#[test]
fn whole_body_revision_logs_revised_with_the_address_and_no_extension() {
    let (vault, store) = typst_vault();

    let outcome = revise_typst_body(&vault, "= Statement\n\nRevised.\n").expect("revise");

    let line = "revised [[concepts/bias-variance-split]] \u{2014} tightened the statement";
    assert_eq!(outcome.log_line.as_deref(), Some(line));
    assert_eq!(outcome.section_target, None);
    assert_eq!(revised_lines(&*store), vec![format!("- **14:32**: {line}")]);
}

#[test]
fn a_new_body_carrying_the_label_is_refused() {
    let (vault, store) = typst_vault();

    let err = revise_typst_body(&vault, "#metadata((type: \"concept\")) <cdno>\n\nText.\n")
        .expect_err("refused");

    assert!(
        matches!(err, DomainError::BodyCarriesMetadataBlock),
        "{err:?}"
    );
    assert_refused_unchanged(&*store);
}

#[test]
fn a_new_body_that_does_not_parse_is_refused_and_nothing_changes() {
    let (vault, store) = typst_vault();

    let err = revise_typst_body(&vault, "fine\n\n#(unclosed\n").expect_err("refused");

    assert!(
        matches!(err, DomainError::BodyDoesNotParse { line: 3, .. }),
        "{err:?}"
    );
    assert_refused_unchanged(&*store);
}

#[test]
fn section_revision_on_typst_is_refused_with_the_remedy() {
    let (vault, store) = typst_vault();

    let err = vault
        .revise_note(
            &vp(TYPST_PATH),
            None,
            section("Statement", "New."),
            "tightened",
            at(),
        )
        .expect_err("refused");

    assert!(
        matches!(err, DomainError::SectionRevisionUnavailableForTypst),
        "{err:?}"
    );
    let remedy = err.remedy().expect("a remedy");
    assert!(
        remedy.contains("whole body") && remedy.contains("expected_hash"),
        "{remedy}"
    );
    assert_refused_unchanged(&*store);
}

#[test]
fn a_typst_evidence_note_is_refused_as_append_only() {
    let body = "\n\nObserved.\n";
    let typst = "portfolios/surrogate-model/2026-09-01-bias-variance-split.typ";
    let markdown = "portfolios/surrogate-model/2026-09-02-surrogate-error-budget.md";
    let typst_note = format!(
        "#metadata((type: \"evidence\", title: \"Split\", date: \"2026-09-01\")) <cdno>{body}"
    );
    let markdown_note = "---\ntype: evidence\ndate: 2026-09-02\n---\n\nObserved.\n";
    let (vault, store) = seeded_vault(&[(typst, &typst_note), (markdown, markdown_note)]);

    let refuse = |path: &str| {
        vault
            .revise_note(
                &vp(path),
                None,
                Revision::Body("New.\n".into()),
                "why",
                at(),
            )
            .expect_err("refused")
    };
    let (typst_err, markdown_err) = (refuse(typst), refuse(markdown));

    let reason = |e: DomainError| match e {
        DomainError::NoteNotRevisable { reason, .. } => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(reason(typst_err), reason(markdown_err));
    assert_eq!(store.read_file(&vp(typst)).unwrap(), typst_note);
}

#[test]
fn stale_hash_is_refused_before_any_typst_check() {
    let (vault, store) = typst_vault();

    let err = vault
        .revise_note(
            &vp(TYPST_PATH),
            Some("not-the-hash"),
            Revision::Body("#(unclosed\n".to_owned()),
            "why",
            at(),
        )
        .expect_err("refused");

    assert!(matches!(err, DomainError::StaleRevision { .. }), "{err:?}");
    assert_refused_unchanged(&*store);
}

#[test]
fn a_typst_body_sent_back_from_read_note_changes_nothing() {
    let (vault, store) = typst_vault();
    let body = vault.read_note(&vp(TYPST_PATH)).unwrap().body;

    let outcome = revise_typst_body(&vault, &body).expect("revise");

    assert!(!outcome.changed);
    assert_refused_unchanged(&*store);
}

#[test]
fn a_hand_written_typst_body_that_does_not_parse_is_a_no_op_when_sent_back_unchanged() {
    let note = format!("{TYPST_BLOCK}\n\n= Statement\n\nHand edit: #(unclosed\n");
    let (vault, store) = seeded_vault(&[(TYPST_PATH, &note)]);
    let body = vault.read_note(&vp(TYPST_PATH)).unwrap().body;

    let outcome = revise_typst_body(&vault, &body).expect("a no-op is not checked");

    assert!(!outcome.changed);
    assert_eq!(store.read_file(&vp(TYPST_PATH)).unwrap(), note);
    let err = revise_typst_body(&vault, &format!("{body}more #(\n")).expect_err("a change is");
    assert!(
        matches!(err, DomainError::BodyDoesNotParse { .. }),
        "{err:?}"
    );
}

#[test]
fn an_edit_of_the_read_body_adds_no_blank_line_after_the_block() {
    let (vault, store) = typst_vault();
    let body = vault.read_note(&vp(TYPST_PATH)).unwrap().body;
    let edited = body.replace("The error", "The total error");

    revise_typst_body(&vault, &edited).expect("revise");

    assert_eq!(
        store.read_file(&vp(TYPST_PATH)).unwrap(),
        typst_note().replace("The error", "The total error")
    );
}

#[test]
fn a_body_starting_on_the_label_line_still_gets_a_line_of_its_own() {
    let note = format!("{TYPST_BLOCK} // kept\n\nOld.\n");
    let (vault, store) = seeded_vault(&[(TYPST_PATH, &note)]);

    revise_typst_body(&vault, "= Statement\n").expect("revise");

    assert_eq!(
        store.read_file(&vp(TYPST_PATH)).unwrap(),
        format!("{TYPST_BLOCK}\n\n= Statement\n")
    );
}
