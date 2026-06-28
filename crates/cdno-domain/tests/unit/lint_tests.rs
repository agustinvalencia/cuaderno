use std::sync::Arc;

use cdno_core::config::{SchemaExtension, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
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
fn lint_flags_a_folder_note_link_missing_its_index_stem() {
    // The #200 regression in miniature: a portfolio note lives at
    // `portfolios/<slug>/_index.md`, so `[[portfolios/<slug>]]` dangles
    // while `[[portfolios/<slug>/_index]]` resolves.
    let portfolio =
        "---\ntype: portfolio\nquestion: \"Q\"\ncreated: 2026-04-01\nproject: null\n---\n# Q\n";
    let dangling = DAILY_LINKING.replace("{{link}}", "[[portfolios/demo]]");
    let resolving = DAILY_LINKING.replace("{{link}}", "[[portfolios/demo/_index]]");

    let vault_bad = vault_with_notes(
        &[
            ("journal/2026/daily/2026-05-01.md", &dangling),
            ("portfolios/demo/_index.md", portfolio),
        ],
        VaultConfig::default(),
    );
    assert_eq!(
        broken_link_issues(&vault_bad.lint_all_notes().unwrap()).len(),
        1,
        "bare folder link should dangle"
    );

    let vault_good = vault_with_notes(
        &[
            ("journal/2026/daily/2026-05-01.md", &resolving),
            ("portfolios/demo/_index.md", portfolio),
        ],
        VaultConfig::default(),
    );
    assert!(
        broken_link_issues(&vault_good.lint_all_notes().unwrap()).is_empty(),
        "the /_index form should resolve"
    );
}

#[test]
fn lint_ignores_a_dangling_frontmatter_link() {
    // `core_question` is a frontmatter wikilink; broken-link scanning is
    // body-only (matching the reconciler's link graph), so a dangling
    // one is not flagged.
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
