//! The slug shape rule, which `VaultIndex::list_aliases` shares with the
//! explicit-slug validator (RFC 0007 §5.7).

use cdno_core::slug::{SlugRejection, is_slug_shaped, validate_explicit_slug};

#[test]
fn explicit_slug_rejections_name_their_reason() {
    let long = "a".repeat(51);
    let rows: Vec<(&str, SlugRejection)> = vec![
        ("", SlugRejection::Empty),
        ("Surrogate", SlugRejection::UpperCase),
        ("a b", SlugRejection::BadChar(' ')),
        ("\u{e4}", SlugRejection::BadChar('\u{e4}')),
        ("-a", SlugRejection::LeadingOrTrailingDash),
        ("a-", SlugRejection::LeadingOrTrailingDash),
        ("a--b", SlugRejection::DoubleDash),
        (&long, SlugRejection::TooLong { max: 50 }),
        ("2026", SlugRejection::YamlTyped),
        ("true", SlugRejection::YamlTyped),
        ("no", SlugRejection::YamlTyped),
        ("1e3", SlugRejection::YamlTyped),
        ("2026-11-05", SlugRejection::YamlTyped),
        ("007", SlugRejection::YamlTyped),
        ("00", SlugRejection::YamlTyped),
        ("0o17", SlugRejection::YamlTyped),
        ("1e-3", SlugRejection::YamlTyped),
        ("12e-04", SlugRejection::YamlTyped),
    ];
    for (input, expected) in rows {
        let got = validate_explicit_slug(input).expect_err(input);
        assert_eq!(got, expected, "input {input:?}");
        assert!(!got.to_string().is_empty());
    }
    for ok in [
        "surrogate-model",
        "a",
        "y",
        "n",
        "2026-11",
        "v2",
        "a-b-c-d-e-f-g",
    ] {
        assert_eq!(validate_explicit_slug(ok), Ok(()), "input {ok:?}");
    }
}

#[test]
fn is_slug_shaped_accepts_what_slugify_produces() {
    for yes in ["no", "007", "2026", "surrogate-model", "2026-11-05"] {
        assert!(is_slug_shaped(yes), "input {yes:?}");
    }
    let long = "a".repeat(51);
    for no in ["", "A", "a--b", "-a", "a-", "a b", &long] {
        assert!(!is_slug_shaped(no), "input {no:?}");
    }
}

#[test]
fn is_slug_shaped_agrees_with_the_validator() {
    // The validator is the shape rule plus the YAML over-rejections, so
    // whatever it accepts is slug-shaped, and the two differ only on YamlTyped.
    let long = "a".repeat(51);
    for input in [
        "old-slug",
        "a",
        "v2",
        "2026-11",
        "",
        "A",
        "a--b",
        "-a",
        &long,
        "no",
        "007",
        "2026",
        "true",
        "2026-11-05",
        "A display name",
    ] {
        let validated = validate_explicit_slug(input);
        if validated.is_ok() {
            assert!(is_slug_shaped(input), "input {input:?}");
        }
        assert_eq!(
            is_slug_shaped(input),
            matches!(validated, Ok(()) | Err(SlugRejection::YamlTyped)),
            "input {input:?}"
        );
    }
}
