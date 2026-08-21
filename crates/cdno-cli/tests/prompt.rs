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

#[test]
fn prompting_needs_a_terminal_on_both_ends() {
    use cdno_cli::prompt::is_interactive_from;
    // The regression that shipped: only stdout was checked, so a caller
    // with a terminal on stdout and not stdin reached the prompt and
    // exited 1. A subprocess test cannot arrange that combination — it
    // gets neither — so the decision is asserted directly.
    assert!(is_interactive_from(false, true, true), "both terminals");
    assert!(
        !is_interactive_from(false, false, true),
        "stdout a terminal but stdin not: the shipped regression"
    );
    assert!(
        !is_interactive_from(false, true, false),
        "stdout not a terminal"
    );
    assert!(!is_interactive_from(false, false, false), "neither");
    assert!(
        !is_interactive_from(true, true, true),
        "--no-interactive outranks both"
    );
}

#[test]
fn json_output_is_never_followed_by_a_prompt() {
    use cdno_cli::prompt::reports_interactively;
    // Every test in the crate runs off a tty, so the `|| json` term
    // changes no outcome under test and dropping it at any of the ten
    // call sites passed the whole suite. Asserting the composition is
    // the only way to pin it without a terminal.
    use cdno_cli::prompt::reports_interactively_from;
    // The terminal terms are supplied so the `json` term is the only
    // thing deciding the outcome — off a tty the whole expression is
    // false anyway, and the assertion would hold with `json` deleted.
    assert!(
        reports_interactively_from(false, false, true, true),
        "a terminal with no --json is where a report may prompt"
    );
    assert!(
        !reports_interactively_from(false, true, true, true),
        "--json must never prompt, even with terminals on both ends"
    );
    assert!(!reports_interactively_from(true, false, true, true));
    assert!(!reports_interactively_from(true, true, true, true));
    // And the wired version still refuses off a tty.
    assert!(!reports_interactively(false, true));
}

#[test]
fn a_terminal_too_narrow_for_a_picker_gets_none() {
    use cdno_cli::prompt::picker_fits;
    // inquire lays its frame out against the real terminal width and
    // underflows below a couple of columns.
    assert!(!picker_fits(Some(0)));
    assert!(!picker_fits(Some(1)));
    assert!(!picker_fits(Some(19)));
    assert!(picker_fits(Some(20)), "exactly the floor");
    assert!(picker_fits(Some(120)));
    assert!(
        picker_fits(None),
        "not a terminal: the question does not arise"
    );
}
