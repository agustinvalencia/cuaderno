//! Tests for the shared CLI table-formatting helper (#153). These run
//! off a tty, so `styled_table()` pins the deterministic fallback width.

use cdno_cli::output::{no_wrap_columns, render, styled_table};

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
