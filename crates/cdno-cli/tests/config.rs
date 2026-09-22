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

// ---------------------------------------------------------------------------
// The structured setters
// ---------------------------------------------------------------------------
//
// These run the real binary rather than the data seams above, because what
// is under test is the composition: read the current value, merge the flags
// actually given, and push the result through the save gate. A seam test
// would exercise the merge without proving the verb wires it in.

use assert_cmd::Command;

fn cdno(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.arg("--vault").arg(dir).arg("--no-interactive");
    cmd
}

#[test]
fn changing_one_key_of_a_note_type_keeps_the_rest() {
    // THE regression this merge exists for. `set_note_type` replaces the
    // whole table: it writes every key the model carries and removes every
    // key it does not. Passing the CLI flags straight through would mean a
    // folder-only edit silently dropped `required` and `template`, because
    // those flags were absent — data loss with a success message on it.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
            "--required",
            "name,email",
            "--template",
            "people.md",
        ])
        .assert()
        .success();

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "humans",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(content.contains("folder = \"humans\""), "the edit applied");
    assert!(
        content.contains("required = [\"name\", \"email\"]"),
        "an omitted flag must keep its current value, not clear it:\n{content}"
    );
    assert!(
        content.contains("template = \"people.md\""),
        "template must survive a folder-only edit:\n{content}"
    );
}

#[test]
fn an_empty_value_clears_an_optional_key() {
    // The counterpart to the merge: if an omitted flag means "keep", there
    // has to be a way to say "remove", or a template could never be unset.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
            "--template",
            "people.md",
        ])
        .assert()
        .success();
    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--template",
            "",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        !content.contains("template = "),
        "an empty value clears the key:\n{content}"
    );
}

#[test]
fn a_new_note_type_needs_a_folder_but_an_existing_one_does_not() {
    // `--folder` is the one conditionally-required input: a new type must
    // say where its notes live, an existing one already has. Non-interactive
    // so the absent flag is an error rather than a prompt.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args(["config", "note-type", "set", "--name", "people"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--folder"));

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
        ])
        .assert()
        .success();
    // Now that it exists, the same command without --folder is fine.
    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--required",
            "name",
        ])
        .assert()
        .success();
}

#[test]
fn setting_a_value_it_already_has_writes_nothing() {
    // Keeps a re-run, or a script that applies the same config repeatedly,
    // from churning the file and burning the compare-and-swap.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args(["config", "var", "set", "--name", "author", "--value", "A"])
        .assert()
        .success();
    let after_first = fs::read_to_string(config_path(dir.path())).unwrap();

    cdno(dir.path())
        .args(["config", "var", "set", "--name", "author", "--value", "A"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No change"));

    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        after_first,
        "a no-op set must not rewrite the file"
    );
}

#[test]
fn removing_something_absent_succeeds() {
    // Idempotent at the domain layer, and the CLI must not turn that into
    // an error: a stale delete or a re-run is what makes this scriptable.
    let dir = tempdir().unwrap();
    seed(dir.path());
    cdno(dir.path())
        .args(["config", "var", "remove", "--name", "nosuch"])
        .assert()
        .success();
    cdno(dir.path())
        .args(["config", "note-type", "remove", "--name", "nosuch"])
        .assert()
        .success();
}

#[test]
fn a_structured_edit_that_would_break_the_vault_is_refused() {
    // The never-brick guarantee, reached through a setter rather than the
    // editor: the same gate, so the same outcome.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let before = fs::read_to_string(config_path(dir.path())).unwrap();

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "project",
            "--folder",
            "elsewhere",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nothing was written"));

    assert_eq!(
        fs::read_to_string(config_path(dir.path())).unwrap(),
        before,
        "a refused structured edit must leave the file byte-identical"
    );
}

#[test]
fn a_setter_refuses_a_config_it_cannot_read_but_show_still_works() {
    // A merge needs something to merge into. On a config that does not
    // deserialise there is nothing to preserve and nothing to compare, so
    // the setter refuses and names the verbs that do work — which is the
    // whole reason `show` and `validate` deliberately avoid the model.
    let dir = tempdir().unwrap();
    seed(dir.path());
    break_config(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be read"));

    cdno(dir.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicates::str::contains("garbage = ["));
}

#[test]
fn a_hand_authored_list_key_survives_an_unrelated_field_edit() {
    // `set_schema_field` deliberately does not write `list`: the key is
    // unimplemented, so a hand-authored value is left alone. The CLI
    // carries it through the merge rather than inventing a flag for a key
    // nothing reads yet, so editing the field must not drop it.
    //
    // `list = false` rather than `true` because `true` is not a reachable
    // state at all: the save gate rejects it as unimplemented, so a config
    // carrying it never validates and the vault never opens. `false` is the
    // only value this key can actually hold, which is what makes it worth
    // pinning — an edit that dropped it would be silent.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let cfg = config_path(dir.path());
    let mut content = fs::read_to_string(&cfg).unwrap();
    content.push_str("\n[schemas.project.fields.tags]\ntype = \"string\"\nlist = false\n");
    fs::write(&cfg, content).unwrap();

    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "project",
            "--field",
            "tags",
            "--required",
        ])
        .assert()
        .success();

    let after = fs::read_to_string(&cfg).unwrap();
    assert!(
        after.contains("list = false"),
        "a hand-authored `list` must survive an unrelated edit:\n{after}"
    );
    assert!(after.contains("required = true"), "the edit applied");
}

#[test]
fn an_unimplemented_list_true_is_refused_rather_than_written_through() {
    // The other half, and the reason the test above uses `false`: a config
    // that already carries `list = true` cannot be edited into a worse
    // state by accident, because the gate refuses the whole save. Worth
    // pinning so the error stays a readable refusal rather than becoming a
    // silent write if `list` is ever implemented without revisiting this.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let cfg = config_path(dir.path());
    let mut content = fs::read_to_string(&cfg).unwrap();
    content.push_str("\n[schemas.project.fields.tags]\ntype = \"string\"\nlist = true\n");
    fs::write(&cfg, content).unwrap();
    let before = fs::read_to_string(&cfg).unwrap();

    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "project",
            "--field",
            "tags",
            "--required",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not yet implemented"));

    assert_eq!(
        fs::read_to_string(&cfg).unwrap(),
        before,
        "the refusal must leave the file untouched"
    );
}

#[test]
fn a_default_is_parsed_against_the_fields_own_type() {
    use cdno_cli::commands::config::{parse_default, parse_field_type};

    // Typed here rather than left to the save gate, so the error names the
    // flag and the expected form instead of surfacing as a TOML complaint.
    let int = parse_field_type("int").unwrap();
    assert!(parse_default("30", int).is_ok());
    assert!(
        parse_default("not-a-number", int).is_err(),
        "an int default must reject prose"
    );

    let date = parse_field_type("date").unwrap();
    assert!(parse_default("2026-09-22", date).is_ok());
    assert!(
        parse_default("22/09/2026", date).is_err(),
        "a date default must be YYYY-MM-DD"
    );

    // A string default takes anything, including what other types reject.
    let string = parse_field_type("string").unwrap();
    assert!(parse_default("not-a-number", string).is_ok());

    assert!(parse_field_type("banana").is_err());
}

#[test]
fn a_boolean_key_survives_an_unrelated_edit_too() {
    // The bool arm of the same merge, and it needs its own test: the list
    // and optional-scalar cases above both pass while `merge_flag` ignores
    // the current value entirely, because neither touches a bool. Without
    // this, an edit to a note type's folder silently cleared its
    // `append_only` mark, and an edit to a field's default silently cleared
    // its `required` — the identical data loss, one type down.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
            "--append-only",
        ])
        .assert()
        .success();
    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "humans",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        content.contains("append_only = true"),
        "append_only must survive a folder-only edit:\n{content}"
    );

    // And the same for a schema field's `required`, the other caller.
    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "people",
            "--field",
            "age",
            "--type",
            "int",
            "--required",
        ])
        .assert()
        .success();
    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "people",
            "--field",
            "age",
            "--default",
            "30",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        content.contains("required = true"),
        "required must survive a default-only edit:\n{content}"
    );
    assert!(
        content.contains("default = 30"),
        "the edit applied:\n{content}"
    );
}

#[test]
fn an_explicit_no_flag_clears_a_boolean() {
    // The escape hatch that makes the merge above usable: if an omitted
    // flag keeps the current value, there has to be a way to say "off".
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--folder",
            "people",
            "--append-only",
        ])
        .assert()
        .success();
    cdno(dir.path())
        .args([
            "config",
            "note-type",
            "set",
            "--name",
            "people",
            "--no-append-only",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        !content.contains("append_only"),
        "--no-append-only clears the key:\n{content}"
    );
}

#[test]
fn the_setter_flags_survive_an_unrelated_edit() {
    // `settable` and `log_on_change` are the two `Option<bool>` keys, and
    // `set_schema_field`'s own documentation is explicit that "the caller,
    // not the writer, owns preservation" of them: the desktop form keeps
    // them by lifting the current value into the spec and re-sending it.
    // This CLI is that caller, so the contract is ours to keep — and the
    // bool test above does not cover it, because `merge_tri` is a separate
    // arm that can drop the current value while `merge_flag` is correct.
    let dir = tempdir().unwrap();
    seed(dir.path());

    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "project",
            "--field",
            "mood",
            "--type",
            "string",
            "--settable",
            "--log-on-change",
        ])
        .assert()
        .success();

    // An edit that names neither flag must round-trip both.
    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "project",
            "--field",
            "mood",
            "--default",
            "calm",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        content.contains("settable = true"),
        "settable must survive an unrelated edit:\n{content}"
    );
    assert!(
        content.contains("log_on_change = true"),
        "log_on_change must survive an unrelated edit:\n{content}"
    );
    assert!(
        content.contains("default = \"calm\""),
        "the edit applied:\n{content}"
    );

    // And clearing one leaves the other alone.
    cdno(dir.path())
        .args([
            "config",
            "field",
            "set",
            "--note-type",
            "project",
            "--field",
            "mood",
            "--no-settable",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        !content.contains("settable = true"),
        "--no-settable cleared it"
    );
    assert!(
        content.contains("log_on_change = true"),
        "clearing one setter flag must not clear the other:\n{content}"
    );
}
