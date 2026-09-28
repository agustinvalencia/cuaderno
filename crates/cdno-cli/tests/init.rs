//! In-process tests for `commands::init::run`.
//!
//! Calls the library function directly against a tempdir so coverage
//! is tracked. Binary-level concerns (exit codes, CWD defaulting,
//! argument parsing) live in `tests/cli.rs` instead.

use std::fs;

use cdno_cli::commands::init;
use cdno_cli::commands::init::{CONCEPT_TEMPLATE, CONCEPT_TYPE_BLOCK};
use chrono::{Datelike, Local};
use tempfile::tempdir;

const EMBEDDED_DAILY: &str = include_str!("../templates/daily.md");

/// The `cdno-domain` built-in daily template — the fallback used when no
/// custom one exists.
const DOMAIN_DAILY: &str = include_str!("../../cdno-domain/templates/daily.md");

#[test]
fn init_daily_seed_matches_the_domain_builtin_template() {
    // `cdno init` seeds `daily.md` into `.cuaderno/templates/`, where it
    // becomes an active custom override (daily is template-driven, #212).
    // If it drifted from the built-in default, an init'd vault would
    // render dailies differently from a non-init'd one. Pin them equal.
    assert_eq!(
        EMBEDDED_DAILY, DOMAIN_DAILY,
        "the init daily seed must stay byte-identical to the cdno-domain built-in daily template"
    );
}

#[test]
fn run_creates_full_directory_tree_with_current_year_partitions() {
    let dir = tempdir().unwrap();
    let target = dir.path();

    let now = Local::now().date_naive();
    let year = now.year();
    let iso_year = now.iso_week().year();

    init::run(target).expect("init succeeds on fresh dir");

    let exists = |rel: &str| target.join(rel).is_dir();
    assert!(exists(&format!("journal/{year}/daily")));
    assert!(exists(&format!("journal/{iso_year}/weekly")));
    assert!(exists(&format!("journal/{year}/monthly")));
    assert!(exists("projects"));
    assert!(exists("projects/_parked"));
    assert!(exists("portfolios"));
    assert!(exists("stewardships"));
    assert!(exists("commitments"));
    assert!(exists(&format!("commitments/_done/{year}")));
    assert!(exists("questions/research"));
    assert!(exists("questions/life"));
    assert!(exists("inbox"));
    assert!(exists(".cuaderno"));
    assert!(exists(".cuaderno/templates"));
}

#[test]
fn run_writes_default_config_with_five_project_cap() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let config =
        fs::read_to_string(dir.path().join(".cuaderno/config.toml")).expect("config.toml present");
    assert!(config.contains("[vault]"));
    assert!(config.contains("max_active_projects = 5"));
}

#[test]
fn run_dumps_daily_template_byte_identical_to_embedded() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let dumped = fs::read_to_string(dir.path().join(".cuaderno/templates/daily.md"))
        .expect("daily.md dumped");
    assert_eq!(dumped, EMBEDDED_DAILY);
}

#[test]
fn run_refuses_when_cuaderno_dir_already_exists() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let err = init::run(dir.path()).expect_err("re-init must fail");
    let msg = format!("{err}");
    assert!(msg.contains("already exists"), "unexpected error: {msg}");
}

#[test]
fn init_writes_concept_type_and_folder() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init succeeds on fresh dir");

    let config =
        fs::read_to_string(dir.path().join(".cuaderno/config.toml")).expect("config.toml present");
    assert!(config.contains("[note_types.concept]"));
    assert!(config.contains(r#"folder = "concepts""#));
    assert!(dir.path().join("concepts").is_dir());

    // A vault with the block still opens (proves it parses and validates).
    let (vault, _report) =
        cdno_cli::bootstrap::open_vault(dir.path()).expect("vault opens with concept declared");

    // The empty type lists with no error.
    let notes = vault
        .list_custom_notes("concept")
        .expect("listing the empty concept type succeeds");
    assert!(notes.is_empty());
}

#[test]
fn init_concept_block_is_deletable() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init succeeds on fresh dir");

    let config_path = dir.path().join(".cuaderno/config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let without_block = config.replace(CONCEPT_TYPE_BLOCK, "");
    assert_ne!(without_block, config, "precondition: block was present");
    fs::write(&config_path, without_block).unwrap();

    // The vault still opens with the block gone — it was an ordinary,
    // deletable custom type, nothing else depends on it.
    cdno_cli::bootstrap::open_vault(dir.path()).expect("vault opens with concept block removed");
}

#[test]
fn run_creates_target_directory_when_missing() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("nested-vault");
    assert!(!target.exists(), "precondition: target absent");

    init::run(&target).expect("init creates missing parent");

    assert!(target.join(".cuaderno").is_dir());
    assert!(target.join("inbox").is_dir());
}

/// A file from `examples/note-types/concept/`, the copy of the concept type
/// an existing vault adopts by hand (RFC 0002 §6.1, T14). A new vault gets
/// the same two files from `cdno init`.
fn concept_example(file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/note-types/concept")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// A `cdno` subprocess pinned to `vault`, with any exported vault path removed
/// so the flag is the only resolution in play.
fn cdno_in(vault: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.arg("--vault").arg(vault);
    cmd
}

/// Create a concept on `vault` with a one-line body and an optional origin,
/// returning the written file's content.
fn create_concept(
    vault: &std::path::Path,
    title: &str,
    slug: &str,
    origin: Option<&str>,
) -> String {
    let body_file = vault.join(format!("{slug}-body.md"));
    fs::write(&body_file, "A rank-k correction to an inverse.\n").unwrap();
    let mut cmd = cdno_in(vault);
    cmd.args([
        "--no-interactive",
        "note",
        "create",
        "concept",
        "--title",
        title,
    ])
    .arg("--body-file")
    .arg(&body_file);
    if let Some(origin) = origin {
        cmd.args(["--origin", origin]);
    }
    cmd.assert().success();
    fs::read_to_string(vault.join(format!("concepts/{slug}.md"))).unwrap()
}

/// Every entry under `root`, relative, with its content (`None` for a
/// directory), sorted. The whole-vault snapshot the init/install parity
/// tests compare.
fn snapshot(root: &std::path::Path) -> Vec<(String, Option<Vec<u8>>)> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut Vec<(String, Option<Vec<u8>>)>,
    ) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.push((rel, None));
                walk(root, &path, out);
            } else {
                out.push((rel, Some(fs::read(&path).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Turn a freshly initialised vault into one made by a binary that predates
/// the concept type (#644): no block, no template, no folder.
fn strip_concept(root: &std::path::Path) {
    let config_path = root.join(".cuaderno/config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let stripped = config.replace(CONCEPT_TYPE_BLOCK, "");
    assert_ne!(stripped, config, "precondition: the block was present");
    fs::write(&config_path, stripped).unwrap();
    fs::remove_file(root.join(".cuaderno/templates/concept.md")).unwrap();
    fs::remove_dir(root.join("concepts")).unwrap();
}

#[test]
fn example_matches_init() {
    // The bundled registry `include_str!`s the example files, so there is one
    // copy on disk. What is left to assert is what the install produces: a
    // config `cdno config validate` accepts, and the same files whether the
    // type came from `cdno init` or from installing it into an older vault.
    let fresh = tempdir().unwrap();
    init::run(fresh.path()).unwrap();
    cdno_in(fresh.path())
        .args(["config", "validate"])
        .assert()
        .success();

    // The installed files are the example files, byte for byte.
    let config = fs::read_to_string(fresh.path().join(".cuaderno/config.toml")).unwrap();
    assert!(
        config.ends_with(&concept_example("config.toml")),
        "the config init writes must end with the example block:\n{config}"
    );
    let installed = fs::read_to_string(fresh.path().join(".cuaderno/templates/concept.md"))
        .expect("init installs concept.md");
    assert_eq!(installed, concept_example("concept.md"));
    assert_eq!(CONCEPT_TYPE_BLOCK, concept_example("config.toml"));
    assert_eq!(CONCEPT_TEMPLATE, concept_example("concept.md"));

    // An older vault, upgraded by the install function, ends up identical.
    let older = tempdir().unwrap();
    init::run(older.path()).unwrap();
    strip_concept(older.path());
    cdno_cli::bundled::install_bundled(older.path(), "concept").expect("install into older vault");
    cdno_in(older.path())
        .args(["config", "validate"])
        .assert()
        .success();
    assert_eq!(
        snapshot(older.path()),
        snapshot(fresh.path()),
        "init and install must produce the same files"
    );
}

#[test]
fn example_concept_template_takes_body_and_origin() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    // Install the example by hand, as an existing vault would.
    fs::write(
        dir.path().join(".cuaderno/templates/concept.md"),
        concept_example("concept.md"),
    )
    .unwrap();

    // `templates vars concept` lists the caller-supplied placeholders.
    let out = cdno_in(dir.path())
        .args(["--json", "templates", "vars", "concept"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = rows.iter().filter_map(|r| r["name"].as_str()).collect();
    for expected in ["title", "created", "body", "origin"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    // Create with a body and a two-link origin (promotion is create-with-origin).
    let origin = "[[journal/2026/daily/2026-09-02#Woodbury identity]] \
                  [[journal/2026/daily/2026-09-24#Low-rank refit]]";
    let content = create_concept(
        dir.path(),
        "Woodbury identity",
        "woodbury-identity",
        Some(origin),
    );
    let (fm, rest) = cdno_core::frontmatter::Frontmatter::parse(&content)
        .unwrap_or_else(|e| panic!("frontmatter parses: {e}\n{content}"));
    let string = |key: &str| fm.optional_field::<String>(key).unwrap();
    assert_eq!(string("type").as_deref(), Some("concept"), "{content}");
    let json = fm.as_json();
    assert!(json.get("created").is_some(), "{content}");
    assert!(json.get("tags").is_some(), "{content}");
    // The title is the body H1, not a frontmatter key (RFC 0002 §6.1).
    assert!(json.get("title").is_none(), "no title key: {content}");
    // Reconciliation appends the supplied origin as a string, after `tags`.
    assert_eq!(string("origin").as_deref(), Some(origin), "{content}");
    let tags_at = content.find("\ntags:").expect("tags line");
    let origin_at = content.find("\norigin:").expect("origin line");
    assert!(tags_at < origin_at, "origin in declared order: {content}");

    // The body sits under the H1 and above the fallback sections.
    let h1 = rest.find("# Woodbury identity").expect("H1");
    let body = rest.find("A rank-k correction").expect("body");
    let statement = rest.find("## Statement").expect("## Statement");
    assert!(h1 < body && body < statement, "{rest}");
    assert!(rest.contains("## Why it matters") && rest.contains("## See also"));

    // The frontmatter is in canonical order, so the note is lint-clean,
    // warnings included.
    cdno_in(dir.path())
        .args(["lint", "--strict"])
        .assert()
        .success();
}

#[test]
fn concept_created_without_origin_has_no_origin_key_or_placeholder() {
    // The ordinary, non-promotion path: no `--origin`. The template carries
    // no `origin` line, so nothing is left for the engine to keep literally.
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let content = create_concept(dir.path(), "Sherman Morrison", "sherman-morrison", None);
    let (fm, _) = cdno_core::frontmatter::Frontmatter::parse(&content)
        .unwrap_or_else(|e| panic!("frontmatter parses: {e}\n{content}"));
    assert!(
        fm.as_json().get("origin").is_none(),
        "no origin key: {content}"
    );
    assert!(
        !content.contains("{{"),
        "no unresolved placeholder: {content}"
    );
    assert!(!content.contains("origin"), "{content}");
}

#[test]
fn concept_title_with_a_colon_keeps_valid_frontmatter() {
    // A `title: {{title}}` line would render as `title: Ratio: A vs B`, which
    // is not YAML; the rebuild would then drop `tags`. With the title only in
    // the H1, the frontmatter is untouched by it.
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let content = create_concept(dir.path(), "Ratio: A vs B", "ratio-a-vs-b", None);
    let (fm, rest) = cdno_core::frontmatter::Frontmatter::parse(&content)
        .unwrap_or_else(|e| panic!("frontmatter parses: {e}\n{content}"));
    let json = fm.as_json();
    assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("concept"));
    assert!(json.get("created").is_some(), "{content}");
    assert!(json.get("tags").is_some(), "tags survive: {content}");
    assert!(json.get("title").is_none(), "{content}");
    assert_eq!(
        rest.lines().find(|l| l.starts_with("# ")),
        Some("# Ratio: A vs B"),
        "the H1 is the title: {content}"
    );

    cdno_in(dir.path())
        .args(["lint", "--strict"])
        .assert()
        .success();
}

#[test]
fn fresh_init_vault_creates_concepts_from_the_installed_template() {
    // No manual copy: `cdno init` installed `concept.md`, so the fallback
    // sections appear and the body lands above them.
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    assert!(dir.path().join(".cuaderno/templates/concept.md").is_file());

    let content = create_concept(dir.path(), "Woodbury identity", "woodbury-identity", None);
    let body = content.find("A rank-k correction").expect("body");
    let statement = content.find("## Statement").expect("## Statement");
    assert!(body < statement, "{content}");
    assert!(content.contains("## Why it matters") && content.contains("## See also"));
}
