//! In-process tests for `commands::lint::run`.

use std::fs;

use cdno_cli::commands::{init, lint};
use chrono::NaiveDate;
use tempfile::tempdir;

/// A day before every date the fixtures write, so the parked read-through
/// rule (overdue dates on parked maps) stays out of these tests. These run
/// `lint::run` in process with this fixed date; the tests that run the real
/// `cdno lint` binary (`cli.rs`, `init.rs`, `migrate.rs`, `note.rs`) use the
/// real clock, so their fixtures must not put a past hard date on a parked
/// map unless they mean to see the read-through row.
fn lint_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()
}

#[test]
fn lint_succeeds_silently_on_a_freshly_inited_vault() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    // Post-#87 the index is empty after init (templates under
    // `.cuaderno/` are excluded from the scan), so lint finds nothing.
    lint::run(dir.path(), lint_day(), false).expect("lint should succeed on empty vault");
}

#[test]
fn lint_returns_err_when_a_note_has_an_unknown_type() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(
        dir.path().join("strange.md"),
        "---\ntype: bogus\ntitle: Mystery\n---\n# Body\n",
    )
    .unwrap();

    let err = lint::run(dir.path(), lint_day(), false).expect_err("lint should fail");
    let msg = format!("{err}");
    assert!(msg.contains("1 error(s)"), "unexpected error: {msg}");
}

#[test]
fn lint_warns_on_frontmatter_order_drift_and_strict_makes_it_fatal() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    // Canonical daily order is `type` then `date`; this note reverses
    // them, so it drifts. (Frontmatter is valid, so it's a warning.)
    let daily = dir.path().join("journal/2026/daily/2026-04-19.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(&daily, "---\ndate: 2026-04-19\ntype: daily\n---\n# Note\n").unwrap();

    // Non-strict: a warning is non-fatal, so lint still succeeds.
    lint::run(dir.path(), lint_day(), false).expect("order drift is non-fatal without --strict");

    // --strict: the warning becomes a failure.
    let err =
        lint::run(dir.path(), lint_day(), true).expect_err("strict lint should fail on the drift");
    assert!(
        format!("{err}").contains("1 warning(s)"),
        "unexpected error: {err}"
    );
}

#[test]
fn lint_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();

    let err = lint::run(dir.path(), lint_day(), false).expect_err("lint without vault must fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}

#[test]
fn lint_warns_on_malformed_stewardship_dashboard_line_and_strict_makes_it_fatal() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    // A stewardship whose Periodic Commitments bullet omits the `next:`
    // marker. The canonical parser rejects it, so the new dashboard rule
    // warns — proving it flows through the CLI surface with no per-rule
    // wiring (the frontmatter is in canonical order and there are no other
    // sections, so this is the only issue in the report).
    let steward = dir.path().join("stewardships/health.md");
    fs::create_dir_all(steward.parent().unwrap()).unwrap();
    fs::write(
        &steward,
        "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Periodic Commitments\n- Dental check-up \u{2014} every 6 months \u{2014} next 2026-04-01\n",
    )
    .unwrap();

    // Non-strict: a warning is non-fatal, so lint still succeeds.
    lint::run(dir.path(), lint_day(), false)
        .expect("dashboard warning is non-fatal without --strict");

    // --strict: the warning becomes a failure.
    let err = lint::run(dir.path(), lint_day(), true)
        .expect_err("strict lint should fail on the dashboard warning");
    assert!(
        format!("{err}").contains("1 warning(s)"),
        "unexpected error: {err}"
    );
}

#[test]
fn lint_resolves_an_on_disk_attachment_embed_through_the_filesystem() {
    // End-to-end through the real `FsVaultStore` (not the in-memory
    // double): a note embedding a pasted image that exists on disk is not
    // a broken link, and `--strict` — which makes any warning fatal — must
    // still succeed. A missing embed, by contrast, warns and fails strict.
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let daily = dir.path().join("journal/2026/daily/2026-04-19.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    // Canonical daily order (`type` then `date`) so no order-drift warning
    // muddies the strict check.
    fs::write(
        &daily,
        "---\ntype: daily\ndate: 2026-04-19\n---\n# Day\n\n![[assets/shot.png]]\n",
    )
    .unwrap();

    // A real binary file beside the note — bytes the text reader could not
    // parse, which is why `read_bytes` is the file probe.
    let assets = daily.parent().unwrap().join("assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(
        assets.join("shot.png"),
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a],
    )
    .unwrap();

    // Strict: the on-disk embed raises no warning, so the vault is clean.
    lint::run(dir.path(), lint_day(), true).expect("an on-disk embed is not a broken link");

    // Now point the embed at a file that does not exist.
    fs::write(
        &daily,
        "---\ntype: daily\ndate: 2026-04-19\n---\n# Day\n\n![[assets/missing.png]]\n",
    )
    .unwrap();
    let err = lint::run(dir.path(), lint_day(), true).expect_err("a missing embed warns");
    assert!(
        format!("{err}").contains("1 warning(s)"),
        "unexpected error: {err}"
    );
}

#[test]
fn severity_decides_the_style_and_the_two_do_not_share_one() {
    // Inverting the mapping — errors rendered as warnings and warnings as
    // errors — is invisible to every other test this command has, because
    // they all read the literal `[error]` / `[warning]` text, which the
    // inversion leaves untouched.
    use cdno_cli::commands::lint::severity_role;
    use cdno_cli::output::style::{Palette, Role};
    use cdno_domain::lint::LintSeverity;

    assert_eq!(severity_role(LintSeverity::Error), Role::Error);
    assert_eq!(severity_role(LintSeverity::Warning), Role::Warn);

    let palette = Palette::forced();
    assert_ne!(
        palette.paint(severity_role(LintSeverity::Error), "x"),
        palette.paint(severity_role(LintSeverity::Warning), "x"),
        "an error must not read like a warning"
    );
}

#[test]
fn lint_reports_the_over_cap_warning() {
    use assert_cmd::Command;
    use predicates::prelude::*;

    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    // Six active maps, none held, against the init default cap of 5. No
    // dates anywhere, so the real clock the binary reads adds no row.
    for slug in ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"] {
        fs::write(
            dir.path().join(format!("projects/{slug}.md")),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n\
             core_question: null\nclosed: null\n---\n\n# Map\n\n## Current State\n\n\
             ## Next Actions\n\n## Milestones\n\n## Waiting On\n",
        )
        .unwrap();
    }

    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH")
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains(".cuaderno/config.toml"))
        .stdout(predicate::str::contains(
            "6 projects count toward the cap of 5 (`max_active_projects`)",
        ))
        .stdout(predicate::str::contains("[[foxtrot]]"));
}

fn write_project(dir: &std::path::Path, slug: &str, aliases: &str) {
    fs::write(
        dir.join(format!("projects/{slug}.md")),
        format!(
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n\
             core_question: null\nclosed: null\naliases: {aliases}\n---\n\n# Map\n\n\
             ## Current State\n\n## Next Actions\n\n## Milestones\n\n## Waiting On\n"
        ),
    )
    .unwrap();
}

#[test]
fn lint_reports_a_straggler_with_the_path_and_both_names() {
    use assert_cmd::Command;
    use predicates::prelude::*;

    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    write_project(dir.path(), "surrogate-model", "[old-model]");
    let daily = dir.path().join("journal/2026/daily/2026-04-19.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(
        &daily,
        "---\ntype: daily\ndate: 2026-04-19\n---\n# Day\n\nSee [[old-model]].\n",
    )
    .unwrap();

    Command::cargo_bin("cdno")
        .expect("cdno binary built")
        .env_remove("CUADERNO_VAULT_PATH")
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[warning] journal/2026/daily/2026-04-19.md: `[[old-model]]` names a renamed note — now `surrogate-model`",
        ));
}

#[test]
fn lint_reports_a_shadowed_alias_with_the_path_and_both_names() {
    use assert_cmd::Command;
    use predicates::prelude::*;

    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    write_project(dir.path(), "surrogate-model", "[ablation-study]");
    write_project(dir.path(), "ablation-study", "[]");

    Command::cargo_bin("cdno")
        .expect("cdno binary built")
        .env_remove("CUADERNO_VAULT_PATH")
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[warning] projects/surrogate-model.md: alias `ablation-study` on `projects/surrogate-model.md` is shadowed by `projects/ablation-study.md`",
        ));
}

/// `cdno` against `root`, never inheriting the developer's vault.
fn cdno_at(root: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH")
        .env("NO_COLOR", "1")
        .arg("--vault")
        .arg(root)
        .arg("--no-interactive");
    cmd
}

/// A fresh vault holding project `old` and one action on it, archived
/// through the binary after its note gained a `[[old]]` line. Returns the
/// vault and the archived note's vault-relative path.
fn vault_with_an_archived_action() -> (tempfile::TempDir, String) {
    let b = tempdir().unwrap();
    init::run(b.path()).expect("init");
    cdno_at(b.path())
        .args(["project", "create", "--title", "Old", "--context", "work"])
        .assert()
        .success();
    cdno_at(b.path())
        .args([
            "action",
            "add",
            "--project",
            "old",
            "--title",
            "Characterise",
        ])
        .args(["--energy", "deep", "--note"])
        .assert()
        .success();
    let active = b.path().join("actions/characterise.md");
    let raw = fs::read_to_string(&active).unwrap();
    fs::write(&active, format!("{raw}\nCalibrated against [[old]].\n")).unwrap();
    cdno_at(b.path())
        .args([
            "action",
            "complete",
            "--project",
            "old",
            "--query",
            "characterise",
        ])
        .assert()
        .success();
    let year = fs::read_dir(b.path().join("actions/_done"))
        .unwrap()
        .map(|y| y.unwrap().file_name().into_string().unwrap())
        .find(|y| {
            b.path()
                .join(format!("actions/_done/{y}/characterise.md"))
                .exists()
        })
        .expect("archived note");
    (b, format!("actions/_done/{year}/characterise.md"))
}

/// Clear the three masked columns of `rel`'s snapshot row in the vault's
/// own SQLite index, which is what a row archived before migration 004
/// holds.
fn make_pre_004(root: &std::path::Path, rel: &str) {
    use cdno_core::index::{ArchivalSnapshot, SqliteIndex, VaultIndex};
    let index = SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index");
    let path = cdno_core::path::VaultPath::new(rel).unwrap();
    let snap = index
        .find_archival_snapshot(&path)
        .unwrap()
        .expect("archival recorded a snapshot");
    assert!(
        snap.masked_hash.is_some(),
        "archival stamps the masked hash"
    );
    index
        .record_archival_snapshot(
            &path,
            &ArchivalSnapshot {
                masked_hash: None,
                mask_version: None,
                masked_size: None,
                ..snap
            },
        )
        .unwrap();
}

/// The second clone (RFC 0007 T16). A rename on one clone rewrites
/// references inside an archived action's frozen prefix; only the markdown
/// syncs, so the other clone's index still holds the snapshot taken before
/// the rename. Here clone B archives through the binary, then receives the
/// bytes a rename of `old` to `surrogate-model` writes (a longer slug, so
/// the raw prefix end moves; the same-length case would hide the bug), and
/// its lint must stay clean of errors.
#[test]
fn a_rename_synced_from_another_clone_leaves_lint_clean() {
    use predicates::prelude::*;

    let (b, rel) = vault_with_an_archived_action();
    let done = b.path().join(&rel);
    let archived = fs::read_to_string(&done).unwrap();
    let renamed = rename_shaped(&archived);
    assert_ne!(
        renamed.len(),
        archived.len(),
        "a slug of a different length"
    );
    fs::write(&done, renamed).unwrap();

    cdno_at(b.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains("[error]").not());
}

/// The ordering limit (#901): clone B's row predates the masked hash, and
/// the renamed markdown reaches B before B first opens on a build that
/// could backfill it. The plain hash can never vouch for the prefix again,
/// so the row stays unstamped and lint reports it, naming the way out.
#[test]
fn a_rename_synced_before_the_first_open_on_this_build_is_reported_with_the_way_out() {
    use predicates::prelude::*;

    let (b, rel) = vault_with_an_archived_action();
    make_pre_004(b.path(), &rel);
    let done = b.path().join(&rel);
    fs::write(&done, rename_shaped(&fs::read_to_string(&done).unwrap())).unwrap();

    cdno_at(b.path())
        .arg("lint")
        .assert()
        .failure()
        .stdout(predicate::str::contains(format!("[error] {rel}: ")))
        .stdout(predicate::str::contains("predates the masked hash"))
        .stdout(predicate::str::contains("`cdno reindex`"));
}

/// The backfill is best-effort and never takes the lock for nothing. A
/// tampered pre-004 row can never be filled, so with another process
/// holding `.cuaderno/.lock` the vault still opens at once and lint
/// reports the tampered note.
#[test]
fn a_tampered_pre_004_row_does_not_need_the_lock_another_process_holds() {
    use cdno_core::store::{FsVaultStore, VaultStore};
    use predicates::prelude::*;

    let (b, rel) = vault_with_an_archived_action();
    make_pre_004(b.path(), &rel);
    let done = b.path().join(&rel);
    let raw = fs::read_to_string(&done).unwrap();
    fs::write(
        &done,
        raw.replace("Calibrated against", "Calibrates against"),
    )
    .unwrap();
    // One open with the lock free lets reconciliation take in the edit.
    cdno_at(b.path()).arg("lint").assert().failure();

    let _held = FsVaultStore::new(b.path())
        .acquire_write_lock()
        .expect("take the vault lock");
    let started = std::time::Instant::now();
    cdno_at(b.path())
        .arg("lint")
        .assert()
        .failure()
        .stdout(predicate::str::contains(format!(
            "[error] {rel}: archived action note modified an existing line"
        )))
        .stderr(predicate::str::contains("timed out").not());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the open waited for the lock"
    );
}

/// A row the backfill could fill, while another process holds the lock:
/// the pass times out, logs, and the vault still opens; lint is clean
/// through the plain hash.
#[test]
fn a_backfill_that_cannot_take_the_lock_does_not_fail_the_open() {
    use cdno_core::store::{FsVaultStore, VaultStore};
    use predicates::prelude::*;

    let (b, rel) = vault_with_an_archived_action();
    make_pre_004(b.path(), &rel);

    let held = FsVaultStore::new(b.path())
        .acquire_write_lock()
        .expect("take the vault lock");
    cdno_at(b.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains("[error]").not());
    assert!(
        masked_hash_of(b.path(), &rel).is_none(),
        "the held lock kept the pass from writing"
    );
    drop(held);

    // With the lock free, the next open stamps the row.
    cdno_at(b.path()).arg("lint").assert().success();
    assert!(
        masked_hash_of(b.path(), &rel).is_some(),
        "the next open retries and fills it"
    );
}

/// The masked hash on `rel`'s snapshot row, read from the vault's index.
fn masked_hash_of(root: &std::path::Path, rel: &str) -> Option<String> {
    use cdno_core::index::{SqliteIndex, VaultIndex};
    let index = SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index");
    index
        .find_archival_snapshot(&cdno_core::path::VaultPath::new(rel).unwrap())
        .unwrap()
        .expect("snapshot row")
        .masked_hash
}

/// The bytes a rename of `old` to `surrogate-model` writes into a note:
/// every wikilink address and the `project:` scalar, through the core
/// rewriters the rename uses.
fn rename_shaped(text: &str) -> String {
    let (linked, edits) = cdno_core::extractors::rewrite_wikilink_addresses(
        text,
        |target| target == "old",
        |_| "surrogate-model".to_owned(),
    );
    assert!(!edits.is_empty(), "the fixture must hold a link to rewrite");
    let (block, _) = cdno_core::frontmatter::split_frontmatter(&linked).expect("frontmatter");
    let start = block.as_ptr() as usize - linked.as_ptr() as usize;
    let end = start + block.len();
    let (rewritten, edits) = cdno_core::frontmatter_edit::rewrite_frontmatter_scalar(
        block,
        "project",
        "old",
        "surrogate-model",
    );
    assert_eq!(edits.len(), 1, "the `project:` scalar is rewritten");
    format!("{}{rewritten}{}", &linked[..start], &linked[end..])
}
