//! In-process tests for `cdno templates vars`. Seed a vault on disk, then
//! assert on the `placeholders` data seam / `render_table` / `json_rows`
//! rather than capturing stdout (same pattern as `cdno search`).

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, templates};
use tempfile::tempdir;

fn seed(root: &Path) {
    init::run(root).expect("init");
}

#[test]
fn templates_vars_lists_the_project_supplied_placeholders() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    // The acceptance set for `project` (#271).
    assert_eq!(
        names,
        ["title", "context", "status", "created", "core_question"]
    );

    let table = templates::render_table(&ph);
    assert!(table.contains("{{title}}"), "table:\n{table}");
    assert!(table.contains("supplied"), "table:\n{table}");
}

#[test]
fn templates_vars_json_rows_are_a_stable_array() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
    let rows = templates::json_rows(&ph);
    assert_eq!(rows.len(), 5, "project supplies five placeholders");
    assert_eq!(rows[0]["name"], "title");
    assert_eq!(rows[0]["source"], "supplied");
    // `message` is only present on prompt rows.
    assert!(rows[0].get("message").is_none());
}

#[test]
fn templates_vars_surfaces_config_and_prompt_vars() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    // Append config variables to the seeded vault's config.
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut body = fs::read_to_string(&cfg).unwrap();
    body.push_str("\n[variables]\nauthor = \"A. Researcher\"\n\n[variables.prompt]\nticket = \"Ticket ID?\"\n");
    fs::write(&cfg, body).unwrap();

    let ph = templates::placeholders(dir.path(), "project").expect("placeholders");
    let rows = templates::json_rows(&ph);
    let author = rows
        .iter()
        .find(|r| r["name"] == "author")
        .expect("author listed");
    assert_eq!(author["source"], "config");
    let ticket = rows
        .iter()
        .find(|r| r["name"] == "ticket")
        .expect("ticket listed");
    assert_eq!(ticket["source"], "prompt");
    assert_eq!(ticket["message"], "Ticket ID?");
}

#[test]
fn templates_vars_tracking_lists_the_complete_supplied_set() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // The supplied set is the type's full create-path key set (#279), so it
    // includes `routine` and `activity_title` even though the generic built-in
    // template doesn't reference them.
    let ph = templates::placeholders(dir.path(), "tracking").expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"activity_title"), "supplied set: {names:?}");
    assert!(names.contains(&"routine"), "supplied set: {names:?}");
}

#[test]
fn templates_vars_rejects_an_unknown_type() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = templates::placeholders(dir.path(), "bogus").expect_err("should error");
    let msg = err.to_string();
    assert!(msg.contains("unknown note type"), "msg: {msg}");
    assert!(msg.contains("project"), "should list valid types: {msg}");
}

#[test]
fn templates_eject_materialises_the_builtin() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let path = templates::eject(dir.path(), "project", false).expect("eject");
    assert_eq!(path, ".cuaderno/templates/project.md");
    let content = fs::read_to_string(dir.path().join(&path)).unwrap();
    // Byte-identical to the built-in — the guarantee the docs make (a note
    // created straight after ejecting is unchanged). Compares the on-disk FS
    // write against the compiled-in source template.
    assert_eq!(
        content,
        include_str!("../../cdno-domain/templates/project.md"),
        "ejected file must be byte-identical to the built-in"
    );
}

#[test]
fn templates_eject_refuses_to_clobber_then_force_overwrites() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let target = dir.path().join(".cuaderno/templates/project.md");
    fs::write(&target, "# mine\n").unwrap();

    let err = templates::eject(dir.path(), "project", false).expect_err("should refuse");
    assert!(err.to_string().contains("already exists"), "msg: {err}");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# mine\n",
        "left untouched"
    );

    templates::eject(dir.path(), "project", true).expect("force eject");
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("## Current State")
    );
}

#[test]
fn templates_eject_tracking_writes_the_generic_template() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // Only base note-type templates eject (no `--variant` flag): the generic
    // tracking template is written. Activity variants are authored in the
    // vault, not ejected.
    let path = templates::eject(dir.path(), "tracking", false).expect("base tracking");
    assert_eq!(path, ".cuaderno/templates/tracking.md");
    assert!(dir.path().join(&path).exists());
}

#[test]
fn templates_eject_all_writes_every_type_skipping_existing() {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use cdno_domain::note_type::NoteType;

    let dir = tempdir().unwrap();
    seed(dir.path());
    let templates_dir = dir.path().join(".cuaderno/templates");

    // Whatever `init` pre-seeds (today just daily.md) is exactly what `--all`
    // skips — derive it rather than hardcode, so this survives init growing.
    let preseeded: BTreeSet<String> = std::fs::read_dir(&templates_dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|stem| NoteType::from_str(stem).is_ok())
        .collect();

    let report = templates::eject_all(dir.path(), false).expect("eject all");
    let written: BTreeSet<String> = report.written.into_iter().collect();
    let skipped: BTreeSet<String> = report.skipped.into_iter().collect();

    assert_eq!(skipped, preseeded, "skips exactly the pre-seeded templates");
    assert!(
        written.is_disjoint(&skipped),
        "written and skipped are disjoint"
    );
    assert_eq!(
        written.len() + skipped.len(),
        12,
        "every type accounted for"
    );
    // Every type now has a template file on disk.
    for nt in NoteType::ALL {
        assert!(
            templates_dir.join(format!("{}.md", nt.as_str())).exists(),
            "missing {}.md",
            nt.as_str()
        );
    }
}

#[test]
fn templates_eject_all_rerun_skips_everything() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    templates::eject_all(dir.path(), false).expect("first");
    let second = templates::eject_all(dir.path(), false).expect("second");
    assert!(second.written.is_empty(), "everything already exists");
    assert_eq!(second.skipped.len(), 12);
}

#[test]
fn templates_eject_all_force_overwrites_a_customised_template() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let project = dir.path().join(".cuaderno/templates/project.md");
    fs::write(&project, "# mine\n").unwrap();

    let report = templates::eject_all(dir.path(), true).expect("force eject all");
    assert_eq!(report.written.len(), 12, "force writes all, none skipped");
    assert!(report.skipped.is_empty());
    // The customised project.md was overwritten with the built-in.
    assert!(
        fs::read_to_string(&project)
            .unwrap()
            .contains("## Current State"),
        "project.md overwritten"
    );
}

/// Init a vault and register a `person` custom type.
fn seed_with_person(root: &Path) {
    init::run(root).expect("init");
    let cfg = root.join(".cuaderno/config.toml");
    let mut content = fs::read_to_string(&cfg).unwrap_or_default();
    content.push_str(
        "\n[note_types.person]\nfolder = \"people\"\nrequired = [\"name\"]\noptional = [\"role\"]\n",
    );
    fs::write(&cfg, content).unwrap();
}

#[test]
fn templates_vars_lists_a_custom_type_supplied_set() {
    let dir = tempdir().unwrap();
    seed_with_person(dir.path());

    let ph = templates::placeholders(dir.path(), "person").expect("placeholders");
    let names: Vec<&str> = ph.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["title", "slug", "created", "date", "name", "role"]);
}

#[test]
fn templates_eject_refuses_a_custom_type() {
    // A custom type has no built-in template to eject.
    let dir = tempdir().unwrap();
    seed_with_person(dir.path());

    let err = templates::eject(dir.path(), "person", false).expect_err("should refuse");
    assert!(
        err.to_string().contains("no built-in template to eject"),
        "err: {err}"
    );
}

#[test]
fn templates_vars_unknown_type_lists_custom_names() {
    // The friendly error's valid set now includes registered custom types.
    let dir = tempdir().unwrap();
    seed_with_person(dir.path());

    let err = templates::placeholders(dir.path(), "gadget").expect_err("should error");
    assert!(err.to_string().contains("person"), "err: {err}");
}

// ---------------------------------------------------------------------------
// list / show / save / new (#599)
// ---------------------------------------------------------------------------
//
// The other half of the template story, which lived only in the desktop app's
// Templates view until the retirement (#597). Asserted on the data and write
// seams, in the file's established pattern.

/// Register a config-defined custom type, which is what `new` is for.
fn add_custom_type(root: &Path) {
    let cfg = root.join(".cuaderno").join("config.toml");
    let mut content = fs::read_to_string(&cfg).unwrap();
    content.push_str("\n[note_types.people]\nfolder = \"people\"\nrequired = [\"name\"]\n");
    fs::write(&cfg, content).unwrap();
}

#[test]
fn show_reads_back_exactly_what_eject_wrote() {
    // #599's own probe, and the reason it is the one that matters: it
    // proves the two halves of the story agree byte for byte, so a
    // customisation workflow that ejects, edits and re-reads cannot drift.
    let dir = tempdir().unwrap();
    seed(dir.path());

    templates::eject(dir.path(), "project", false).expect("eject");
    let on_disk = fs::read_to_string(
        dir.path()
            .join(".cuaderno")
            .join("templates")
            .join("project.md"),
    )
    .unwrap();

    let shown = templates::template_content(dir.path(), "project", None).expect("show");
    assert_eq!(
        shown.content, on_disk,
        "show must be byte-identical to the ejected file"
    );
}

#[test]
fn list_covers_every_built_in_and_reports_the_source_in_effect() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let rows = templates::summaries(dir.path()).expect("list");
    let project = rows
        .iter()
        .find(|r| r.note_type == "project")
        .expect("project is listed");
    assert!(!project.is_custom_type);
    assert!(
        !project.has_custom_file,
        "a fresh vault has no project override"
    );

    // Ejecting flips the source from the built-in default to the override,
    // which is the state change the list exists to show.
    templates::eject(dir.path(), "project", false).expect("eject");
    let rows = templates::summaries(dir.path()).expect("list again");
    let project = rows.iter().find(|r| r.note_type == "project").unwrap();
    assert!(
        project.has_custom_file,
        "the ejected override must show up in the list"
    );
}

#[test]
fn saving_a_built_in_creates_its_override_without_an_eject_first() {
    // `save_template` transparently creates the override — the desktop's
    // direct edit-and-save model. Pinned because a `save` that required a
    // prior `eject` would be a silently worse CLI than the view it replaces.
    let dir = tempdir().unwrap();
    seed(dir.path());
    let body = "---\ntype: action\n---\n\n# {{title}}\n\nmine\n";

    let path = templates::save_content(dir.path(), "action", None, body).expect("save");
    assert!(path.contains("action.md"), "wrote the override: {path}");
    assert_eq!(
        templates::template_content(dir.path(), "action", None)
            .unwrap()
            .content,
        body,
        "show must read back exactly what save wrote"
    );
}

#[test]
fn new_scaffolds_a_custom_type_and_refuses_a_second_one() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    add_custom_type(dir.path());

    let path = templates::create(dir.path(), "people").expect("scaffold");
    assert!(path.contains("people.md"), "{path}");

    let content = templates::template_content(dir.path(), "people", None)
        .unwrap()
        .content;
    assert!(
        content.contains("type: people"),
        "the starter declares its type:\n{content}"
    );
    assert!(
        content.contains("{{name}}"),
        "each declared required field becomes a placeholder:\n{content}"
    );

    // Idempotency is deliberately NOT the contract here: a second scaffold
    // would silently overwrite an author's work, so it errors instead.
    assert!(
        templates::create(dir.path(), "people").is_err(),
        "scaffolding over an existing template must refuse"
    );
}

#[test]
fn new_on_a_built_in_explains_itself_in_terms_of_templates() {
    // The domain's own `BuiltinTypeNotCustom` message is about creating a
    // NOTE of a built-in type — "use `cdno project create`" — which is
    // right for `create_note` and sends someone running `templates new`
    // entirely the wrong way. The CLI rephrases it, so this pins the
    // rephrasing rather than the domain's wording.
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err =
        templates::create(dir.path(), "project").expect_err("a built-in has nothing to scaffold");
    let message = format!("{err}");
    assert!(
        message.contains("templates eject") || message.contains("templates save"),
        "the error must name the verb that does work here, got: {message}"
    );
    assert!(
        !message.contains("project create"),
        "it must not send the user to the note-creation verb, got: {message}"
    );
}

#[test]
fn an_unknown_type_is_refused_by_every_verb() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    assert!(templates::template_content(dir.path(), "nosuch", None).is_err());
    assert!(templates::save_content(dir.path(), "nosuch", None, "x").is_err());
    assert!(templates::create(dir.path(), "nosuch").is_err());
}

#[test]
fn show_prints_verbatim_with_no_added_newline() {
    // Deliberately through the binary rather than the seam above. #599's
    // probe is `templates show project | diff - .cuaderno/templates/project.md`,
    // which is a claim about STDOUT: swapping `print!` for `println!` leaves
    // the data seam byte-identical and still breaks the probe, so a seam
    // test cannot catch it.
    use assert_cmd::Command;

    let dir = tempdir().unwrap();
    seed(dir.path());
    templates::eject(dir.path(), "project", false).expect("eject");
    let on_disk = fs::read_to_string(
        dir.path()
            .join(".cuaderno")
            .join("templates")
            .join("project.md"),
    )
    .unwrap();

    let out = Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .args(["--vault"])
        .arg(dir.path())
        .args(["templates", "show", "project"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(out).unwrap(),
        on_disk,
        "stdout must diff clean against the file on disk"
    );
}

#[test]
fn save_without_input_off_a_terminal_errors_rather_than_blanking() {
    // The failure mode worth pinning: `save` with nothing to save must not
    // resolve to an empty string. Off a terminal there is no editor to
    // open, so the absent flag is an error — otherwise a scripted
    // `templates save --note-type action` would silently truncate the
    // template to zero bytes and report success.
    use assert_cmd::Command;

    let dir = tempdir().unwrap();
    seed(dir.path());
    let body = "---\ntype: action\n---\n\n# {{title}}\n\nkeep me\n";
    templates::save_content(dir.path(), "action", None, body).expect("seed a template");

    Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .args(["--vault"])
        .arg(dir.path())
        .args([
            "--no-interactive",
            "templates",
            "save",
            "--note-type",
            "action",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("flag: --file"));

    assert_eq!(
        templates::template_content(dir.path(), "action", None)
            .unwrap()
            .content,
        body,
        "the refused save must leave the template intact"
    );
}

#[test]
fn save_reads_the_template_from_stdin_when_the_file_is_a_dash() {
    // `--file -` is documented in the flag's own help, so it is behaviour
    // rather than an accident of path handling: without the sentinel, `-`
    // is looked up as a literal filename and the save fails with a
    // no-such-file error. Piping is the natural way to script this verb,
    // so it gets a test rather than only a mention.
    use assert_cmd::Command;

    let dir = tempdir().unwrap();
    seed(dir.path());
    let body = "---\ntype: question\n---\n\n# {{title}}\n\npiped\n";

    Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .args(["--vault"])
        .arg(dir.path())
        .args([
            "--no-interactive",
            "templates",
            "save",
            "--note-type",
            "question",
            "--file",
            "-",
        ])
        .write_stdin(body)
        .assert()
        .success();

    assert_eq!(
        templates::template_content(dir.path(), "question", None)
            .unwrap()
            .content,
        body,
        "stdin must be written verbatim"
    );
}
