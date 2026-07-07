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
fn ensure_section_is_noop_when_present() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let before = doc.render().to_owned();
    doc.ensure_section("Milestones").unwrap();
    assert_eq!(
        doc.render(),
        before,
        "existing section must not be re-added"
    );
}

#[test]
fn ensure_section_appends_when_absent() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.ensure_section("Waiting On").unwrap();
    let rendered = doc.render();
    assert!(
        rendered.contains("## Waiting On"),
        "new heading present:\n{rendered}"
    );
    // Subsequent operations on the section work normally.
    doc.append_to_section("Waiting On", "- new item\n").unwrap();
    assert!(doc.render().contains("- new item"));
}

#[test]
fn ensure_section_separates_with_blank_line() {
    // The existing body ends with a heading + content; the new
    // section should not run into the previous one.
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    doc.ensure_section("Brand New").unwrap();
    let rendered = doc.render();
    // Find the trailing chunk and assert it's separated cleanly.
    assert!(
        rendered.contains("\n\n## Brand New\n"),
        "missing blank-line separator:\n{rendered}"
    );
}

#[test]
fn ensure_section_preserves_existing_when_ambiguous() {
    // If two headings already share the text, ensure_section
    // should NOT add a third — existing structure is left as-is
    // and the caller's later `section()` call will surface the
    // ambiguity.
    let raw = "\
---
title: x
---
## Same
a

## Same
b
";
    let mut doc = MarkdownDocument::parse(raw).unwrap();
    doc.ensure_section("Same").unwrap();
    let rendered = doc.render();
    let count = rendered.matches("## Same").count();
    assert_eq!(count, 2, "no extra heading added; got:\n{rendered}");
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
fn nested_fenced_code_blocks_do_not_leak_fake_headings() {
    // An outer 4-backtick fence contains a 3-backtick inner fence.
    // CommonMark closes the outer block only on a line of >= 4
    // backticks, so headings inside both fences must be invisible
    // to section lookup. The doc has exactly one real heading,
    // `## Real`, sitting before the outer fence.
    let raw = "\
---
title: x
---
## Real
before fence

````md
# Outer fake heading

```python
def fn(): pass
```

# Another outer fake
````
after fence
";
    let doc = MarkdownDocument::parse(raw).unwrap();

    // Neither fake heading is discoverable.
    assert!(matches!(
        doc.section("Outer fake heading").unwrap_err(),
        ManipulationError::SectionNotFound(_)
    ));
    assert!(matches!(
        doc.section("Another outer fake").unwrap_err(),
        ManipulationError::SectionNotFound(_)
    ));

    // The real heading's content includes the entire nested-fence
    // block verbatim plus the trailing text after it.
    let real = doc.section("Real").unwrap();
    assert!(real.contains("before fence"));
    assert!(real.contains("````md"));
    assert!(real.contains("# Outer fake heading"));
    assert!(real.contains("```python"));
    assert!(real.contains("def fn(): pass"));
    assert!(real.contains("# Another outer fake"));
    assert!(real.contains("after fence"));
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

// ---------------------------------------------------------------------
// move_section_to_end (#232 — keep `## Logs` last)
// ---------------------------------------------------------------------

const DRIFTED_DAILY: &str = "\
---
type: daily
---
# Monday

## Logs
- **09:00**: started
- **10:00**: standup

## Meeting
notes from the meeting
";

#[test]
fn move_section_to_end_moves_a_mid_note_section_below_later_ones() {
    let mut doc = MarkdownDocument::parse(DRIFTED_DAILY).unwrap();
    doc.move_section_to_end("Logs").unwrap();
    let out = doc.render();

    let meeting = out.find("## Meeting").expect("Meeting present");
    let logs = out.find("## Logs").expect("Logs present");
    assert!(meeting < logs, "Logs should now be last:\n{out}");
    // Content of both sections preserved verbatim.
    assert!(
        out.contains("- **10:00**: standup"),
        "log content kept:\n{out}"
    );
    assert!(
        out.contains("notes from the meeting"),
        "meeting kept:\n{out}"
    );
    // Heading appears exactly once (not duplicated by the move).
    assert_eq!(out.matches("## Logs").count(), 1);
}

#[test]
fn move_section_to_end_is_a_noop_when_already_last() {
    let src = "---\ntype: daily\n---\n# Mon\n\n## Meeting\nm\n\n## Logs\n- x\n";
    let mut doc = MarkdownDocument::parse(src).unwrap();
    doc.move_section_to_end("Logs").unwrap();
    assert_eq!(
        doc.render(),
        src,
        "an already-last section is left byte-identical"
    );
}

#[test]
fn move_section_to_end_is_idempotent() {
    let mut doc = MarkdownDocument::parse(DRIFTED_DAILY).unwrap();
    doc.move_section_to_end("Logs").unwrap();
    let once = doc.render().to_owned();
    doc.move_section_to_end("Logs").unwrap();
    assert_eq!(
        doc.render(),
        once,
        "moving an already-last section changes nothing"
    );
}

#[test]
fn move_section_to_end_is_a_noop_when_section_absent() {
    let mut doc = MarkdownDocument::parse(SIMPLE_DOC).unwrap();
    let before = doc.render().to_owned();
    doc.move_section_to_end("Logs").unwrap();
    assert_eq!(doc.render(), before, "absent section: nothing to move");
}

#[test]
fn move_section_to_end_carries_nested_subheadings_and_body_verbatim() {
    // The moved level-2 section owns its nested level-3 subsection;
    // moving it must carry the `###` heading and the exact body text
    // along, not just the `##` line. Multi-byte UTF-8 in the body
    // guards the byte-range splice against off-by-one truncation.
    let src = "\
---
type: daily
---
# Día

## Logs
- **09:00**: café ☕ — started

### Detail
nested — résumé

## Meeting
notes from the meeting
";
    let mut doc = MarkdownDocument::parse(src).unwrap();
    doc.move_section_to_end("Logs").unwrap();
    let out = doc.render();

    let meeting = out.find("## Meeting").expect("Meeting present");
    let logs = out.find("## Logs").expect("Logs present");
    assert!(
        meeting < logs,
        "Logs (with its subheading) moved last:\n{out}"
    );
    // The nested `### Detail` travelled with its parent section, in order.
    let nested = out.find("### Detail").expect("nested heading kept");
    assert!(logs < nested, "### Detail stays under ## Logs:\n{out}");
    // Bodies preserved verbatim, multi-byte intact.
    assert!(
        out.contains("- **09:00**: café ☕ — started"),
        "log body verbatim:\n{out}"
    );
    assert!(
        out.contains("nested — résumé"),
        "nested body verbatim:\n{out}"
    );
    assert!(
        out.contains("notes from the meeting"),
        "meeting body kept:\n{out}"
    );
    assert_eq!(out.matches("## Logs").count(), 1, "heading not duplicated");
}

// ---------------------------------------------------------------------
// extract_first_table
// ---------------------------------------------------------------------

use cdno_core::markdown::extract_first_table;

#[test]
fn extract_first_table_parses_headers_and_rows() {
    let body = "\
# Gym — 6 April 2026

| Exercise | Sets | Reps | Weight (kg) |
|----------|------|------|-------------|
| Squat    | 3    | 8    | 80          |
| Bench    | 3    | 10   | 60          |

## Notes
felt strong
";
    let table = extract_first_table(body).unwrap();
    assert_eq!(
        table.headers,
        vec!["Exercise", "Sets", "Reps", "Weight (kg)"]
    );
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0], vec!["Squat", "3", "8", "80"]);
    assert_eq!(table.rows[1], vec!["Bench", "3", "10", "60"]);
}

#[test]
fn extract_first_table_returns_first_of_several() {
    let body = "\
| A | B |
|---|---|
| 1 | 2 |

| C |
|---|
| 3 |
";
    let table = extract_first_table(body).unwrap();
    assert_eq!(table.headers, vec!["A", "B"]);
    assert_eq!(table.rows, vec![vec!["1", "2"]]);
}

#[test]
fn extract_first_table_requires_delimiter_row() {
    // Two adjacent pipe-prefixed lines without a delimiter row are
    // prose, not a table.
    let body = "| not | a table |\n| just | pipes |\n";
    assert!(extract_first_table(body).is_none());
}

#[test]
fn extract_first_table_none_on_tableless_body() {
    assert!(extract_first_table("# Heading\n\nplain prose\n").is_none());
}

#[test]
fn extract_first_table_tolerates_alignment_colons_and_ragged_rows() {
    let body = "\
| Metric | Value |
|:-------|------:|
| Weight | 82.5  |
| Resting HR |
";
    let table = extract_first_table(body).unwrap();
    assert_eq!(table.headers, vec!["Metric", "Value"]);
    // The ragged second row survives with its actual width.
    assert_eq!(table.rows[0], vec!["Weight", "82.5"]);
    assert_eq!(table.rows[1], vec!["Resting HR"]);
}

#[test]
fn extract_first_table_keeps_interior_empty_cells() {
    let body = "| A | B | C |\n|---|---|---|\n| 1 |   | 3 |\n";
    let table = extract_first_table(body).unwrap();
    assert_eq!(table.rows[0], vec!["1", "", "3"]);
}
