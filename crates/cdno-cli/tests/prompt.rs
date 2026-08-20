//! Unit tests for the `--var name=value` parser (#238).

use cdno_cli::prompt::parse_key_val;

#[test]
fn parses_a_simple_pair() {
    assert_eq!(
        parse_key_val("ticket=ABC-1").unwrap(),
        ("ticket".to_owned(), "ABC-1".to_owned())
    );
}

#[test]
fn splits_on_the_first_equals_only() {
    // Values may contain `=`; only the first separates name from value.
    assert_eq!(
        parse_key_val("expr=a=b").unwrap(),
        ("expr".to_owned(), "a=b".to_owned())
    );
}

#[test]
fn allows_an_empty_value() {
    assert_eq!(
        parse_key_val("note=").unwrap(),
        ("note".to_owned(), String::new())
    );
}

#[test]
fn rejects_a_missing_equals() {
    assert!(parse_key_val("ticket").is_err());
}

#[test]
fn rejects_an_empty_name() {
    assert!(parse_key_val("=value").is_err());
}

// ---------------------------------------------------------------------
// Drill-down.
//
// The interactive loop itself needs a pty to exercise, and pulling in a
// pty harness to assert that three lines match an inquire error variant
// is not proportionate — those lines mirror `triage.rs`, which has
// shipped. What *is* worth pinning is the guard, because it is the
// promise every scripted, piped, and `--json` caller relies on.
// ---------------------------------------------------------------------

use std::cell::Cell;

use cdno_cli::prompt::drill_down;

#[test]
fn a_non_interactive_caller_never_enters_the_loop() {
    // If this regressed, `cdno project list | head` and every agent
    // calling the CLI would hang waiting on a prompt nobody can answer.
    let shown = Cell::new(false);
    drill_down(
        &["alpha", "beta"],
        "Inspect",
        false,
        |s| (*s).to_owned(),
        |_| {
            shown.set(true);
            Ok(())
        },
    )
    .expect("a suppressed drill-down is not an error");
    assert!(!shown.get(), "nothing should have been shown");
}

#[test]
fn an_empty_report_does_not_prompt_even_interactively() {
    // Guarding on `interactive` alone would leave `Select` with no
    // options to offer; this test hanging is itself the signal.
    let shown = Cell::new(false);
    drill_down(
        &[] as &[&str],
        "Inspect",
        true,
        |s| (*s).to_owned(),
        |_| {
            shown.set(true);
            Ok(())
        },
    )
    .expect("an empty report is not an error");
    assert!(!shown.get());
}
