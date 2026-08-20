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

use cdno_cli::output::style::{Accent, ColourChoice, Palette, Role, cell};

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
fn contexts_are_actually_distinguishable_by_gutter_colour() {
    use cdno_domain::frontmatter::Context;
    let palette = Palette::forced();
    let painted: Vec<String> = Context::ALL
        .iter()
        .map(|c| palette.paint_accent(Accent::for_context(*c), "▎"))
        .collect();

    for (context, bar) in Context::ALL.iter().zip(&painted) {
        assert_eq!(strip_sgr(bar), "▎", "{context:?} must not alter the glyph");
    }
    // The point of the gutter is that a mixed list separates before a
    // word is read, so the colours must differ from each other. Asserting
    // only "starts with an escape" would pass with every context mapped
    // to one hue — which is exactly what the previous version of this
    // test allowed.
    let distinct: std::collections::BTreeSet<&String> = painted.iter().collect();
    assert_eq!(
        distinct.len(),
        Context::ALL.len(),
        "every context needs its own colour, got:\n{painted:#?}"
    );
}

#[test]
fn question_domains_are_distinguishable_from_each_other() {
    use cdno_domain::frontmatter::QuestionDomain;
    let palette = Palette::forced();
    let research = palette.paint_accent(Accent::for_question(QuestionDomain::Research), "▎");
    let life = palette.paint_accent(Accent::for_question(QuestionDomain::Life), "▎");
    assert_ne!(
        research, life,
        "research and life questions sit in one listing and must differ"
    );
}

#[test]
fn staleness_reads_off_the_gutter() {
    // One mapping for both portfolios and stewardships — they used to
    // pick their own hues for the same idea, so a healthy portfolio was
    // cyan and a healthy stewardship green for no inferable reason.
    assert_eq!(
        Accent::for_staleness(0, None),
        Accent::Grey,
        "nothing filed"
    );
    assert_eq!(
        Accent::for_staleness(0, Some(99)),
        Accent::Grey,
        "nothing filed"
    );
    assert_eq!(
        Accent::for_staleness(3, Some(2)),
        Accent::Green,
        "fed recently"
    );
    assert_eq!(
        Accent::for_staleness(3, None),
        Accent::Green,
        "fed, undated"
    );
    assert_eq!(
        Accent::for_staleness(3, Some(31)),
        Accent::Yellow,
        "gone stale"
    );
    // The boundary is the interesting part.
    assert_eq!(
        Accent::for_staleness(3, Some(30)),
        Accent::Green,
        "exactly 30 days"
    );
}

/// The SGR parameters a rendered string carries, canonicalised so the
/// two encodings of a base-16 colour compare equal: anstyle writes
/// `ESC[36m` while comfy-table (via crossterm) writes `ESC[38;5;6m`, and
/// both mean colour index 6.
fn sgr_facts(text: &str) -> (Option<u8>, bool, bool) {
    let (mut colour, mut bold, mut dim) = (None, false, false);
    for chunk in text.split('\u{1b}').skip(1) {
        let Some(params) = chunk.strip_prefix('[').and_then(|c| c.split('m').next()) else {
            continue;
        };
        let parts: Vec<&str> = params.split(';').collect();
        match parts.as_slice() {
            ["1"] => bold = true,
            ["2"] => dim = true,
            // 38;5;N — the 256-colour form comfy-table emits.
            ["38", "5", n] => colour = n.parse().ok(),
            // 30-37 normal, 90-97 bright — the form anstyle emits.
            [n] => {
                if let Ok(v) = n.parse::<u8>() {
                    if (30..=37).contains(&v) {
                        colour = Some(v - 30);
                    } else if (90..=97).contains(&v) {
                        colour = Some(v - 90 + 8);
                    }
                }
            }
            _ => {}
        }
    }
    (colour, bold, dim)
}

/// Render one styled cell the way `styled_table` would with colour on.
fn rendered_cell(role: Role) -> String {
    let mut table = comfy_table::Table::new();
    table.load_preset(comfy_table::presets::NOTHING);
    table.enforce_styling();
    table.style_text_only();
    table.add_row(vec![cell(role, "x")]);
    table.to_string()
}

#[test]
fn a_table_cell_carries_the_same_style_as_the_card_beside_it() {
    // `Role::Slug` used to come out as colour index 6 through anstyle and
    // index 14 through comfy-table, because crossterm reserves the plain
    // colour names for the *bright* range while anstyle uses them for the
    // normal one — so the same slug was two different cyans depending on
    // which renderer drew it. The table path now derives from the card
    // path; this pins that they agree, for every role.
    let palette = Palette::forced();
    for role in EVERY_ROLE {
        let card = sgr_facts(&palette.paint(role, "x"));
        let table = sgr_facts(&rendered_cell(role));
        assert_eq!(
            card, table,
            "{role:?}: card renders {card:?} but the table renders {table:?}"
        );
    }
}

#[test]
fn no_role_brightens_on_the_table_path() {
    // The specific bug class, stated as its own assertion: indices 8-15
    // are the bright range, and a name-to-name mapping would land there.
    for role in EVERY_ROLE {
        if let (Some(index), _, _) = sgr_facts(&rendered_cell(role)) {
            assert!(
                index < 8,
                "{role:?} resolved to bright colour index {index}; the palette uses 0-7"
            );
        }
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
