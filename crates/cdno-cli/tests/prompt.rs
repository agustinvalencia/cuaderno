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

use cdno_cli::prompt::{drive_for_test, leaves_quietly};
use inquire::InquireError;

fn labels(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("row-{i}")).collect()
}

#[test]
fn cancelling_leaves_without_an_error() {
    // Esc and Ctrl-C are the ordinary way out of a read-only report, so
    // they must exit 0. Making this arm propagate instead used to pass
    // the whole suite.
    for err in [
        InquireError::OperationCanceled,
        InquireError::OperationInterrupted,
        InquireError::NotTTY,
        InquireError::IO(std::io::Error::other("no terminal")),
    ] {
        assert!(
            leaves_quietly(&err),
            "{err:?} should end the session quietly"
        );
    }
    assert!(
        !leaves_quietly(&InquireError::InvalidConfiguration("broken".into())),
        "a genuine configuration fault is not a quiet exit"
    );
}

#[test]
fn the_loop_re_asks_after_showing_a_detail() {
    let items = ["alpha", "beta"];
    let seen = Cell::new(Vec::new());
    let mut answers = vec![Ok(1), Ok(0), Err(InquireError::OperationCanceled)].into_iter();
    drive_for_test(
        &items,
        &labels(2),
        &mut || answers.next().unwrap(),
        |item| {
            let mut v = seen.take();
            v.push(*item);
            seen.set(v);
            Ok(())
        },
    )
    .expect("cancelling ends the session cleanly");
    assert_eq!(
        seen.take(),
        vec!["beta", "alpha"],
        "each pick shows its own row, and the loop asks again"
    );
}

#[test]
fn a_row_that_cannot_be_shown_does_not_end_the_session() {
    // The tolerance `cdno triage` has for a failing item. Making this
    // propagate instead passed every test in the crate.
    let items = ["alpha", "beta"];
    let shown = Cell::new(0);
    let mut answers = vec![Ok(0), Ok(1), Err(InquireError::OperationCanceled)].into_iter();
    let result = drive_for_test(
        &items,
        &labels(2),
        &mut || answers.next().unwrap(),
        |item| {
            shown.set(shown.get() + 1);
            if *item == "alpha" {
                anyhow::bail!("cannot read alpha");
            }
            Ok(())
        },
    );
    assert!(result.is_ok(), "one bad row must not abort: {result:?}");
    assert_eq!(shown.get(), 2, "the second row was still offered");
}

#[test]
fn a_genuine_prompt_fault_propagates() {
    let items = ["alpha"];
    let mut answers = vec![Err(InquireError::InvalidConfiguration("bad".into()))].into_iter();
    let result = drive_for_test(&items, &labels(1), &mut || answers.next().unwrap(), |_| {
        Ok(())
    });
    assert!(result.is_err(), "a real fault should not be swallowed");
}
