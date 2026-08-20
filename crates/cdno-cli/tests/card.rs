//! Tests for the gutter-bar card renderer.
//!
//! Two of these carry most of the weight. The layout golden is a literal
//! block of expected output — the only kind of assertion that actually
//! notices when the look regresses. And
//! `colour_moves_no_character_of_the_layout` proves the module's central
//! discipline: wrap plain, paint last. If stripping the escapes from a
//! painted render reproduces the plain render exactly, then colour
//! cannot have shifted a single column.

mod strip;

use cdno_cli::output::card::{Card, render_cards};
use cdno_cli::output::style::{Accent, Palette, Role};
use textwrap::core::display_width;

use strip::strip_sgr;

fn fixtures() -> Vec<Card> {
    vec![
        Card::new("surrogate")
            .badge("side-project")
            .accent(Accent::Cyan)
            .prose("Six contributors settled; scope fixed to the solver rather than the mesher."),
        Card::new("mesh")
            .badge("work")
            .accent(Accent::Blue)
            .prose("Coarse-mesh run validated end to end.")
            .meta("next: profile the assembly step"),
    ]
}

#[test]
fn an_empty_set_renders_as_the_empty_string() {
    // Mirrors the contract `output::render` already has for an empty
    // table, so a caller can print the result unconditionally.
    assert_eq!(render_cards(&[], &Palette::plain(), 60), "");
}

#[test]
fn the_layout_is_what_we_think_it_is() {
    let out = render_cards(&fixtures(), &Palette::plain(), 60);
    let expected = "\
▎ surrogate    side-project
▎ Six contributors settled; scope fixed to the solver rather
▎ than the mesher.

▎ mesh         work
▎ Coarse-mesh run validated end to end.
▎ next: profile the assembly step
";
    assert_eq!(out, expected, "\n--- actual ---\n{out}");
}

#[test]
fn badges_align_into_one_column_across_the_set() {
    let cards = vec![
        Card::new("nfm").badge("work"),
        Card::new("a-considerably-longer-slug").badge("family"),
    ];
    let out = render_cards(&cards, &Palette::plain(), 100);
    let badge_columns: Vec<usize> = out
        .lines()
        .filter_map(|line| {
            let byte = line.find("work").or_else(|| line.find("family"))?;
            // Count columns, not bytes — the gutter glyph is 3 bytes.
            Some(line[..byte].chars().count())
        })
        .collect();
    assert_eq!(
        badge_columns.len(),
        2,
        "both headers should be found:\n{out}"
    );
    assert_eq!(
        badge_columns[0], badge_columns[1],
        "badges must share a column:\n{out}"
    );
}

#[test]
fn every_line_a_card_owns_carries_the_gutter() {
    let out = render_cards(&fixtures(), &Palette::plain(), 40);
    for line in out.lines().filter(|l| !l.is_empty()) {
        assert!(
            line.starts_with('▎'),
            "every card line needs the gutter: {line:?}"
        );
    }
}

#[test]
fn cards_are_separated_by_a_blank_line_with_none_trailing() {
    let out = render_cards(&fixtures(), &Palette::plain(), 60);
    assert_eq!(
        out.lines().filter(|l| l.is_empty()).count(),
        1,
        "exactly one separator between two cards:\n{out}"
    );
    assert!(out.ends_with('\n') && !out.ends_with("\n\n"));
}

#[test]
fn no_line_carries_trailing_whitespace() {
    // comfy-table's padding bug in card form: a gutter followed by an
    // empty body line must not leave `"▎ "` behind.
    let cards = vec![Card::new("slug").badge("work").prose("first\n\nthird")];
    let out = render_cards(&cards, &Palette::plain(), 60);
    assert!(
        !out.lines().any(|line| line.ends_with(' ')),
        "no trailing pad: {out:?}"
    );
    assert!(
        out.lines().any(|line| line == "▎"),
        "a blank body line keeps a bare gutter:\n{out}"
    );
}

#[test]
fn body_text_wraps_within_the_available_width() {
    let long = "one two three four five six seven eight nine ten eleven twelve";
    let cards = vec![Card::new("s").prose(long)];
    let out = render_cards(&cards, &Palette::plain(), 30);
    for line in out.lines() {
        // Display width, not `chars().count()` — the renderer budgets in
        // columns, and counting chars would let a CJK card overflow the
        // terminal by 2x while this assertion still passed.
        assert!(
            display_width(line) <= 30,
            "line is {} columns, budget 30: {line:?}",
            display_width(line)
        );
    }
    assert!(
        out.lines().count() > 2,
        "the body should have wrapped:\n{out}"
    );
}

#[test]
fn text_without_ascii_spaces_still_wraps() {
    // Chinese, Japanese and Thai have no ASCII spaces, so a wrapper with
    // only `WordSeparator::AsciiSpace` sees one unbreakable word and
    // never wraps at all — the paragraph runs off the terminal at any
    // width. That is what happens without textwrap's `unicode-linebreak`
    // feature, and it is why the feature is enabled.
    let cjk = "中文测试文字".repeat(20);
    let cards = vec![Card::new("cjk").prose(cjk)];
    let out = render_cards(&cards, &Palette::plain(), 40);
    for line in out.lines() {
        assert!(
            display_width(line) <= 40,
            "{} columns, budget 40: {line:?}",
            display_width(line)
        );
    }
    assert!(
        out.lines().count() > 4,
        "120 wide characters must wrap at width 40:\n{out}"
    );
}

#[test]
fn a_wide_title_still_aligns_the_badge_column() {
    // Catches measuring the title with `chars().count()`: CJK titles are
    // one char but two columns each, so a char-counted badge column would
    // sit half as far right as it should and badges would not line up.
    let cards = vec![
        Card::new("中文项目").badge("work"),
        Card::new("ascii-slug").badge("family"),
    ];
    let out = render_cards(&cards, &Palette::plain(), 80);
    let columns: Vec<usize> = out
        .lines()
        .filter_map(|line| {
            let at = line.find("work").or_else(|| line.find("family"))?;
            Some(display_width(&line[..at]))
        })
        .collect();
    assert_eq!(columns.len(), 2, "both headers found:\n{out}");
    assert_eq!(columns[0], columns[1], "badges share a column:\n{out}");
}

#[test]
fn an_over_narrow_terminal_falls_back_to_the_floor() {
    // Exercises the MIN_BODY_WIDTH floor, which no other test reaches
    // because they all use widths comfortably above it.
    let cards = vec![Card::new("s").prose("alpha beta gamma delta epsilon zeta")];
    let narrow = render_cards(&cards, &Palette::plain(), 4);
    let floored = render_cards(&cards, &Palette::plain(), 22);
    assert_eq!(
        narrow, floored,
        "below the floor the layout should match the floor, not degenerate"
    );
}

#[test]
fn control_characters_in_note_content_cannot_drive_the_terminal() {
    // Vault content is data, not instructions. A tab measures zero
    // columns but renders as up to eight; a carriage return walks the
    // cursor back over the gutter; an escape lets a note set colours,
    // move the cursor, or clear the screen.
    let hostile = "before\ttab\rcarriage\u{1b}[31mescape\u{7}bell";
    let cards = vec![Card::new("s").prose(hostile)];
    let out = render_cards(&cards, &Palette::plain(), 80);
    assert!(!out.contains('\t'), "tab survived: {out:?}");
    assert!(!out.contains('\r'), "carriage return survived: {out:?}");
    assert!(!out.contains('\u{1b}'), "escape survived: {out:?}");
    assert!(!out.contains('\u{7}'), "bell survived: {out:?}");
    // The visible words are still there — this sanitises, it does not censor.
    for word in ["before", "tab", "carriage", "escape", "bell"] {
        assert!(out.contains(word), "{word} was dropped: {out}");
    }
}

#[test]
fn a_blank_line_in_a_styled_body_block_keeps_a_bare_gutter() {
    // `paint(Meta, "")` is a pair of escapes, not an empty string, so
    // deciding emptiness on the painted value would put a stray space
    // after the gutter on every blank line in a styled block.
    let cards = vec![Card::new("s").block(Role::Meta, "first\n\nthird")];
    let plain = render_cards(&cards, &Palette::plain(), 60);
    let painted = render_cards(&cards, &Palette::forced(), 60);
    assert_eq!(strip_sgr(&painted), plain, "colour changed the layout");
    assert!(
        !painted.lines().any(|l| strip_sgr(l).ends_with(' ')),
        "{painted:?}"
    );
}

#[test]
fn accented_text_is_measured_in_columns_not_bytes() {
    // "años" is 4 columns but 5 bytes; measuring bytes would wrap a
    // column early on every Spanish word in the vault.
    let cards = vec![Card::new("s").prose("años años años años años años años")];
    let out = render_cards(&cards, &Palette::plain(), 24);
    for line in out.lines() {
        let visible = line.chars().count();
        assert!(visible <= 24, "{line:?} is {visible} columns");
    }
    assert!(
        out.lines().any(|l| l.matches("años").count() >= 4),
        "22 columns should fit four 4-column words:\n{out}"
    );
}

#[test]
fn an_over_long_token_overflows_rather_than_breaking() {
    // Same principle as `no_wrap_columns`: an identifier split across
    // lines is worse than one that runs past the edge.
    let slug = "a-very-long-identifier-that-exceeds-the-whole-width-on-its-own";
    let cards = vec![Card::new("s").prose(slug)];
    let out = render_cards(&cards, &Palette::plain(), 30);
    assert!(
        out.lines().any(|line| line.contains(slug)),
        "the token must stay whole:\n{out}"
    );
}

#[test]
fn source_newlines_are_hard_breaks() {
    let cards = vec![Card::new("s").prose("first paragraph\nsecond paragraph")];
    let out = render_cards(&cards, &Palette::plain(), 80);
    assert!(
        out.contains("▎ first paragraph\n▎ second paragraph"),
        "{out}"
    );
}

#[test]
fn a_card_without_a_badge_renders_a_bare_title() {
    let cards = vec![Card::new("solo").prose("body")];
    let out = render_cards(&cards, &Palette::plain(), 60);
    assert_eq!(out, "▎ solo\n▎ body\n");
}

#[test]
fn colour_moves_no_character_of_the_layout() {
    // The module's central claim, as an assertion.
    for width in [24, 40, 60, 100] {
        let plain = render_cards(&fixtures(), &Palette::plain(), width);
        let painted = render_cards(&fixtures(), &Palette::forced(), width);
        assert_ne!(plain, painted, "the forced palette should paint something");
        assert_eq!(
            strip_sgr(&painted),
            plain,
            "colour perturbed the layout at width {width}"
        );
    }
}

#[test]
fn a_painted_card_paints_the_gutter_itself() {
    let cards = vec![Card::new("s").accent(Accent::Blue).block(Role::Meta, "m")];
    let painted = render_cards(&cards, &Palette::forced(), 60);
    assert!(
        painted.starts_with('\u{1b}'),
        "the gutter is the accent's job: {painted:?}"
    );
}
