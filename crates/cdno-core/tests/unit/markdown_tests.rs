use cdno_core::error::{ManipulationError, ParseError};
use cdno_core::markdown::MarkdownDocument;

const SIMPLE_DOC: &str = "\
---
title: Sample
---
# Intro
intro text

## Milestones
- [ ] first
- [ ] second

## Next Actions
- [ ] call Alice

# Separate Top
other top content
";

#[test]
fn parse_succeeds_on_document_with_frontmatter() {
    let doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    assert_eq!(
        doc.frontmatter().require_field::<String>("title").unwrap(),
        "Sample"
    );
}

#[test]
fn parse_errors_on_document_without_frontmatter() {
    let err = MarkdownDocument::parse("# heading\nbody\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingFrontmatter(_)));
}

#[test]
fn section_returns_content_under_heading() {
    let doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let milestones = doc.section("Milestones").unwrap();
    assert!(milestones.contains("- [ ] first"));
    assert!(milestones.contains("- [ ] second"));
    // Content does not leak into the next sibling section.
    assert!(!milestones.contains("- [ ] call Alice"));
}

#[test]
fn section_content_ends_at_next_same_or_higher_level_heading() {
    let doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let intro = doc.section("Intro").unwrap();
    // Intro (level 1) contains its own text and the H2 subsections.
    assert!(intro.contains("intro text"));
    assert!(intro.contains("## Milestones"));
    assert!(intro.contains("## Next Actions"));
    // But stops at the next level-1 heading.
    assert!(!intro.contains("# Separate Top"));
    assert!(!intro.contains("other top content"));
}

#[test]
fn section_missing_returns_not_found() {
    let doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let err = doc.section("Nonexistent").unwrap_err();
    assert!(matches!(err, ManipulationError::SectionNotFound(_)));
}

#[test]
fn section_ambiguous_returns_error() {
    let raw = "\
---
title: x
---
## Notes
first notes
## Notes
second notes
";
    let doc = MarkdownDocument::parse(raw).unwrap();
    let err = doc.section("Notes").unwrap_err();
    assert!(matches!(err, ManipulationError::AmbiguousSection(_)));
}

#[test]
fn heading_inside_code_block_is_not_matched() {
    let raw = "\
---
title: x
---
## Real Heading
real content

```
## Fake Heading
inside code block
```
";
    let doc = MarkdownDocument::parse(raw).unwrap();
    // The section inside the code block must not be found as a heading.
    let err = doc.section("Fake Heading").unwrap_err();
    assert!(matches!(err, ManipulationError::SectionNotFound(_)));
    // And the real heading's content must include the code block intact.
    let real = doc.section("Real Heading").unwrap();
    assert!(real.contains("## Fake Heading"));
    assert!(real.contains("inside code block"));
}

#[test]
fn replace_section_overwrites_content() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.replace_section("Milestones", "- [x] new single task\n")
        .unwrap();
    let updated = doc.section("Milestones").unwrap();
    assert!(updated.contains("- [x] new single task"));
    assert!(!updated.contains("- [ ] first"));
}

#[test]
fn replace_section_preserves_other_sections() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.replace_section("Milestones", "- [x] replaced\n")
        .unwrap();
    let next = doc.section("Next Actions").unwrap();
    assert!(next.contains("- [ ] call Alice"));
    let separate = doc.section("Separate Top").unwrap();
    assert!(separate.contains("other top content"));
}

#[test]
fn replace_section_missing_returns_not_found() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let err = doc.replace_section("Nonexistent", "x").unwrap_err();
    assert!(matches!(err, ManipulationError::SectionNotFound(_)));
}

#[test]
fn append_to_section_extends_content() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.append_to_section("Milestones", "- [ ] third\n")
        .unwrap();
    let updated = doc.section("Milestones").unwrap();
    assert!(updated.contains("- [ ] first"));
    assert!(updated.contains("- [ ] second"));
    assert!(updated.contains("- [ ] third"));
}

#[test]
fn append_to_section_does_not_bleed_into_next_section() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.append_to_section("Milestones", "- [ ] third\n")
        .unwrap();
    let next = doc.section("Next Actions").unwrap();
    assert!(!next.contains("- [ ] third"));
    assert!(next.contains("- [ ] call Alice"));
}

#[test]
fn append_to_section_missing_returns_not_found() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let err = doc.append_to_section("Nowhere", "x").unwrap_err();
    assert!(matches!(err, ManipulationError::SectionNotFound(_)));
}

#[test]
fn render_after_parse_returns_original_raw() {
    let doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    assert_eq!(doc.render(), SIMPLE_DOC);
}

#[test]
fn render_after_replace_reflects_changes() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.replace_section("Milestones", "- [x] replaced\n")
        .unwrap();
    let rendered = doc.render();
    assert!(rendered.contains("- [x] replaced"));
    assert!(!rendered.contains("- [ ] first"));
    // The other sections survive.
    assert!(rendered.contains("- [ ] call Alice"));
    assert!(rendered.contains("other top content"));
}

#[test]
fn render_after_append_reflects_changes() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.append_to_section("Next Actions", "- [ ] call Bob\n")
        .unwrap();
    let rendered = doc.render();
    assert!(rendered.contains("- [ ] call Alice"));
    assert!(rendered.contains("- [ ] call Bob"));
}

#[test]
fn section_with_empty_content_returns_empty_string() {
    let raw = "\
---
title: x
---
## Empty

## Next
content
";
    let doc = MarkdownDocument::parse(raw).unwrap();
    let empty = doc.section("Empty").unwrap();
    assert!(empty.trim().is_empty());
}

#[test]
fn multiple_operations_compose() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.append_to_section("Milestones", "- [ ] third\n")
        .unwrap();
    doc.replace_section("Next Actions", "- [ ] call Bob\n")
        .unwrap();

    let m = doc.section("Milestones").unwrap();
    assert!(m.contains("- [ ] first"));
    assert!(m.contains("- [ ] third"));

    let n = doc.section("Next Actions").unwrap();
    assert!(n.contains("- [ ] call Bob"));
    assert!(!n.contains("- [ ] call Alice"));
}
