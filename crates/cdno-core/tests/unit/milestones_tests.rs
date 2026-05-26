use cdno_core::index::MilestoneEntry;
use cdno_core::markdown::{MarkdownDocument, extract_milestones_from_body};

fn m(name: &str, date: Option<&str>, is_hard: bool, completed: bool) -> MilestoneEntry {
    MilestoneEntry {
        name: name.to_owned(),
        date: date.map(str::to_owned),
        is_hard,
        completed,
    }
}

#[test]
fn extracts_hard_deadline_milestone() {
    let section = "- [ ] ICML paper submitted — hard: 2026-05-22\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m("ICML paper submitted", Some("2026-05-22"), true, false)],
    );
}

#[test]
fn extracts_soft_target_with_iso_date() {
    let section = "- [ ] Full geometry evaluation — target: 2026-04-30\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m(
            "Full geometry evaluation",
            Some("2026-04-30"),
            false,
            false
        )],
    );
}

#[test]
fn soft_target_with_fuzzy_marker_has_no_date() {
    // `target: April` / `target: TBD` are legitimate milestones with no
    // sortable date — captured, but excluded from date-window queries.
    let section = "\
- [ ] Full geometry evaluation — target: April
- [ ] First milestone — target: TBD
";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![
            m("Full geometry evaluation", None, false, false),
            m("First milestone", None, false, false),
        ],
    );
}

#[test]
fn extracts_completed_milestone_with_trailing_date() {
    // The keyword-less completed shape from design §5.3: `- [x] <name>
    // — YYYY-MM-DD`. The trailing date is the date it fired.
    let section = "- [x] Baseline dense model trained — 2026-02-10\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m(
            "Baseline dense model trained",
            Some("2026-02-10"),
            false,
            true
        )],
    );
}

#[test]
fn completed_hard_milestone_keeps_hard_flag() {
    // A hard milestone that has since been checked off stays `is_hard`.
    let section = "- [x] Baseline done — hard: 2026-02-10\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m("Baseline done", Some("2026-02-10"), true, true)],
    );
}

#[test]
fn keeps_undated_milestone_with_no_marker() {
    // A bare checklist item in the Milestones section is still a
    // milestone — just one without a date.
    let section = "- [ ] Define first concrete step\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m("Define first concrete step", None, false, false)],
    );
}

#[test]
fn does_not_slice_a_date_out_of_the_middle_of_a_name() {
    // An embedded date that isn't at the end stays part of the name;
    // only a trailing, separator-preceded date is split off.
    let section = "- [ ] Review 2024-01-01 retro notes\n";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m("Review 2024-01-01 retro notes", None, false, false)],
    );
}

#[test]
fn skips_non_checklist_lines() {
    let section = "\
## Milestones
Some prose about the plan.
- [ ] ICML paper submitted — hard: 2026-05-22
";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![m("ICML paper submitted", Some("2026-05-22"), true, false)],
    );
}

#[test]
fn rejects_invalid_dates() {
    // Malformed or impossible dates yield a milestone with no date,
    // not a ghost date.
    let section = "\
- [ ] bad format — hard: May 22
- [ ] impossible — hard: 2026-02-30
";
    assert_eq!(
        extract_milestones_from_body(section),
        vec![
            m("bad format", None, true, false),
            m("impossible", None, true, false),
        ],
    );
}

#[test]
fn empty_section_returns_no_milestones() {
    assert!(extract_milestones_from_body("").is_empty());
    assert!(extract_milestones_from_body("\n\n").is_empty());
}

#[test]
fn composes_with_markdown_document_section() {
    // Realistic flow: parse a project note, pull the Milestones
    // section, extract the full timeline.
    let raw = "\
---
type: project
---
## Milestones
- [x] Baseline done — 2026-02-10
- [ ] ICML paper submitted — hard: 2026-05-22
- [ ] Full geometry evaluation — target: April

## Next Actions
- [ ] draft abstract
";
    let doc = MarkdownDocument::parse(raw).unwrap();
    let section = doc.section("Milestones").unwrap();
    assert_eq!(
        extract_milestones_from_body(section),
        vec![
            m("Baseline done", Some("2026-02-10"), false, true),
            m("ICML paper submitted", Some("2026-05-22"), true, false),
            m("Full geometry evaluation", None, false, false),
        ],
    );
}
