//! Tests for editor resolution and execution.
//!
//! Everything up to `spawn` is pure, so the precedence rules, the tokeniser,
//! and the placeholder substitution are asserted directly rather than through
//! a terminal. `spawn` itself is exercised against `/bin/true` and
//! `/bin/false`, which is enough to pin the exit-code contract.
//!
//! The environment is injected rather than read: edition 2024 makes
//! `std::env::set_var` `unsafe`, and it is racy across cargo's test threads.

use std::collections::HashMap;
use std::path::Path;

use cdno_cli::editor::{EditorCommand, EditorSource, parse_template, resolve, substitute_path};

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

fn argv_of(cmd: &EditorCommand) -> Vec<String> {
    match cmd {
        EditorCommand::Argv { argv, .. } => argv.clone(),
        other => panic!("expected Argv, got {other:?}"),
    }
}

fn source_of(cmd: &EditorCommand) -> EditorSource {
    match cmd {
        EditorCommand::Argv { source, .. } | EditorCommand::Uri { source, .. } => *source,
        other => panic!("expected a configured editor, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Resolution order
// ---------------------------------------------------------------------

#[test]
fn the_flag_outranks_everything() {
    let env = env_of(&[("CUADERNO_EDITOR", "env-one"), ("EDITOR", "env-two")]);
    let cmd = resolve(Some("flag-editor"), &env, None).unwrap();

    assert_eq!(argv_of(&cmd), vec!["flag-editor"]);
    assert_eq!(source_of(&cmd), EditorSource::Flag);
}

#[test]
fn the_cuaderno_env_var_outranks_visual_and_editor() {
    let env = env_of(&[
        ("CUADERNO_EDITOR", "env-editor"),
        ("VISUAL", "visual-editor"),
        ("EDITOR", "fallback"),
    ]);
    let cmd = resolve(None, &env, None).unwrap();

    assert_eq!(argv_of(&cmd), vec!["env-editor"]);
    assert_eq!(source_of(&cmd), EditorSource::EnvCuaderno);
}

#[test]
fn visual_outranks_editor() {
    let env = env_of(&[("VISUAL", "visual-editor"), ("EDITOR", "plain-editor")]);
    let cmd = resolve(None, &env, None).unwrap();

    assert_eq!(argv_of(&cmd), vec!["visual-editor"]);
    assert_eq!(source_of(&cmd), EditorSource::EnvVisual);
}

#[test]
fn nothing_configured_falls_back_to_the_platform_default() {
    let env = env_of(&[]);
    assert_eq!(
        resolve(None, &env, None).unwrap(),
        EditorCommand::PlatformDefault
    );
}

/// The security property, pinned as a test rather than left to a comment.
///
/// Every source `resolve` consults must be one the person at the keyboard
/// controls. A vault can be cloned, so nothing read from `.cuaderno/` may
/// reach here — reintroducing a vault-config source would make
/// `cdno open` execute a cloned repository's choice of program.
///
/// Constraining the value's shape would not rescue such a source: a bare
/// binary name with no arguments still admits `sh`, which runs `sh <the
/// note>` and so executes the note's own contents. The provenance is the
/// control, not the syntax.
#[test]
fn every_editor_source_is_one_the_user_controls() {
    // The full set `resolve` reads, enumerated so that adding a source
    // without thinking about provenance fails here.
    let sources = [
        EditorSource::Flag,
        EditorSource::EnvCuaderno,
        EditorSource::EnvVisual,
        EditorSource::EnvEditor,
    ];
    for source in sources {
        let described = source.describe();
        assert!(
            described.starts_with("--") || described.starts_with('$'),
            "editor source {described:?} is neither a flag nor an environment \
             variable — if it reads vault-resident data, `cdno open` can be \
             made to run a cloned repository's program"
        );
    }
}

/// `EDITOR=""` is common in stripped CI images. Treating it as set would
/// produce an empty argv rather than falling through to something usable.
#[test]
fn a_blank_source_falls_through_rather_than_resolving_to_nothing() {
    let env = env_of(&[("CUADERNO_EDITOR", "   "), ("EDITOR", "real-editor")]);
    let cmd = resolve(None, &env, None).unwrap();

    assert_eq!(argv_of(&cmd), vec!["real-editor"]);
    assert_eq!(source_of(&cmd), EditorSource::EnvEditor);
}

// ---------------------------------------------------------------------
// Tokenising
// ---------------------------------------------------------------------

#[test]
fn a_template_keeps_its_arguments() {
    let cmd = parse_template("code -g {path}", EditorSource::EnvEditor, None).unwrap();
    assert_eq!(argv_of(&cmd), vec!["code", "-g", "{path}"]);
}

/// Quoting has to be honoured, or macOS application paths cannot be named.
#[test]
fn a_quoted_program_name_with_spaces_stays_one_argument() {
    let cmd = parse_template(
        "\"/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl\" -w {path}",
        EditorSource::EnvCuaderno,
        None,
    )
    .unwrap();

    assert_eq!(
        argv_of(&cmd),
        vec![
            "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl",
            "-w",
            "{path}"
        ]
    );
}

/// The security property, stated as a test: shell grammar is never honoured.
/// These are rejected rather than passed through, because a user who wrote
/// them expected `sh -c` semantics they will not get — and silently opening a
/// file called `&&` would be a worse answer than an error.
#[test]
fn unquoted_shell_metacharacters_are_refused() {
    for template in [
        "vim {path}; rm -rf /tmp/x",
        "vim {path} && echo done",
        "vim {path} | tee log",
        "vim $(whoami)",
        "vim `whoami`",
        "vim {path} > out",
    ] {
        let err = parse_template(template, EditorSource::EnvCuaderno, None)
            .expect_err(&format!("should refuse: {template}"));
        let msg = err.to_string();
        assert!(
            msg.contains("not through a shell"),
            "unhelpful message for {template}: {msg}"
        );
        // The message must name where the setting came from — with five
        // possible sources, otherwise the user has nowhere to go.
        assert!(msg.contains("$CUADERNO_EDITOR"), "no source named: {msg}");
    }
}

/// ...but a metacharacter *inside* quotes is just text, so a legitimate
/// program name is not collateral damage.
#[test]
fn a_quoted_metacharacter_is_allowed_through_as_text() {
    let cmd = parse_template(
        "\"/opt/My Editor (beta)/bin/edit\" {path}",
        EditorSource::EnvCuaderno,
        None,
    )
    .unwrap();

    assert_eq!(
        argv_of(&cmd),
        vec!["/opt/My Editor (beta)/bin/edit", "{path}"]
    );
}

#[test]
fn unbalanced_quotes_are_an_error_that_names_the_source() {
    let err = parse_template("\"unclosed {path}", EditorSource::EnvVisual, None).unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("unbalanced quotes"), "got: {msg}");
    assert!(msg.contains("$VISUAL"), "got: {msg}");
}

// ---------------------------------------------------------------------
// URI templates
// ---------------------------------------------------------------------

/// Detected syntactically — no executable name contains `://` — and detected
/// *before* tokenising, because a URI has no sensible argv decomposition.
#[test]
fn a_uri_template_is_never_tokenised() {
    let cmd = parse_template(
        "obsidian://open?path={path}",
        EditorSource::EnvCuaderno,
        None,
    )
    .unwrap();

    match cmd {
        EditorCommand::Uri { template, .. } => {
            assert_eq!(template, "obsidian://open?path={path}");
        }
        other => panic!("expected Uri, got {other:?}"),
    }
}

/// A URI carries `?` and `&`, which are metacharacters — so the URI check has
/// to come first or every URI template would be refused.
#[test]
fn a_uri_is_not_refused_for_containing_metacharacters() {
    assert!(
        parse_template(
            "obsidian://open?vault=v&file={path}",
            EditorSource::Flag,
            None
        )
        .is_ok()
    );
}

// ---------------------------------------------------------------------
// Path substitution
// ---------------------------------------------------------------------

#[test]
fn the_placeholder_is_replaced_in_place() {
    let argv = vec!["code".to_owned(), "-g".to_owned(), "{path}".to_owned()];
    assert_eq!(
        substitute_path(&argv, Path::new("/v/projects/surrogate-model.md")),
        vec!["code", "-g", "/v/projects/surrogate-model.md"]
    );
}

/// The append fallback is what makes a bare `EDITOR=nvim` work without the
/// user ever learning the placeholder exists — no `EDITOR` in the wild
/// carries one.
#[test]
fn a_template_without_the_placeholder_gets_the_path_appended() {
    let argv = vec!["nvim".to_owned()];
    assert_eq!(
        substitute_path(&argv, Path::new("/v/notes/a.md")),
        vec!["nvim", "/v/notes/a.md"]
    );
}

/// A path with spaces must stay one argv entry: substitution happens after
/// tokenising precisely so the filesystem cannot re-split the command.
#[test]
fn a_path_containing_spaces_stays_a_single_argument() {
    let argv = vec!["nvim".to_owned(), "{path}".to_owned()];
    let out = substitute_path(&argv, Path::new("/v/My Notes/a b.md"));

    assert_eq!(out.len(), 2);
    assert_eq!(out[1], "/v/My Notes/a b.md");
}

// ---------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------

#[test]
fn a_successful_editor_reports_zero() {
    let cmd = parse_template("/usr/bin/true", EditorSource::Flag, None).unwrap();
    assert_eq!(cmd.spawn(Path::new("/tmp/whatever.md")).unwrap(), Some(0));
}

/// A non-zero exit is a signal to pass on, not a cdno failure — it is how
/// `git commit` learns an edit was abandoned.
#[test]
fn a_failing_editor_reports_its_code_rather_than_erroring() {
    let cmd = parse_template("/usr/bin/false", EditorSource::Flag, None).unwrap();
    assert_eq!(cmd.spawn(Path::new("/tmp/whatever.md")).unwrap(), Some(1));
}

#[test]
fn a_missing_editor_says_so_and_names_where_it_came_from() {
    let cmd = parse_template(
        "cdno-no-such-editor-binary",
        EditorSource::EnvCuaderno,
        None,
    )
    .unwrap();

    let err = cmd.spawn(Path::new("/tmp/whatever.md")).unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("not found on PATH"), "got: {msg}");
    assert!(msg.contains("$CUADERNO_EDITOR"), "got: {msg}");
}

/// Percent-encoding is the detail this gets wrong if nobody looks: a vault
/// under `~/Google Drive/` or a filename containing `&` would otherwise
/// truncate the query parameter and open the wrong thing, or nothing.
#[test]
fn a_uri_percent_encodes_the_path() {
    let cmd = parse_template(
        "obsidian://open?path={path}",
        EditorSource::EnvCuaderno,
        None,
    )
    .unwrap();

    let uri = cmd
        .uri_for(Path::new("/Users/me/Google Drive/vault/a&b.md"))
        .unwrap();

    assert_eq!(
        uri,
        "obsidian://open?path=%2FUsers%2Fme%2FGoogle%20Drive%2Fvault%2Fa%26b.md"
    );
    // The separators that would end the parameter must not survive raw.
    assert!(
        !uri.trim_start_matches("obsidian://open?path=")
            .contains('&')
    );
}

/// A URI naming no note would open "nothing in particular" while reporting
/// success. The argv branch can append a missing placeholder; a URI cannot.
#[test]
fn a_uri_without_the_placeholder_is_refused() {
    let err = parse_template("obsidian://open", EditorSource::EnvCuaderno, None).unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("no `{path}` placeholder"), "got: {msg}");
    assert!(msg.contains("$CUADERNO_EDITOR"), "got: {msg}");
}

/// An editor killed by a signal has no exit code, and reporting that as
/// "handed off successfully" would tell a wrapper script the edit went fine.
#[test]
fn an_editor_killed_by_a_signal_does_not_report_success() {
    // A script that kills itself with SIGTERM, so `status.code()` is None.
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("suicide.sh");
    std::fs::write(&script, "#!/bin/sh\nkill -TERM $$\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let cmd = parse_template(&script.to_string_lossy(), EditorSource::Flag, None).unwrap();
    let code = cmd.spawn(Path::new("/tmp/whatever.md")).unwrap();

    assert_ne!(code, Some(0), "a signal death must not read as success");
    assert!(code.is_some(), "a waited-for editor must report some code");
}
