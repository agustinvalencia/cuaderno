use cdno_core::markdown::{MarkdownDocument, extract_hard_deadlines};

#[test]
fn extracts_single_hard_deadline() {
    let section = "- [ ] ICML paper submitted — hard: 2026-05-22\n";
    let out = extract_hard_deadlines(section);
    assert_eq!(
        out,
        vec![("ICML paper submitted".to_owned(), "2026-05-22".to_owned())]
    );
}

#[test]
fn extracts_multiple_hard_deadlines_in_order() {
    let section = "\
- [ ] ship v1 — hard: 2026-05-22
- [ ] ship v2 — hard: 2026-09-01
";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].0, "ship v1");
    assert_eq!(out[0].1, "2026-05-22");
    assert_eq!(out[1].0, "ship v2");
    assert_eq!(out[1].1, "2026-09-01");
}

#[test]
fn skips_completed_milestones() {
    // Completed items never become commitments — the work is done.
    let section = "\
- [x] Baseline trained — hard: 2026-02-10
- [ ] ICML paper submitted — hard: 2026-05-22
";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "ICML paper submitted");
}

#[test]
fn skips_soft_target_milestones() {
    // `target:` indicates a fuzzy goal; only `hard:` becomes a commitment.
    let section = "\
- [ ] Full geometry evaluation — target: April
- [ ] ICML paper submitted — hard: 2026-05-22
";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "ICML paper submitted");
}

#[test]
fn skips_lines_that_are_not_list_items() {
    let section = "\
This is prose mentioning hard: 2026-05-22 inline.
- [ ] ICML paper submitted — hard: 2026-05-22
";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
}

#[test]
fn rejects_invalid_date_format() {
    // Anything that is not YYYY-MM-DD should not produce a deadline.
    let section = "\
- [ ] sloppy — hard: May 22, 2026
- [ ] sloppy short — hard: 26-05-22
- [ ] valid — hard: 2026-05-22
";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "valid");
}

#[test]
fn rejects_nonexistent_calendar_date() {
    // Format passes but the date is impossible — chrono validation
    // catches it so downstream commitments aggregation never sees a
    // ghost date.
    let section = "- [ ] impossible — hard: 2026-02-30\n";
    let out = extract_hard_deadlines(section);
    assert!(out.is_empty());
}

#[test]
fn handles_hyphen_separator_before_hard() {
    // Not every user will type an em-dash. A plain hyphen should work.
    let section = "- [ ] ship v1 - hard: 2026-05-22\n";
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "ship v1");
}

#[test]
fn trims_title_whitespace() {
    let section = "- [ ]   ship v1   —   hard: 2026-05-22\n";
    let out = extract_hard_deadlines(section);
    assert_eq!(out[0].0, "ship v1");
}

#[test]
fn empty_section_returns_no_deadlines() {
    assert!(extract_hard_deadlines("").is_empty());
    assert!(extract_hard_deadlines("\n\n").is_empty());
}

#[test]
fn no_deadline_lines_returns_empty() {
    let section = "\
- [ ] just a task
- [ ] another with target: sometime
";
    assert!(extract_hard_deadlines(section).is_empty());
}

#[test]
fn composes_with_markdown_document_section() {
    // The realistic flow: parse a project note, pull the Milestones
    // section, then extract hard deadlines from it. Verifies the
    // extractor works on what `MarkdownDocument::section` actually
    // returns.
    let raw = "\
---
type: project
---
## Milestones
- [x] Baseline done — hard: 2026-02-10
- [ ] ICML paper submitted — hard: 2026-05-22
- [ ] Full geometry evaluation — target: April

## Next Actions
- [ ] draft abstract
";
    let doc = MarkdownDocument::parse(raw).unwrap();
    let section = doc.section("Milestones").unwrap();
    let out = extract_hard_deadlines(section);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "ICML paper submitted");
    assert_eq!(out[0].1, "2026-05-22");
}
