use cdno_core::error::{ParseError, ValidationError};
use cdno_core::frontmatter::Frontmatter;

#[test]
fn parses_minimal_frontmatter_with_body() {
    let raw = "---\ntitle: Hello\n---\nbody text\n";
    let (fm, body) = Frontmatter::parse(raw).unwrap();
    assert_eq!(fm.require_field::<String>("title").unwrap(), "Hello");
    assert_eq!(body, "body text\n");
}

#[test]
fn parses_frontmatter_with_empty_body() {
    let raw = "---\ntitle: Hello\n---\n";
    let (_fm, body) = Frontmatter::parse(raw).unwrap();
    assert_eq!(body, "");
}

#[test]
fn parses_frontmatter_with_no_trailing_newline_after_closing_delim() {
    let raw = "---\ntitle: Hello\n---";
    let (_fm, body) = Frontmatter::parse(raw).unwrap();
    assert_eq!(body, "");
}

#[test]
fn parses_multiple_fields() {
    let raw = "---\ntitle: Research\ncount: 42\npublished: true\n---\n";
    let (fm, _body) = Frontmatter::parse(raw).unwrap();
    assert_eq!(fm.require_field::<String>("title").unwrap(), "Research");
    assert_eq!(fm.require_field::<i64>("count").unwrap(), 42);
    assert!(fm.require_field::<bool>("published").unwrap());
}

#[test]
fn parses_nested_structures() {
    let raw = "---\ntags:\n  - rust\n  - notes\n---\n";
    let (fm, _body) = Frontmatter::parse(raw).unwrap();
    let tags: Vec<String> = fm.require_field("tags").unwrap();
    assert_eq!(tags, vec!["rust", "notes"]);
}

#[test]
fn accepts_crlf_line_endings() {
    let raw = "---\r\ntitle: Hello\r\n---\r\nbody\r\n";
    let (fm, body) = Frontmatter::parse(raw).unwrap();
    assert_eq!(fm.require_field::<String>("title").unwrap(), "Hello");
    assert_eq!(body, "body\r\n");
}

#[test]
fn empty_string_is_missing_frontmatter() {
    let err = Frontmatter::parse("").unwrap_err();
    assert!(matches!(err, ParseError::MissingFrontmatter(_)));
}

#[test]
fn document_without_delimiters_is_missing_frontmatter() {
    let err = Frontmatter::parse("just body\nno delimiters\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingFrontmatter(_)));
}

#[test]
fn leading_whitespace_before_opening_delim_is_missing_frontmatter() {
    let err = Frontmatter::parse("\n---\ntitle: x\n---\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingFrontmatter(_)));
}

#[test]
fn unclosed_frontmatter_is_invalid() {
    let err = Frontmatter::parse("---\ntitle: Hello\nbody with no close\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidFrontmatter(_)));
}

#[test]
fn malformed_yaml_is_invalid() {
    let err = Frontmatter::parse("---\nkey: : bad\n---\n").unwrap_err();
    assert!(matches!(
        err,
        ParseError::Yaml(_) | ParseError::InvalidFrontmatter(_)
    ));
}

#[test]
fn non_mapping_yaml_is_invalid() {
    let err = Frontmatter::parse("---\n- just\n- a\n- list\n---\n").unwrap_err();
    assert!(matches!(err, ParseError::InvalidFrontmatter(_)));
}

#[test]
fn require_missing_field_returns_missing_field_error() {
    let raw = "---\ntitle: Hello\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    let err = fm.require_field::<String>("author").unwrap_err();
    assert!(matches!(err, ValidationError::MissingField { ref field } if field == "author"));
}

#[test]
fn require_wrong_type_returns_invalid_field_error() {
    let raw = "---\ntitle: Hello\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    let err = fm.require_field::<i64>("title").unwrap_err();
    assert!(matches!(err, ValidationError::InvalidField { ref field, .. } if field == "title"));
}

#[test]
fn optional_missing_field_returns_none() {
    let raw = "---\ntitle: Hello\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    assert!(fm.optional_field::<String>("author").unwrap().is_none());
}

#[test]
fn optional_present_field_returns_some() {
    let raw = "---\ntitle: Hello\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    assert_eq!(
        fm.optional_field::<String>("title").unwrap().as_deref(),
        Some("Hello")
    );
}

#[test]
fn optional_wrong_type_errors_rather_than_none() {
    // Silent None on type mismatch would hide bugs.
    let raw = "---\ntitle: Hello\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    let err = fm.optional_field::<i64>("title").unwrap_err();
    assert!(matches!(err, ValidationError::InvalidField { .. }));
}

#[test]
fn optional_explicit_null_returns_none() {
    // YAML `field: null` is the canonical way to write "this
    // optional field has no value". `optional_field` collapses it
    // into `None`, matching `lint`'s "absent or null counts as
    // missing" interpretation and letting writers emit `null`
    // placeholders without tripping a non-existent type mismatch.
    let raw = "---\ntitle: ~\n---\n";
    let (fm, _) = Frontmatter::parse(raw).unwrap();
    assert!(fm.optional_field::<String>("title").unwrap().is_none());
}

#[test]
fn empty_frontmatter_block_parses_as_empty() {
    let raw = "---\n---\nbody\n";
    let (fm, body) = Frontmatter::parse(raw).unwrap();
    assert!(fm.optional_field::<String>("anything").unwrap().is_none());
    assert_eq!(body, "body\n");
}
