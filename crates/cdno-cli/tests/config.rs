//! In-process tests for `cdno config`. Seed a vault on disk, then assert on
//! the data seams (`read_raw`, `finish_edit`, `validate_config_str`) rather
//! than capturing stdout — the pattern `cdno templates` and `cdno search`
//! established.
//!
//! The case these verbs exist for is a config that does NOT parse, so most
//! of what follows deliberately runs against a broken vault. That is also
//! the regression guarded hardest: an earlier draft opened the vault first,
//! which made `cdno config validate` fail with `loading config.toml` and no
//! line, column or reason — useless on the one input it is for.

use std::fs;
use std::path::Path;

use cdno_cli::commands::{config, init};
use cdno_core::store::FsVaultStore;
use cdno_domain::ConfigSaveError;
use cdno_domain::validate_config_str;
use tempfile::tempdir;

fn seed(root: &Path) {
    init::run(root).expect("init");
}

fn config_path(root: &Path) -> std::path::PathBuf {
    root.join(".cuaderno").join("config.toml")
}

fn break_config(root: &Path) {
    let path = config_path(root);
    let mut content = fs::read_to_string(&path).expect("read config");
    content.push_str("\ngarbage = [\n");
    fs::write(&path, content).expect("write broken config");
}

#[test]
fn show_reads_a_config_that_does_not_parse() {
    // The whole point: you cannot fix a broken config you cannot read, and
    // `Vault::new` refuses to hand back a vault for one.
    let dir = tempdir().unwrap();
    seed(dir.path());
    break_config(dir.path());

    let doc = config::read_raw(dir.path()).expect("read a broken config");
    assert!(
        doc.content.contains("garbage = ["),
        "content should be verbatim, including the breakage:\n{}",
        doc.content
    );
    assert_eq!(doc.hash.len(), 16, "content hash is 16 hex chars");
}

#[test]
fn show_is_byte_verbatim() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let on_disk = fs::read_to_string(config_path(dir.path())).unwrap();
    let doc = config::read_raw(dir.path()).expect("read");
    // Not `trim()`-equal — byte-equal. `cdno config show > config.toml` has
    // to round-trip, so a helpfully-added trailing newline would be a bug.
    assert_eq!(doc.content, on_disk);
}

#[test]
fn validate_names_the_line_and_column_of_a_syntax_error() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    break_config(dir.path());

    let content = config::read_raw(dir.path()).expect("read").content;
    let err = validate_config_str(&content).expect_err("a broken config must not validate");
    assert!(
        err.message.contains("TOML parse error"),
        "message should be TOML's own rendering, got: {}",
        err.message
    );
    assert!(err.line.is_some(), "a parse error carries a line");
}

#[test]
fn validate_accepts_a_freshly_initialised_vault() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let content = config::read_raw(dir.path()).expect("read").content;
    assert!(
        validate_config_str(&content).is_ok(),
        "the config `cdno init` writes must validate"
    );
}

#[test]
fn an_unchanged_buffer_writes_nothing() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");

    let outcome = config::finish_edit(&store, &original, &original.content).expect("no-op edit");
    assert_eq!(outcome, config::EditOutcome::Unchanged);
}

#[test]
fn an_unchanged_buffer_does_not_trip_the_compare_and_swap() {
    // A no-op edit is checked BEFORE the save gate, so someone else touching
    // the file while an editor sat open cannot turn "I changed nothing" into
    // a conflict the user has to resolve.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");

    // Land a concurrent edit underneath.
    let mut moved = original.content.clone();
    moved.push_str("\n# a concurrent comment\n");
    fs::write(config_path(dir.path()), &moved).unwrap();

    let outcome = config::finish_edit(&store, &original, &original.content)
        .expect("an unchanged buffer is never a conflict");
    assert_eq!(outcome, config::EditOutcome::Unchanged);
    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        moved,
        "the concurrent edit must survive untouched"
    );
}

#[test]
fn a_valid_edit_is_written_and_re_read() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");

    let edited = format!("{}\n# a new comment\n", original.content);
    let outcome = config::finish_edit(&store, &original, &edited).expect("a valid edit saves");
    assert_eq!(
        outcome,
        config::EditOutcome::Saved {
            bytes: edited.len()
        }
    );
    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        edited,
        "written verbatim — comments and ordering preserved"
    );
}

#[test]
fn an_invalid_edit_is_refused_and_the_file_is_untouched() {
    // The never-brick guarantee, at the seam the CLI actually calls.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");
    let before = fs::read_to_string(config_path(dir.path())).unwrap();

    let edited = format!("{}\ngarbage = [\n", original.content);
    let err = config::finish_edit(&store, &original, &edited)
        .expect_err("a config that would not reopen must be refused");
    assert!(
        matches!(err, ConfigSaveError::Validation(_)),
        "expected a validation refusal, got {err:?}"
    );
    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        before,
        "nothing may be written when validation rejects"
    );
}

#[test]
fn a_concurrent_edit_is_refused_rather_than_clobbered() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");

    // Someone else saves while the editor is open.
    let theirs = format!("{}\n# theirs\n", original.content);
    fs::write(config_path(dir.path()), &theirs).unwrap();

    let mine = format!("{}\n# mine\n", original.content);
    let err = config::finish_edit(&store, &original, &mine)
        .expect_err("a stale baseline must not overwrite a newer file");
    assert!(
        matches!(err, ConfigSaveError::Conflict),
        "expected a conflict, got {err:?}"
    );
    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        theirs,
        "their edit must survive intact"
    );
}

#[test]
fn an_edit_that_is_both_invalid_and_stale_reports_the_validation_error() {
    // The gate's ORDER, not just its outcomes. Validation runs first, so a
    // buffer that is both unparseable and built on a stale baseline is
    // reported as invalid rather than as a conflict.
    //
    // Both orderings refuse the write, so the file is untouched either way —
    // the difference is what the user is told to do. "Reload and reapply"
    // sends someone to redo an edit that would not have validated anyway;
    // "this would not open" points at the actual problem. Mutation-tested:
    // moving the compare-and-swap ahead of validation passes every other
    // test in this file and in the domain suite, and fails only this one.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let store = FsVaultStore::new(dir.path());
    let original = config::read_raw(dir.path()).expect("read");

    // A concurrent save lands, making our baseline stale...
    fs::write(
        config_path(dir.path()),
        format!("{}\n# theirs\n", original.content),
    )
    .unwrap();

    // ...and our buffer is also broken.
    let mine = format!("{}\ngarbage = [\n", original.content);
    let err = config::finish_edit(&store, &original, &mine).expect_err("must be refused");
    assert!(
        matches!(err, ConfigSaveError::Validation(_)),
        "validation is checked first, so this is a Validation error, not a \
         Conflict — got {err:?}"
    );
}
