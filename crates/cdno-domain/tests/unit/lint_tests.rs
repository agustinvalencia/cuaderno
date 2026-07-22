use std::sync::Arc;

use cdno_core::config::{CustomNoteType, SchemaExtension, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// A config registering a `person` custom type (folder `people`, required
/// `name`, optional `role`) for the custom-type lint tests.
fn config_with_person() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec!["name".to_owned()],
            optional: vec!["role".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    config
}

/// Build a vault containing the given `(path, body)` notes. Reconciliation
/// runs as part of `Vault::new` so the index reflects the seeded files.
fn vault_with_notes(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new succeeded");
    vault
}

#[test]
fn lint_returns_empty_report_for_empty_vault() {
    let vault = vault_with_notes(&[], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_skips_an_ignored_file() {
    // A note that lint would otherwise flag (unknown type, see
    // `lint_flags_a_note_with_an_unknown_type` for the un-ignored
    // counterpart). With its path in `ignore` it never enters the index,
    // so lint never sees it — proving the config `ignore` exclusion
    // reaches lint, not just the reconciler.
    let body = "---\ntype: bogus\ntitle: Mystery\n---\n# Body\n";
    let config = VaultConfig {
        ignore: vec!["scratch.md".to_string()],
        ..Default::default()
    };
    let vault = vault_with_notes(&[("scratch.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(
        report.is_clean(),
        "an ignored file must not be linted: {:?}",
        report.issues
    );
}

#[test]
fn lint_passes_for_a_valid_note_with_a_known_type() {
    let body = "---\ntype: daily\ntitle: A clean note\n---\n# Body\n";
    let vault = vault_with_notes(&[("note.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_flags_a_note_with_an_unknown_type() {
    let body = "---\ntype: bogus\ntitle: Mystery\n---\n# Body\n";
    let vault = vault_with_notes(&[("strange.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].path, vp("strange.md"));
    assert!(
        report.issues[0].message.contains("unknown note type"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_flags_a_missing_extra_required_field() {
    let body = "---\ntype: project\ntitle: A project without an owner\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
            ..Default::default()
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].path, vp("projects/foo.md"));
    assert!(
        report.issues[0]
            .message
            .contains("missing required field `owner`"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_passes_when_extra_required_field_is_present() {
    let body = "---\ntype: project\ntitle: A project\nowner: alice\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
            ..Default::default()
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_skips_extra_required_check_when_type_is_unknown() {
    // The note has both an unknown type AND would be missing a
    // required field if its declared type were valid. Only the
    // type issue should appear — chaining further checks against an
    // unknown type adds noise without telling the user anything new.
    let body = "---\ntype: bogus\ntitle: confused\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "bogus".to_string(),
        SchemaExtension {
            extra_required: vec!["irrelevant".to_string()],
            ..Default::default()
        },
    );
    let vault = vault_with_notes(&[("note.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert!(report.issues[0].message.contains("unknown note type"));
}

#[test]
fn lint_treats_explicit_null_value_as_missing() {
    // YAML `owner: ~` round-trips to JSON `null`. From a schema
    // perspective the field is unset, so lint should flag it.
    let body = "---\ntype: project\ntitle: nulled out\nowner: ~\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_string(),
        SchemaExtension {
            extra_required: vec!["owner".to_string()],
            ..Default::default()
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1);
    assert!(
        report.issues[0]
            .message
            .contains("missing required field `owner`")
    );
}

// ---------------------------------------------------------------------
// Declared-field value type-mismatch lint (#301). A `[schemas.<type>.fields]`
// block opts a built-in type into typed linting: a present field whose value
// doesn't match its declared `FieldType` (or `values`) warns.
// ---------------------------------------------------------------------

use cdno_core::config::{FieldSpec, FieldType};

/// A config with a single `[schemas.<type>.fields.<field>]` spec.
fn config_with_field(note_type: &str, field: &str, spec: FieldSpec) -> VaultConfig {
    let mut config = VaultConfig::default();
    let mut schema = SchemaExtension::default();
    schema.fields.insert(field.to_owned(), spec);
    config.schemas.insert(note_type.to_owned(), schema);
    config
}

/// A scalar field spec of the given type (no default, no `values`).
fn typed_field(ty: FieldType) -> FieldSpec {
    FieldSpec {
        ty,
        default: None,
        required: false,
        values: None,
        list: None,
        settable: None,
        log_on_change: None,
    }
}

fn bool_field() -> FieldSpec {
    typed_field(FieldType::Bool)
}

#[test]
fn lint_warns_on_a_mistyped_declared_field_value() {
    // `meds` is declared `bool`, but the note carries a string.
    let body = "---\ntype: project\ntitle: P\nmeds: \"maybe\"\n---\n# Body\n";
    let config = config_with_field("project", "meds", bool_field());
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    let issue = report
        .issues
        .iter()
        .find(|i| i.message.contains("meds"))
        .expect("a warning for the mistyped field");
    assert!(
        issue.message.contains("not a valid bool"),
        "{}",
        issue.message
    );
    assert_eq!(issue.severity, cdno_domain::LintSeverity::Warning);
}

#[test]
fn lint_passes_on_a_correctly_typed_declared_field_value() {
    let body = "---\ntype: project\ntitle: P\nmeds: true\n---\n# Body\n";
    let config = config_with_field("project", "meds", bool_field());
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report.issues.iter().all(|i| !i.message.contains("meds")),
        "a correctly-typed value must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_warns_on_a_value_outside_the_declared_values_set() {
    let spec = FieldSpec {
        ty: FieldType::String,
        default: None,
        required: false,
        values: Some(vec!["low".to_owned(), "ok".to_owned(), "good".to_owned()]),
        list: None,
        settable: None,
        log_on_change: None,
    };
    let body = "---\ntype: project\ntitle: P\nmood: elated\n---\n# Body\n";
    let config = config_with_field("project", "mood", spec);
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("mood") && i.message.contains("allowed values")),
        "an out-of-set value must warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_skips_the_type_check_for_an_absent_declared_field() {
    // A declared field the note simply omits is out of scope for the
    // type-check (presence is the required-field lint's concern). A non-required
    // field's absence produces no issue at all.
    let body = "---\ntype: project\ntitle: P\n---\n# Body\n";
    let config = config_with_field("project", "meds", bool_field());
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_passes_on_a_correctly_typed_int_field() {
    // A YAML integer parses to a JSON i64/u64 — the `is_i64() || is_u64()`
    // branch accepts it.
    let body = "---\ntype: project\ntitle: P\nsteps: 3\n---\n# Body\n";
    let config = config_with_field("project", "steps", typed_field(FieldType::Int));
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report.issues.iter().all(|i| !i.message.contains("steps")),
        "an integer value must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_warns_on_a_float_for_an_int_field() {
    // A float is not an integer — guards the documented float-rejection.
    let body = "---\ntype: project\ntitle: P\nsteps: 3.5\n---\n# Body\n";
    let config = config_with_field("project", "steps", typed_field(FieldType::Int));
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("steps") && i.message.contains("not a valid int")),
        "a float must warn against an int field: {:?}",
        report.issues
    );
}

#[test]
fn lint_warns_on_a_string_for_an_int_field() {
    // A quoted numeric string is a string, not an integer.
    let body = "---\ntype: project\ntitle: P\nsteps: \"3\"\n---\n# Body\n";
    let config = config_with_field("project", "steps", typed_field(FieldType::Int));
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("steps") && i.message.contains("not a valid int")),
        "a string must warn against an int field: {:?}",
        report.issues
    );
}

#[test]
fn lint_passes_on_a_correctly_formatted_date_field() {
    // A `YYYY-MM-DD` string that parses as a calendar date is accepted.
    let body = "---\ntype: project\ntitle: P\nsince: 2026-01-01\n---\n# Body\n";
    let config = config_with_field("project", "since", typed_field(FieldType::Date));
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report.issues.iter().all(|i| !i.message.contains("since")),
        "a valid date must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_warns_on_an_unparseable_date_field() {
    // A non-date string fails the string-AND-`NaiveDate::parse` branch.
    let body = "---\ntype: project\ntitle: P\nsince: nope\n---\n# Body\n";
    let config = config_with_field("project", "since", typed_field(FieldType::Date));
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("since") && i.message.contains("not a valid date")),
        "an unparseable date must warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_type_check_is_gated_on_an_explicit_fields_block() {
    // A note value that WOULD trip the type-lint (`owner: 123` against an
    // implied string), but the schema has only `extra_required` and NO
    // `[schemas.<type>.fields]` block — so the opt-in type-check never runs.
    // Locks the `!schema.fields.is_empty()` gate. The `extra_required`
    // presence check still passes (the field is present, non-null).
    let body = "---\ntype: project\ntitle: P\nowner: 123\n---\n# Body\n";
    let mut config = VaultConfig::default();
    config.schemas.insert(
        "project".to_owned(),
        SchemaExtension {
            extra_required: vec!["owner".to_owned()],
            ..Default::default()
        },
    );
    let vault = vault_with_notes(&[("projects/foo.md", body)], config);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report.issues.iter().all(|i| !i.message.contains("owner")),
        "no type-check without an explicit fields block: {:?}",
        report.issues
    );
}

// ---------------------------------------------------------------------
// Append-only-after-completion lint (#111). Archived action notes in
// `actions/_done/<year>/` may grow new lines but must not edit their
// pre-archival prefix. The snapshot recorded at archival time is the
// baseline.
// ---------------------------------------------------------------------

use cdno_domain::frontmatter::EnergyLevel;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

const ACTIVE_PROJECT_FOR_ARCHIVE: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

/// The append-only-after-completion issues only — filter the report to
/// keep these tests focused on the append-only check, independent of any
/// other lint severity. (Since #215 the archived-action daily reference
/// resolves, so it no longer adds broken-wikilink noise; the filter is
/// belt-and-suspenders.)
fn append_only_issues(report: &cdno_domain::LintReport) -> Vec<&cdno_domain::LintIssue> {
    report
        .issues
        .iter()
        .filter(|i| i.message.contains("append-only") || i.message.contains("truncated"))
        .collect()
}

/// Like `vault_with_notes` but also returns the backing store so a test
/// can mutate the archived file after the fact.
fn vault_with_notes_and_store(
    notes: &[(&str, &str)],
    config: VaultConfig,
) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, config).expect("Vault::new succeeded");
    (vault, store)
}

/// Spin a fresh attached action on project `foo` and complete it,
/// returning the path to the archived note (which the test can then
/// mutate to exercise the lint).
fn archive_a_fresh_action(vault: &Vault) -> VaultPath {
    vault
        .add_action_with_note(
            dt(2026, 5, 1, 9, 0),
            "foo",
            "Characterise",
            EnergyLevel::Deep,
        )
        .expect("add action with note");
    vault
        .complete_action(dt(2026, 5, 2, 9, 0), "foo", "characterise")
        .expect("complete action");
    vp("actions/_done/2026/characterise.md")
}

#[test]
fn append_only_lint_silent_on_unchanged_archived_note() {
    let (vault, _store) = vault_with_notes_and_store(
        &[("projects/foo.md", ACTIVE_PROJECT_FOR_ARCHIVE)],
        VaultConfig::default(),
    );
    let _done = archive_a_fresh_action(&vault);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        append_only_issues(&report).is_empty(),
        "unchanged archived note must be silent: {:?}",
        append_only_issues(&report)
    );
}

#[test]
fn append_only_lint_silent_when_only_appending_lines() {
    let (vault, store) = vault_with_notes_and_store(
        &[("projects/foo.md", ACTIVE_PROJECT_FOR_ARCHIVE)],
        VaultConfig::default(),
    );
    let done = archive_a_fresh_action(&vault);

    let original = store.read_file(&done).unwrap();
    let with_followup =
        format!("{original}\n## Six months later\nlate retrospective, see [[evidence/x]]\n");
    store.write_file(&done, &with_followup).unwrap();

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        append_only_issues(&report).is_empty(),
        "pure-append should be silent: {:?}",
        append_only_issues(&report)
    );
}

#[test]
fn append_only_lint_flags_size_changing_prefix_edit() {
    // Replacing "completed" (9 chars) with "blocked" (7 chars) shrinks
    // the file below `frozen_size` — the truncation branch fires.
    let (vault, store) = vault_with_notes_and_store(
        &[("projects/foo.md", ACTIVE_PROJECT_FOR_ARCHIVE)],
        VaultConfig::default(),
    );
    let done = archive_a_fresh_action(&vault);

    let original = store.read_file(&done).unwrap();
    let modified = original.replace("status: completed", "status: blocked");
    assert_ne!(
        original, modified,
        "replacement must actually change content"
    );
    store.write_file(&done, &modified).unwrap();

    let report = vault.lint_all_notes().expect("lint succeeds");
    let issues = append_only_issues(&report);
    assert_eq!(issues.len(), 1, "report: {:?}", issues);
    assert_eq!(issues[0].path, done);
    assert!(
        issues[0].message.contains("truncated"),
        "message: {}",
        issues[0].message
    );
}

#[test]
fn append_only_lint_flags_same_length_prefix_edit() {
    // Swap one ISO date for another of the same width — the file size
    // stays exactly the same, so the hash-mismatch branch is what fires
    // (rather than the truncation guard).
    let (vault, store) = vault_with_notes_and_store(
        &[("projects/foo.md", ACTIVE_PROJECT_FOR_ARCHIVE)],
        VaultConfig::default(),
    );
    let done = archive_a_fresh_action(&vault);

    let original = store.read_file(&done).unwrap();
    let modified = original.replace("completed: 2026-05-02", "completed: 2026-05-22");
    assert_ne!(
        original, modified,
        "replacement must actually change content"
    );
    assert_eq!(
        original.len(),
        modified.len(),
        "this test exercises the same-length path"
    );
    store.write_file(&done, &modified).unwrap();

    let report = vault.lint_all_notes().expect("lint succeeds");
    let issues = append_only_issues(&report);
    assert_eq!(issues.len(), 1, "report: {:?}", issues);
    assert_eq!(issues[0].path, done);
    assert!(
        issues[0].message.contains("append-only"),
        "message: {}",
        issues[0].message
    );
}

// ---------------------------------------------------------------------
// Attachment stub ↔ artefact-folder pairing (#154). An evidence note
// with a `kind` field is a stub linking a non-markdown artefact in a
// sibling folder. Lint checks the pairing in both directions: a stub
// whose folder vanished, and a folder whose stub vanished. Neither
// artefact is indexed, so lint is the only place these go noticed.
// ---------------------------------------------------------------------

const ATTACHMENT_STUB: &str = "---\ntype: evidence\ncreated: 2026-06-13\nsource: Some Paper\nportfolio: demo\norigin: \"[[projects/foo]]\"\nkind: pdf\n---\nA descriptive abstract of the PDF.\n";

const PLAIN_EVIDENCE: &str = "---\ntype: evidence\ncreated: 2026-06-13\nsource: An observation\nportfolio: demo\norigin: \"[[projects/foo]]\"\n---\nProse evidence, no artefact.\n";

#[test]
fn lint_flags_attachment_stub_with_missing_artefact_folder() {
    // Stub carries `kind: pdf` but the sibling folder that should hold
    // the artefact is absent — the artefacts were hand-deleted while the
    // stub survived.
    let stub = "portfolios/demo/2026-06-13-some-paper.md";
    let vault = vault_with_notes(&[(stub, ATTACHMENT_STUB)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert_eq!(report.issues[0].path, vp(stub));
    assert!(
        report.issues[0].message.contains("missing or empty"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_passes_attachment_stub_with_populated_folder() {
    // Stub paired with its artefact in the sibling folder — both pairing
    // directions are satisfied, so the report is clean.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/2026-06-13-some-paper.md", ATTACHMENT_STUB),
            (
                "portfolios/demo/2026-06-13-some-paper/some-paper.pdf",
                "%PDF-1.7 fake bytes",
            ),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_flags_orphan_artefact_folder() {
    // An artefact sits in a stub-shaped folder but the stub is gone —
    // the evidence is invisible to every structural retrieval.
    let vault = vault_with_notes(
        &[(
            "portfolios/demo/2026-06-13-some-paper/some-paper.pdf",
            "%PDF-1.7 fake bytes",
        )],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert_eq!(
        report.issues[0].path,
        vp("portfolios/demo/2026-06-13-some-paper")
    );
    assert!(
        report.issues[0].message.contains("orphaned attachment"),
        "message: {}",
        report.issues[0].message
    );
}

#[test]
fn lint_ignores_plain_evidence_without_kind() {
    // A prose evidence note (no `kind`) is not an attachment stub, so
    // the absence of a sibling folder is expected, not an issue.
    let vault = vault_with_notes(
        &[(
            "portfolios/demo/2026-06-13-an-observation.md",
            PLAIN_EVIDENCE,
        )],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

// ---------------------------------------------------------------------
// Broken-wikilink detection (#205). A body link that resolves to no
// note is a Warning, not an Error: the note parses fine. This is the
// check that would have caught the #200 dangling backlink.
// ---------------------------------------------------------------------

use cdno_domain::LintSeverity;

const DAILY_LINKING: &str =
    "---\ntype: daily\ntitle: Day\n---\n# Day\n\nSee {{link}} for details.\n";
const PROJECT_FOO: &str =
    "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n# Foo\n";

fn broken_link_issues(report: &cdno_domain::LintReport) -> Vec<&cdno_domain::LintIssue> {
    report
        .issues
        .iter()
        .filter(|i| i.message.contains("broken wikilink"))
        .collect()
}

#[test]
fn lint_flags_a_broken_wikilink_as_a_warning() {
    let body = DAILY_LINKING.replace("{{link}}", "[[projects/ghost]]");
    let vault = vault_with_notes(
        &[("journal/2026/daily/2026-05-01.md", &body)],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    let broken = broken_link_issues(&report);
    assert_eq!(broken.len(), 1, "issues: {:?}", report.issues);
    assert_eq!(broken[0].severity, LintSeverity::Warning);
    assert!(
        broken[0].message.contains("[[projects/ghost]]"),
        "message: {}",
        broken[0].message
    );
}

#[test]
fn lint_passes_a_resolvable_wikilink() {
    // The link target exists, so it resolves and lint stays quiet.
    let body = DAILY_LINKING.replace("{{link}}", "[[projects/foo]]");
    let vault = vault_with_notes(
        &[
            ("journal/2026/daily/2026-05-01.md", &body),
            ("projects/foo.md", PROJECT_FOO),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        broken_link_issues(&report).is_empty(),
        "issues: {:?}",
        report.issues
    );
}

#[test]
fn lint_resolves_a_folder_note_link_to_its_index() {
    // A portfolio note lives at `portfolios/<slug>/_index.md`. Both the
    // bare folder form `[[portfolios/<slug>]]` — what authors and the
    // daily-log writer naturally emit — and the explicit
    // `[[portfolios/<slug>/_index]]` resolve to the folder's `_index.md`
    // via the resolver's folder-index rule; neither dangles. (Supersedes
    // the #200-era convention that required the explicit `/_index` stem:
    // bare-is-canonical, since that is what the vault's own tooling writes.)
    let portfolio =
        "---\ntype: portfolio\nquestion: \"Q\"\ncreated: 2026-04-01\nproject: null\n---\n# Q\n";
    let bare = DAILY_LINKING.replace("{{link}}", "[[portfolios/demo]]");
    let explicit = DAILY_LINKING.replace("{{link}}", "[[portfolios/demo/_index]]");

    for daily in [bare.as_str(), explicit.as_str()] {
        let vault = vault_with_notes(
            &[
                ("journal/2026/daily/2026-05-01.md", daily),
                ("portfolios/demo/_index.md", portfolio),
            ],
            VaultConfig::default(),
        );
        assert!(
            broken_link_issues(&vault.lint_all_notes().unwrap()).is_empty(),
            "a folder-note link (bare or explicit) resolves to its _index.md"
        );
    }
}

#[test]
fn lint_flags_a_folder_link_with_no_index_note() {
    // The folder-index rule only resolves when the folder actually holds an
    // `_index.md`. A `[[portfolios/ghost]]` naming no such note still
    // dangles — resolution stays sound and lint keeps catching genuinely
    // broken links.
    let dangling = DAILY_LINKING.replace("{{link}}", "[[portfolios/ghost]]");
    let vault = vault_with_notes(
        &[("journal/2026/daily/2026-05-01.md", &dangling)],
        VaultConfig::default(),
    );
    assert_eq!(
        broken_link_issues(&vault.lint_all_notes().unwrap()).len(),
        1,
        "a folder link with no _index.md still dangles"
    );
}

#[test]
fn lint_ignores_a_dangling_frontmatter_link() {
    // `core_question` is a frontmatter wikilink; lint's broken-link scan is
    // deliberately body-only (even though the reconciler now indexes
    // frontmatter links as backlinks, #395), so a dangling one is not
    // flagged here.
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\ncore_question: \"[[questions/ghost]]\"\n---\n# Foo\n";
    let vault = vault_with_notes(&[("projects/foo.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        broken_link_issues(&report).is_empty(),
        "frontmatter links are out of scope: {:?}",
        report.issues
    );
}

#[test]
fn lint_counts_errors_and_warnings_separately() {
    // One unknown-type note (error) and one note with two broken body
    // links (two warnings) -- pins the error/warning split arithmetic.
    let vault = vault_with_notes(
        &[
            ("bogus.md", "---\ntype: nonsense\n---\n# x\n"),
            (
                "journal/2026/daily/2026-05-01.md",
                "---\ntype: daily\ntitle: D\n---\n# D\n\nSee [[a/ghost]] and [[b/ghost]].\n",
            ),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert_eq!(report.error_count(), 1, "issues: {:?}", report.issues);
    assert_eq!(report.warning_count(), 2, "issues: {:?}", report.issues);
}

#[test]
fn lint_reports_a_corrupt_indexed_note_as_error_without_aborting() {
    // A note valid at index time but corrupt on disk now (a stale index
    // row the reconciler's mtime fast-path didn't refresh). Lint must
    // report it as an error and keep going -- aborting would hide every
    // other issue, the opposite of what lint is for.
    let (vault, store) = vault_with_notes_and_store(
        &[
            ("a.md", "---\ntype: daily\ntitle: A\n---\n# A\n"),
            ("b.md", "---\ntype: daily\ntitle: B\n---\n# B\n"),
        ],
        VaultConfig::default(),
    );
    // Corrupt `a` after indexing: unterminated YAML flow sequence.
    store
        .write_file(&vp("a.md"), "---\nfoo: [1, 2\n---\n# A\n")
        .unwrap();

    let report = vault
        .lint_all_notes()
        .expect("lint must not abort on a corrupt note");
    let a_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.path == vp("a.md"))
        .collect();
    assert_eq!(a_issues.len(), 1, "report: {:?}", report.issues);
    assert!(
        a_issues[0].message.contains("malformed frontmatter"),
        "message: {}",
        a_issues[0].message
    );
    // `b` was still reached (the run continued past the corrupt note).
    assert!(report.issues.iter().all(|i| i.path != vp("b.md")));
}

#[test]
fn lint_does_not_flag_an_archived_action_reference() {
    // #215 resolved: completing an action archives its note to
    // `actions/_done/<year>/`, and the add-time daily-log entry's
    // `[[actions/<slug>]]` now resolves there via the resolver's
    // last-segment fallback -- so it is no longer reported as broken.
    let (vault, _store) = vault_with_notes_and_store(
        &[("projects/foo.md", ACTIVE_PROJECT_FOR_ARCHIVE)],
        VaultConfig::default(),
    );
    archive_a_fresh_action(&vault);

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        !broken_link_issues(&report)
            .iter()
            .any(|i| i.message.contains("[[actions/characterise]]")),
        "the archived-action reference should resolve, not dangle: {:?}",
        report.issues
    );
}

// --- frontmatter-order drift (#236) ----------------------------------

#[test]
fn lint_flags_frontmatter_out_of_canonical_order() {
    // Canonical daily order is `type` then `date`; this note reverses
    // them, so lint emits an order Warning (the note is valid, just
    // untidy -- `cdno normalise` fixes it).
    let body = "---\ndate: 2026-04-19\ntype: daily\n---\n# Note\n";
    let vault = vault_with_notes(
        &[("journal/2026/daily/2026-04-19.md", body)],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    let order_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.message.contains("canonical order"))
        .collect();
    assert_eq!(order_issues.len(), 1, "issues: {:?}", report.issues);
    assert_eq!(order_issues[0].severity, LintSeverity::Warning);
    assert_eq!(order_issues[0].path, vp("journal/2026/daily/2026-04-19.md"));
}

#[test]
fn lint_does_not_flag_canonical_frontmatter_order() {
    // Keys already in the canonical `type` then `date` order: no drift.
    let body = "---\ntype: daily\ndate: 2026-04-19\n---\n# Note\n";
    let vault = vault_with_notes(
        &[("journal/2026/daily/2026-04-19.md", body)],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.message.contains("canonical order")),
        "a canonical note must not be flagged: {:?}",
        report.issues
    );
}

#[test]
fn lint_frontmatter_order_follows_a_custom_template() {
    // A custom daily template declaring `date` before `type` makes that
    // the canonical order, so a note in that order is clean -- proving
    // the rule derives order from the effective template (matching
    // `cdno normalise`), not a hardcoded built-in order.
    let custom = "---\ndate: {{date}}\ntype: daily\n---\n# {{heading}}\n\n## Logs\n";
    let note = "---\ndate: 2026-04-19\ntype: daily\n---\n# Note\n";
    let vault = vault_with_notes(
        &[
            (".cuaderno/templates/daily.md", custom),
            ("journal/2026/daily/2026-04-19.md", note),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.message.contains("canonical order")),
        "note matches the custom template's order, so no drift: {:?}",
        report.issues
    );
}

#[test]
fn lint_flags_tracking_note_out_of_variant_order() {
    // Tracking order is variant-specific (keyed by `activity`). A gym
    // note must be checked against the tracking-gym order, not generic.
    // This scrambled gym note mirrors the normalise variant test, but
    // through the lint entry point.
    let scrambled = "---\ndate: 2026-04-26\ntype: tracking\nactivity: gym\nroutine: null\nstewardship: health\nduration_min: null\n---\n# Gym\n";
    let p = "stewardships/health/tracking/2026-04-26-gym.md";
    let vault = vault_with_notes(&[(p, scrambled)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let order_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.message.contains("canonical order"))
        .collect();
    assert_eq!(order_issues.len(), 1, "issues: {:?}", report.issues);
    assert_eq!(order_issues[0].severity, LintSeverity::Warning);
    assert_eq!(order_issues[0].path, vp(p));
}

#[test]
fn lint_does_not_flag_tracking_note_in_variant_order() {
    // A gym note already in tracking-gym order is clean -- confirms the
    // variant resolves as gym (not the generic tracking order).
    let in_order = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-04-26\nduration_min: null\nroutine: null\n---\n# Gym\n";
    let p = "stewardships/health/tracking/2026-04-26-gym.md";
    let vault = vault_with_notes(&[(p, in_order)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.message.contains("canonical order")),
        "gym note in variant order must not be flagged: {:?}",
        report.issues
    );
}

#[test]
fn lint_does_not_flag_canonical_order_with_trailing_unknown_key() {
    // Canonical project order with one extra key the template doesn't
    // define: unknown keys stay trailing, so this is NOT drift.
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\nextra: x\n---\n# Foo\n\n## Current State\n";
    let vault = vault_with_notes(&[("projects/foo.md", body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.message.contains("canonical order")),
        "a trailing unknown key is not order drift: {:?}",
        report.issues
    );
}

#[test]
fn lint_order_flags_agree_with_normalise_check() {
    // The PR's central claim: lint flags exactly the notes `normalise
    // --check` would reorder. Seed one drifted + one canonical note and
    // assert the two sets match.
    let drifted = "---\ndate: 2026-04-19\ntype: daily\n---\n# A\n";
    let canonical = "---\ntype: daily\ndate: 2026-04-20\n---\n# B\n";
    let vault = vault_with_notes(
        &[
            ("journal/2026/daily/2026-04-19.md", drifted),
            ("journal/2026/daily/2026-04-20.md", canonical),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    let mut lint_flagged: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.message.contains("canonical order"))
        .map(|i| i.path.clone())
        .collect();
    lint_flagged.sort_by_key(|p| p.to_string());

    let mut normalise_would_change = vault
        .normalise_notes(true)
        .expect("normalise check")
        .changed;
    normalise_would_change.sort_by_key(|p| p.to_string());

    assert_eq!(
        lint_flagged, normalise_would_change,
        "lint's order flags must match normalise --check"
    );
    assert_eq!(lint_flagged, vec![vp("journal/2026/daily/2026-04-19.md")]);
}

// --- Config-defined custom note types (schema-only) ---

#[test]
fn lint_accepts_a_valid_custom_type_note() {
    // A registered custom type with all declared required fields present
    // lints clean — no "unknown note type", no missing-field error.
    let body = "---\ntype: person\nname: Ada\nrole: advisor\n---\n# Ada\n";
    let vault = vault_with_notes(&[("people/ada.md", body)], config_with_person());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_flags_a_custom_type_missing_a_required_field() {
    // `name` is declared required for `person`; omitting it is an error.
    let body = "---\ntype: person\nrole: advisor\n---\n# Someone\n";
    let vault = vault_with_notes(&[("people/x.md", body)], config_with_person());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("missing required field `name`")),
        "expected a missing-required-field error, got: {:?}",
        report.issues
    );
}

#[test]
fn lint_still_flags_an_unregistered_type_as_unknown() {
    // The typo-guard survives: a type that is neither built-in nor a registered
    // custom type is still a hard error (here `persn` while `person` is
    // registered), so a stray `type:` typo can't silently mint a type.
    let body = "---\ntype: persn\nname: Ada\n---\n# Ada\n";
    let vault = vault_with_notes(&[("people/ada.md", body)], config_with_person());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("unknown note type: `persn`")),
        "expected unknown-type error, got: {:?}",
        report.issues
    );
}

#[test]
fn lint_flags_custom_type_frontmatter_out_of_order() {
    // Custom types get the same frontmatter-order warning as built-ins,
    // against the config-declared order (`type`, `name`, `role`).
    let body = "---\nrole: advisor\nname: Ada\ntype: person\n---\n# Ada\n";
    let vault = vault_with_notes(&[("people/ada.md", body)], config_with_person());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message.contains("canonical order")),
        "expected a frontmatter-order warning, got: {:?}",
        report.issues
    );
}

// --- Stewardship-dashboard near-miss lint (#312) -------------------------
//
// `Vault::lapsed_habits` and the periodic-commitment parser skip malformed
// dashboard bullets silently by design; this rule turns that silent skip
// into a visible `Warning`. Acceptance is the canonical parsers' verdict —
// these tests exercise each near-miss class plus the negatives (a valid
// line, prose, and dashboards without the sections).

/// Wrap dashboard `sections` in a minimal stewardship note.
fn stewardship(sections: &str) -> String {
    format!("---\ntype: stewardship\ncontext: personal\n---\n\n# Steward\n\n{sections}")
}

/// The dashboard warnings in a report — those naming one of the two scanned
/// sections. Filtering keeps these assertions independent of unrelated lint
/// output (e.g. frontmatter-order drift).
fn dashboard_warnings(report: &cdno_domain::LintReport) -> Vec<&cdno_domain::LintIssue> {
    report
        .issues
        .iter()
        .filter(|i| {
            i.message.contains("Active Habits") || i.message.contains("Periodic Commitments")
        })
        .collect()
}

#[test]
fn lint_flags_active_habits_line_with_ascii_hyphen() {
    // The `- ` marker is a real bullet; the *internal* separator is an ASCII
    // hyphen where an em-dash belongs, so the lapse scan drops it silently.
    let body = stewardship("## Active Habits\n- Swimming 1x/week - lapsed since March\n");
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    let issue = warnings[0];
    assert_eq!(issue.severity, LintSeverity::Warning);
    assert_eq!(issue.path, vp("stewardships/health.md"));
    assert!(
        issue.message.contains("ASCII hyphen"),
        "hint should name the hyphen: {}",
        issue.message
    );
}

#[test]
fn lint_flags_active_habits_line_with_en_dash() {
    // An en-dash (U+2013) is visually almost indistinguishable from the
    // em-dash (U+2014) the grammar requires — the classic invisible typo.
    let body = stewardship("## Active Habits\n- Swimming 1x/week \u{2013} lapsed since March\n");
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("en-dash"),
        "hint should name the en-dash: {}",
        warnings[0].message
    );
}

#[test]
fn lint_flags_periodic_line_missing_next_marker() {
    // Two em-dashes, right shape — but the third field says `next 2026-...`
    // without the `next:` marker, so `parse_periodic_line` rejects it.
    let body = stewardship(
        "## Periodic Commitments\n- Dental check-up \u{2014} every 6 months \u{2014} next 2026-04-01\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("next:"),
        "hint should name the missing marker: {}",
        warnings[0].message
    );
}

#[test]
fn lint_flags_periodic_line_with_unparseable_date() {
    // Correct structure and a `next:` marker, but the date is not
    // YYYY-MM-DD, so the aggregator would have skipped it silently.
    let body = stewardship(
        "## Periodic Commitments\n- Eye exam \u{2014} yearly \u{2014} next: sometime\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("unparseable date"),
        "hint should name the bad date: {}",
        warnings[0].message
    );
}

#[test]
fn lint_accepts_valid_dashboard_lines() {
    // A perfectly canonical habit line and periodic-commitment line: the
    // parsers accept both, so no near-miss warning is emitted.
    let body = stewardship(
        "## Active Habits\n- Resistance training 3x/week \u{2014} on track\n\n\
         ## Periodic Commitments\n- Blood work \u{2014} yearly \u{2014} next: 2099-11-01\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        dashboard_warnings(&report).is_empty(),
        "valid lines must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_ignores_prose_lines_and_missing_sections() {
    // A non-bullet prose line inside `## Active Habits` is not a bullet, so
    // it is never fed to the parser and never flagged. The dashboard also
    // carries no `## Periodic Commitments` section at all, which the scan
    // simply skips.
    let body = stewardship(
        "## Active Habits\nHabits I am nurturing this quarter:\n- Stretching \u{2014} on track\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        dashboard_warnings(&report).is_empty(),
        "prose and absent sections must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_ignores_dashboard_without_scanned_sections() {
    // A stewardship with neither scanned section produces no dashboard
    // warnings — the scan has nothing to look at.
    let body = stewardship("## Current Status\nHolding steady.\n");
    let vault = vault_with_notes(
        &[("stewardships/finances.md", &body)],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    assert!(
        dashboard_warnings(&report).is_empty(),
        "a dashboard without the sections must not warn: {:?}",
        report.issues
    );
}

#[test]
fn lint_flags_active_habits_line_missing_separator_entirely() {
    // No dash of any kind: the bullet is just prose. The parser rejects
    // it and the hint names the missing em-dash.
    let body = stewardship("## Active Habits\n- Swimming every week\n");
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("missing the em-dash"),
        "hint should name the missing separator: {}",
        warnings[0].message
    );
}

#[test]
fn lint_flags_active_habits_line_with_empty_habit_and_lapse_scan_skips_it() {
    // The degenerate `- {em-dash} lapsed ...` bullet: the old lapse scan
    // detected it; the shared grammar now rejects it (empty habit text),
    // so the lapse scan skips it AND lint compensates with a warning.
    // This pins the one behavioural change the parse_habit_line
    // extraction introduced -- the silent skip is made visible, never lost.
    let body = stewardship("## Active Habits\n- \u{2014} lapsed since March\n");
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let lapsed = vault.lapsed_habits().expect("lapse scan succeeds");
    assert!(
        lapsed.is_empty(),
        "empty-habit bullet must not count as a lapse: {lapsed:?}"
    );

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0]
            .message
            .contains("either side of the em-dash is empty"),
        "hint should name the empty side: {}",
        warnings[0].message
    );
}

#[test]
fn lint_flags_periodic_line_with_en_dash_separators() {
    // En-dashes standing in for both em-dash separators.
    let body = stewardship(
        "## Periodic Commitments\n- Eye exam \u{2013} yearly \u{2013} next: 2026-04-01\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("en-dash"),
        "hint should name the en-dash: {}",
        warnings[0].message
    );
}

#[test]
fn lint_flags_periodic_line_with_ascii_hyphen_separators() {
    // ASCII hyphens standing in for the em-dash separators.
    let body = stewardship("## Periodic Commitments\n- Eye exam - yearly - next: 2026-04-01\n");
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("ASCII hyphen"),
        "hint should name the hyphen: {}",
        warnings[0].message
    );
}

#[test]
fn lint_periodic_hint_does_not_blame_legitimate_en_dash_in_title() {
    // An en-dash *range* in the title is legitimate; the line fails
    // because the third field lacks the `next:` marker. The hint must
    // point at the real defect, not the dash (PR #334 review finding).
    let body = stewardship(
        "## Periodic Commitments\n- Q1\u{2013}Q2 review \u{2014} quarterly \u{2014} 2026-04-01\n",
    );
    let vault = vault_with_notes(&[("stewardships/health.md", &body)], VaultConfig::default());

    let report = vault.lint_all_notes().expect("lint succeeds");
    let warnings = dashboard_warnings(&report);
    assert_eq!(warnings.len(), 1, "issues: {:?}", report.issues);
    assert!(
        warnings[0].message.contains("next:") && !warnings[0].message.contains("en-dash"),
        "hint must name the missing marker, not the title's en-dash: {}",
        warnings[0].message
    );
}

// ---------------------------------------------------------------------
// Orphan detection after #451. Ownership is resolved by location, not by
// extension, so a folder of filed markdown is checked like any other —
// the blind spot that let a detached one go unreported in both
// directions. The finding is raised once, at the
// `portfolios/<p>/<folder>` level, however deep the artefacts sit.
// ---------------------------------------------------------------------

const MD_ARTEFACT: &str = "# Reviewer notes\n\nVerdict: approve with changes.\n";

#[test]
fn lint_flags_orphan_folder_holding_only_markdown() {
    // Filing a `.md` document produces the same stub-plus-folder pair as
    // filing a PDF, so losing the stub is the same failure: the evidence
    // is invisible to every structural retrieval.
    let vault = vault_with_notes(
        &[(
            "portfolios/demo/2026-07-03-review-panel/02-reviewer-b.md",
            MD_ARTEFACT,
        )],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert_eq!(
        report.issues[0].path,
        vp("portfolios/demo/2026-07-03-review-panel")
    );
}

#[test]
fn lint_passes_markdown_artefact_paired_with_its_stub() {
    let vault = vault_with_notes(
        &[
            (
                "portfolios/demo/2026-07-03-review-panel.md",
                ATTACHMENT_STUB,
            ),
            (
                "portfolios/demo/2026-07-03-review-panel/02-reviewer-b.md",
                MD_ARTEFACT,
            ),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_reports_a_deep_orphan_tree_once_at_the_portfolio_subfolder() {
    // A pasted-image folder can hold dozens of files across several
    // levels. One finding, pointing at the folder a user would act on.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/assets/first.png", "fake bytes"),
            ("portfolios/demo/assets/second.png", "fake bytes"),
            ("portfolios/demo/assets/nested/third.png", "fake bytes"),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert_eq!(report.issues[0].path, vp("portfolios/demo/assets"));
}

#[test]
fn lint_passes_an_artefact_nested_inside_its_stubs_folder() {
    // A filed directory tree keeps its internal structure; every file in
    // it resolves to the same owning stub, so nothing is orphaned.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/2026-06-13-bundle.md", ATTACHMENT_STUB),
            (
                "portfolios/demo/2026-06-13-bundle/src/main.rs",
                "fn main() {}",
            ),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_does_not_flag_a_grouping_folder_holding_a_stub_and_its_artefacts() {
    // The stub sits one level below the portfolio root, inside a hand-made
    // grouping folder. Its own folder is properly owned, and the stub is a
    // note, so nothing here is an orphan. Reporting `portfolios/demo/sweep`
    // would be worse than noise: the remedy the message names — create
    // `sweep.md` — is exactly what would make reconciliation treat the stub
    // as an artefact and drop it from the index.
    let vault = vault_with_notes(
        &[
            (
                "portfolios/demo/sweep/2026-07-03-run-07.md",
                ATTACHMENT_STUB,
            ),
            (
                "portfolios/demo/sweep/2026-07-03-run-07/log.txt",
                "run output",
            ),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_does_not_flag_a_grouping_folder_of_ordinary_notes() {
    // Hand-organised evidence in a subfolder is notes, not artefacts.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/2026-Q2/first.md", PLAIN_EVIDENCE),
            ("portfolios/demo/2026-Q2/second.md", PLAIN_EVIDENCE),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_honours_ignore_globs_when_scanning_for_orphans() {
    // The orphan scan walks the store directly rather than the index, so
    // it has to apply the `ignore` globs itself — otherwise it reports on
    // files the rest of the tool has been told to disregard.
    let config = VaultConfig {
        ignore: vec!["portfolios/*/scratch/**".to_string()],
        ..Default::default()
    };
    let vault = vault_with_notes(&[("portfolios/demo/scratch/notes.txt", "scratch")], config);

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_still_flags_an_unowned_non_note_file() {
    // The check's actual purpose survives all three exemptions: a pasted
    // image folder with no stub is still reported.
    let vault = vault_with_notes(
        &[("portfolios/demo/assets/pasted.png", "fake bytes")],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert_eq!(report.issues[0].path, vp("portfolios/demo/assets"));
}

#[test]
fn lint_does_not_flag_a_grouping_folder_that_also_holds_a_stray_file() {
    // The exemption is folder-scoped, not file-scoped. Exempting note
    // files one by one still let a single stray beside them raise the
    // finding on their behalf — and the remedy it names (create
    // `<folder>.md`) would claim those notes as artefacts and drop them
    // from the index. A macOS `.DS_Store` is enough to trigger it, so this
    // is not a hypothetical.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/2026-Q2/first.md", PLAIN_EVIDENCE),
            ("portfolios/demo/2026-Q2/second.md", PLAIN_EVIDENCE),
            ("portfolios/demo/2026-Q2/.DS_Store", "mac metadata"),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_exempts_a_grouping_folder_from_a_note_nested_deeper_in_it() {
    // The folder is exempt because it holds a note *anywhere* beneath it:
    // the stub the message would name owns the whole subtree, so a note at
    // any depth is a note the advice would lose.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/sweep/runs/first.md", PLAIN_EVIDENCE),
            ("portfolios/demo/sweep/chart.png", "fake bytes"),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn lint_says_a_namesake_that_is_not_a_stub_does_not_claim_the_folder() {
    // `<folder>.md` is right there on disk, so calling it missing would
    // name a file the user can plainly see. It is simply not a stub.
    let vault = vault_with_notes(
        &[
            ("portfolios/demo/assets.md", PLAIN_EVIDENCE),
            ("portfolios/demo/assets/pasted.png", "fake bytes"),
        ],
        VaultConfig::default(),
    );

    let report = vault.lint_all_notes().expect("lint succeeds");

    assert_eq!(report.issues.len(), 1, "report: {:?}", report.issues);
    assert!(
        report.issues[0].message.contains("is not claimed by"),
        "message: {}",
        report.issues[0].message
    );
    assert!(
        !report.issues[0].message.contains("has no evidence stub"),
        "message: {}",
        report.issues[0].message
    );
}
