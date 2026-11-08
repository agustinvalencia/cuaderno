use cdno_core::error::ParseError as PE;
use cdno_core::error::{ParseError, ValidationError};
use cdno_core::frontmatter::{
    Frontmatter, parse_note, parse_note_spanned, split_frontmatter, yaml_scalar_is_string,
};
use cdno_core::note::note_headings;
use cdno_core::path::VaultPath;
use cdno_core::typst::TypstError;

fn vp(s: &str) -> VaultPath {
    VaultPath::new(s).unwrap()
}

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

#[test]
fn split_frontmatter_returns_the_raw_block_and_the_untouched_body() {
    let raw = "---\ntype: concept\norigin: [[a]] [[b]]\n---\n\n# T\n";
    // The block is returned unparsed, so YAML that would not parse still splits.
    assert_eq!(
        split_frontmatter(raw),
        Some(("type: concept\norigin: [[a]] [[b]]\n", "\n# T\n"))
    );
    assert_eq!(split_frontmatter("---\n---\nbody"), Some(("", "body")));
    assert_eq!(split_frontmatter("# T\n"), None);
    assert_eq!(split_frontmatter("---\nunclosed: 1\n"), None);
}

#[test]
fn yaml_scalar_is_string_asks_the_parser() {
    for typed in ["true", "0o17", "1e-3", "null", "~", "2026", "", "[a"] {
        assert!(!yaml_scalar_is_string(typed), "{typed:?}");
    }
    for text in ["y", "n", "surrogate-model", "2026-11", "v2"] {
        assert!(yaml_scalar_is_string(text), "{text:?}");
    }
}

#[test]
fn parse_note_on_markdown_is_frontmatter_parse() {
    let fixtures = [
        "---\ntitle: Hello\n---\nbody text\n",
        "---\ntitle: Hello\n---\n",
        "---\ntitle: Hello\n---",
        "---\n---\nbody\n",
        "---\r\ntitle: Surrogate model\r\n---\r\nbody\r\n",
        "---\ntags:\n  - rust\n  - notes\n---\n# T\n",
        "---\ntype: evidence\norigin: \"[[surrogate-model]]\"\ncount: 42\n---\n\n# Bias-variance split\n",
        "no frontmatter at all\n",
        "---\nunclosed: 1\n",
        "---\n- a\n- b\n---\nbody\n",
        "---\n: [broken\n---\nbody\n",
    ];
    let path = vp("concepts/surrogate-model.md");
    for raw in fixtures {
        match (Frontmatter::parse(raw), parse_note(&path, raw)) {
            (Ok((a, abody)), Ok((b, bbody))) => {
                assert_eq!(a.as_json(), b.as_json(), "{raw:?}");
                assert_eq!(abody, bbody, "{raw:?}");
            }
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{raw:?}"),
            other => panic!("{raw:?}: the two disagree: {other:?}"),
        }
    }
}

#[test]
fn parse_note_on_typst_returns_the_block_and_the_rest() {
    let head = "#metadata((type: \"evidence\", title: \"Bias-variance split\", tags: (\"a\", \"b\"), n: 3, ok: true)) <cdno>";
    let rest = "\n// a comment on the next line\n= Bias-variance split\n";
    let raw = format!("{head}{rest}");
    let (fm, body) = parse_note(&vp("portfolios/surrogate-model/x.typ"), &raw).unwrap();
    assert_eq!(body, rest);
    assert_eq!(fm.require_field::<String>("type").unwrap(), "evidence");
    assert_eq!(
        fm.require_field::<String>("title").unwrap(),
        "Bias-variance split"
    );
    assert_eq!(fm.require_field::<Vec<String>>("tags").unwrap(), ["a", "b"]);
    assert_eq!(fm.require_field::<i64>("n").unwrap(), 3);
    assert!(fm.require_field::<bool>("ok").unwrap());

    let spanned = parse_note_spanned(&vp("portfolios/surrogate-model/x.typ"), &raw).unwrap();
    assert_eq!(spanned.metadata_span, 0..head.len());
    assert_eq!(spanned.body, rest);
}

#[test]
fn parse_note_spanned_on_markdown_spans_the_frontmatter_block() {
    let raw = "---\ntitle: Hello\n---\nbody\n";
    let s = parse_note_spanned(&vp("a.md"), raw).unwrap();
    assert_eq!(&raw[s.metadata_span], "---\ntitle: Hello\n---\n");
    assert_eq!(s.body, "body\n");
}

#[test]
fn parse_note_on_a_typst_file_without_a_block_is_no_block() {
    let err = parse_note(&vp("portfolios/p/x.typ"), "= Just a heading\n").unwrap_err();
    assert!(matches!(err, PE::Typst(TypstError::NoBlock)), "{err:?}");
}

#[test]
fn parse_note_on_a_pdf_path_is_an_error() {
    let err = parse_note(&vp("portfolios/p/x/figure.pdf"), "---\ntitle: x\n---\n").unwrap_err();
    assert!(matches!(err, PE::MissingFrontmatter(_)), "{err:?}");
    assert!(parse_note(&vp("README"), "---\n---\n").is_err());
}

#[test]
fn note_headings_dispatches_on_the_path() {
    assert_eq!(
        note_headings(&vp("a.md"), "# One\n## Two\n"),
        [(1, "One".to_string()), (2, "Two".to_string())]
    );
    assert_eq!(
        note_headings(&vp("a.typ"), "= One\n== Two\n"),
        [(1, "One".to_string()), (2, "Two".to_string())]
    );
    // A Markdown reading of the same Typst bytes finds nothing.
    assert!(note_headings(&vp("a.md"), "= One\n").is_empty());
}
