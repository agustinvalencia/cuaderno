//! Tests for the shared CLI table-formatting helper (#153). These run
//! off a tty, so `styled_table()` pins the deterministic fallback width.

use cdno_cli::output::{NON_TTY_WIDTH, credible_width, no_wrap_columns, render, styled_table};

#[test]
fn render_strips_trailing_whitespace_from_every_line() {
    let mut table = styled_table();
    table.add_row(vec!["slug", "a short description"]);
    table.add_row(vec!["other", "another one"]);
    let out = render(&table);
    assert!(
        !out.lines().any(|line| line.ends_with(' ')),
        "no rendered line should carry comfy-table's trailing cell pad:\n{out:?}"
    );
}

#[test]
fn render_of_an_empty_table_is_the_empty_string() {
    let table = styled_table();
    assert_eq!(render(&table), "");
}

#[test]
fn no_wrap_columns_keeps_a_long_identifier_whole() {
    // A long identifier next to a long free-text column: under plain
    // Dynamic arrangement comfy-table would wrap the identifier to
    // balance widths. Pinning column 0 must force the free-text column
    // to absorb all the reflow instead, leaving the slug intact.
    let slug = "a-very-long-identifier-slug-that-would-otherwise-wrap-under-dynamic-arrangement";
    let mut table = styled_table();
    table.add_row(vec![
        slug.to_owned(),
        "and a long free-text description that should absorb the wrapping rather than \
         letting the identifier column reflow across multiple rows"
            .to_owned(),
    ]);
    no_wrap_columns(&mut table, &[0]);
    let out = render(&table);
    assert!(
        out.lines().any(|line| line.contains(slug)),
        "the pinned identifier must stay whole on one line:\n{out}"
    );
}

#[test]
fn an_unbelievable_terminal_width_falls_back_rather_than_shredding_the_table() {
    // A pty opened with no window size reports zero columns. Handing that
    // straight to comfy-table is not a near-miss — `set_width(0)` wraps
    // every cell to one character per line, so a three-row table becomes
    // a hundred-line vertical column of letters.
    //
    // This exercises the guard directly rather than through
    // `render_width`: the suite runs off a tty, so `render_width` returns
    // the fallback before it ever asks the terminal, and asserting on it
    // would only re-confirm `NON_TTY_WIDTH >= 20`.
    assert_eq!(credible_width(Some(0)), NON_TTY_WIDTH, "a zero-column pty");
    assert_eq!(credible_width(Some(1)), NON_TTY_WIDTH, "one column");
    assert_eq!(
        credible_width(Some(19)),
        NON_TTY_WIDTH,
        "just under the floor"
    );
    assert_eq!(credible_width(None), NON_TTY_WIDTH, "no answer at all");
    // At and above the floor the terminal is believed.
    assert_eq!(credible_width(Some(20)), 20, "exactly the floor");
    assert_eq!(credible_width(Some(120)), 120, "an ordinary terminal");
}

#[test]
fn a_short_row_stays_on_one_line_at_the_fallback_width() {
    let mut table = styled_table();
    table.add_row(vec!["alpha", "next: define the first concrete step"]);
    let out = render(&table);
    assert_eq!(
        out.lines().count(),
        1,
        "a short row must not wrap at the fallback width:\n{out}"
    );
}
