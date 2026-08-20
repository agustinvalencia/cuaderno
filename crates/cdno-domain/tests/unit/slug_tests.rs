//! Unit tests for the public [`slugify`] rules — the word cap, the char
//! cap, and where the char cap is allowed to land.
//!
//! The slug is the filename *and* the wikilink target, so what these
//! pin is permanent and visible everywhere a note is referenced: #524
//! was a cut landing inside a word, leaving `…-and-materia` on disk.

use cdno_domain::slugify;

#[test]
fn slugify_leaves_a_title_under_both_caps_alone() {
    // The ordinary path: six words or fewer, comfortably under the char
    // cap, so neither rule fires.
    assert_eq!(
        slugify("Surrogate model for turbulence"),
        "surrogate-model-for-turbulence"
    );
}

#[test]
fn slugify_ends_on_a_whole_word_when_the_char_cap_falls_mid_word() {
    // #524. Six words joined is 54 chars, so the cap cuts four chars into
    // `laminate` and used to leave the fragment `-lami` on the end. The
    // slug now backs off to the preceding boundary instead.
    assert_eq!(
        slugify("Characterising thermal expansion in composite laminate material"),
        "characterising-thermal-expansion-in-composite",
    );
}

#[test]
fn slugify_keeps_a_final_word_the_char_cap_left_room_for() {
    // The cut falls exactly on a separator: the last kept word is already
    // whole, so backing off further would discard a word that fits. Four
    // ten-char words plus a six-char one is exactly 50, with the dash at
    // index 50 the char being dropped.
    assert_eq!(
        slugify("aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeee ffff"),
        "aaaaaaaaaa-bbbbbbbbbb-cccccccccc-dddddddddd-eeeeee",
    );
}

#[test]
fn slugify_never_ends_in_a_stray_separator() {
    // The other half of the original intent, kept: here the cut lands on
    // the first char of a word, so the kept text ends in the separator
    // before it, and the slug must not end in a dash.
    assert_eq!(
        slugify("aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeee ffff"),
        "aaaaaaaaaa-bbbbbbbbbb-cccccccccc-dddddddddd-eeeee",
    );
}

#[test]
fn slugify_still_truncates_a_single_word_longer_than_the_char_cap() {
    // The case the cap exists for. There is no boundary to back off to,
    // so the hard truncation stands rather than collapsing the slug.
    assert_eq!(slugify(&"a".repeat(80)), "a".repeat(50));
}

#[test]
fn slugify_counts_chars_not_bytes_when_the_cap_bites() {
    // The cap is char-aware, and the boundary check that follows it reads
    // a byte at that char index -- so a multi-byte word is the input that
    // would expose a byte/char slip, by panicking on a non-boundary
    // truncate or by misreading the dropped char.
    assert_eq!(slugify(&"\u{fc}".repeat(80)), "\u{fc}".repeat(50));
}
