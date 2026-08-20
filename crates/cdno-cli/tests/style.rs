//! Tests for the CLI colour palette and its gate.
//!
//! The contract worth pinning is not "cyan is cyan" — it is that a
//! disabled palette is *byte-identical* to no palette at all. The whole
//! existing test surface asserts on plain substrings while running off a
//! tty, and it stays green only because `paint` is a true no-op rather
//! than a visual one.
//!
//! Note what is deliberately absent: nothing here touches the process
//! gate (`init` / `colour_enabled`). It is a write-once global, so a
//! test that set it would leak into every other test in this binary.
//! `Palette::{plain, forced}` exist precisely so the styled path is
//! testable without it.

mod strip;

use cdno_cli::output::style::{Accent, ColourChoice, Palette, Role};

use strip::strip_sgr;

const EVERY_ROLE: [Role; 9] = [
    Role::Slug,
    Role::Badge,
    Role::Heading,
    Role::Prose,
    Role::Meta,
    Role::Muted,
    Role::Warn,
    Role::Error,
    Role::Success,
];

#[test]
fn a_disabled_palette_returns_its_input_untouched() {
    let palette = Palette::plain();
    for role in EVERY_ROLE {
        let text = "projects/surrogate-model.md";
        assert_eq!(
            palette.paint(role, text),
            text,
            "{role:?} must not alter the text when colour is off"
        );
    }
}

#[test]
fn a_disabled_palette_leaves_the_gutter_accent_untouched() {
    let palette = Palette::plain();
    for accent in [Accent::Blue, Accent::Cyan, Accent::Grey, Accent::Red] {
        assert_eq!(palette.paint_accent(accent, "▎"), "▎");
    }
}

#[test]
fn a_forced_palette_paints_and_always_resets() {
    let palette = Palette::forced();
    // Prose is the deliberate exception: it is the baseline every other
    // role is read against, so it carries no style of its own.
    for role in EVERY_ROLE.into_iter().filter(|r| *r != Role::Prose) {
        let painted = palette.paint(role, "cuaderno");
        assert!(
            painted.starts_with('\u{1b}'),
            "{role:?} should open with an escape: {painted:?}"
        );
        assert!(
            painted.ends_with("\u{1b}[0m"),
            "{role:?} should close with a reset: {painted:?}"
        );
        assert!(
            painted.contains("cuaderno"),
            "{role:?} must not mangle the text: {painted:?}"
        );
    }
}

#[test]
fn prose_is_unstyled_even_when_forced() {
    assert_eq!(
        Palette::forced().paint(Role::Prose, "body text"),
        "body text"
    );
}

#[test]
fn painting_changes_no_visible_character() {
    // The property the card renderer depends on: styling adds escape
    // bytes and nothing else, so layout computed on plain text stays
    // correct once the text is painted.
    let palette = Palette::forced();
    for role in EVERY_ROLE {
        let text = "surrogate — mesh scaling";
        assert_eq!(strip_sgr(&palette.paint(role, text)), text);
    }
}

#[test]
fn every_context_gets_a_gutter_colour() {
    use cdno_domain::frontmatter::Context;
    let palette = Palette::forced();
    for context in Context::ALL {
        let accent = Accent::for_context(context);
        let painted = palette.paint_accent(accent, "▎");
        assert_eq!(strip_sgr(&painted), "▎", "{context:?}");
        assert!(
            painted.starts_with('\u{1b}'),
            "{context:?} should be distinguishable by colour: {painted:?}"
        );
    }
}

#[test]
fn the_flag_maps_onto_the_shared_colour_choice() {
    // The flag exists to feed `colorchoice`; if the mapping drifts,
    // `--color never` would silently mean something else.
    assert_eq!(
        colorchoice::ColorChoice::from(ColourChoice::Never),
        colorchoice::ColorChoice::Never
    );
    assert_eq!(
        colorchoice::ColorChoice::from(ColourChoice::Always),
        colorchoice::ColorChoice::Always
    );
    assert_eq!(
        colorchoice::ColorChoice::from(ColourChoice::Auto),
        colorchoice::ColorChoice::Auto
    );
    assert_eq!(ColourChoice::default(), ColourChoice::Auto);
}
