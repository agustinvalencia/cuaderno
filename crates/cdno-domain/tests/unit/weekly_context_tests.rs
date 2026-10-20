//! Unit tests for `Vault::weekly_context` — the composed weekly-review
//! snapshot (RFC 0006 §8.0). `MemoryVaultStore` / `MemoryIndex` keep it
//! fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use chrono::{Datelike, Duration, NaiveDate};

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(String, String)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(path).unwrap(), body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

/// A daily note for `date` whose `## Logs` section carries `lines`.
fn daily(date: NaiveDate, lines: &str) -> (String, String) {
    (
        cdno_core::paths::daily_note_relpath(date),
        format!(
            "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{lines}",
            date = date.format("%Y-%m-%d"),
        ),
    )
}

/// `count` plain checkpoint lines.
fn checkpoints(count: usize) -> String {
    (0..count)
        .map(|i| format!("- **10:{:02}**: checkpoint {i}\n", i % 60))
        .collect()
}

#[test]
fn weekly_context_composes_every_field_from_its_source() {
    // 2030-06-12 is a Wednesday: the week runs Mon 06-10 to Sun 06-16.
    let today = ymd(2030, 6, 12);
    let monday = ymd(2030, 6, 10);
    let sunday = ymd(2030, 6, 16);

    // 120 log lines across the week, more than the 100 an interface may
    // keep: 59 checkpoints and a state change on Monday, 59 checkpoints
    // and a bullet completion today. The composition must not bound them.
    let monday_logs = format!(
        "- **08:00**: state on [[alpha]]\n  was: drafting\n  now: the parser is in\n{}",
        checkpoints(59)
    );
    let today_logs = format!(
        "{}- **17:00**: action done on [[alpha]] \u{2014} Write the boundary tests (deep)\n",
        checkpoints(59)
    );
    let vault = vault_with(&[
        daily(monday, &monday_logs),
        daily(today, &today_logs),
        // Outside the week: its line must not be counted.
        daily(ymd(2030, 6, 9), "- **09:00**: last Sunday\n"),
        (
            "actions/_done/2030/win.md".to_owned(),
            "---\ntype: action\nstatus: completed\nproject: alpha\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2030-06-01\ncompleted: 2030-06-11\nblocker: null\ncriteria: null\ntags: []\n---\n\n# Win\n".to_owned(),
        ),
        (
            "projects/_done/2030/shipped.md".to_owned(),
            "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2030-01-01\nclosed: 2030-06-11\n---\n\n# Shipped\n".to_owned(),
        ),
        (
            "commitments/send-the-draft.md".to_owned(),
            "---\ntype: commitment\nstatus: active\ndue: 2030-06-20\ncreated: 2030-06-01\ncompleted: null\ncontext: work\n---\n\n# Send the draft\n".to_owned(),
        ),
    ]);

    let ctx = vault.weekly_context(today).unwrap();

    assert_eq!(ctx.week_of, monday);

    assert_eq!(ctx.logs, vault.weekly_logs(today).unwrap());
    assert_eq!(ctx.logs.len(), 120, "every line of the week, unbounded");

    let completed = vault.completed_actions_between(monday, sunday).unwrap();
    assert_eq!(ctx.completed_actions, completed);
    assert_eq!(
        ctx.completed_actions.len(),
        2,
        "note and bullet: {completed:?}"
    );

    assert_eq!(
        ctx.closed_projects,
        vault.closed_projects_between(monday, sunday).unwrap()
    );
    assert_eq!(ctx.closed_projects.len(), 1);
    assert_eq!(ctx.closed_projects[0].slug, "shipped");

    assert_eq!(
        ctx.state_changes,
        vault.project_state_changes_between(monday, sunday).unwrap()
    );
    assert_eq!(ctx.state_changes.len(), 1);
    assert_eq!(ctx.state_changes[0].old_state, "drafting", "full bodies");

    assert_eq!(ctx.commitments, vault.commitments(today, 14).unwrap());
    assert_eq!(ctx.commitments.len(), 1);
    assert_eq!(ctx.commitments[0].title, "Send the draft");

    assert!(ctx.projects.is_empty(), "no active map in this vault");
}

// ---------------------------------------------------------------------
// The review fields (RFC 0006 §8.5). Dates are relative to the real
// clock: the in-memory store stamps every map with it.
// ---------------------------------------------------------------------

fn real_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// The Monday of the ISO week before the current one, so the whole
/// Monday-to-Friday span lies in the past whatever day the suite runs.
fn last_monday() -> NaiveDate {
    let today = real_today();
    today - Duration::days(i64::from(today.weekday().num_days_from_monday()) + 7)
}

/// An active project map at `projects/<slug>.md`.
fn active_map(slug: &str, holder: Option<&str>, extra: &str, waiting: &str) -> (String, String) {
    let held = holder
        .map(|h| format!("held_by: \"{h}\"\n"))
        .unwrap_or_default();
    (
        format!("projects/{slug}.md"),
        format!(
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n{held}{extra}---\n\n\
             # {slug}\n\n## Current State\nN/A.\n\n## Next Actions\n\n## Waiting On\n{waiting}"
        ),
    )
}

#[test]
fn a_held_map_reports_the_days_with_work_started() {
    // Starts on Monday and Wednesday, and a resume on Wednesday: three
    // lines over two days, so the count is 2. A start on the Sunday
    // before is outside the week; a start on another map is not this one's.
    let monday = last_monday();
    let wednesday = monday + Duration::days(2);
    let friday = monday + Duration::days(4);
    let vault = vault_with(&[
        active_map(
            "harbour",
            Some("the harbour office"),
            "",
            "- Berth survey from the harbour office\n",
        ),
        active_map("quay", None, "", "(nothing yet)\n"),
        daily(
            monday - Duration::days(1),
            "- **09:00**: started [[harbour]] \u{2014} Read the survey (light)\n",
        ),
        daily(
            monday,
            "- **09:00**: started [[harbour]] \u{2014} Read the survey (light)\n\
             - **11:00**: started [[quay]] \u{2014} Draft the plan (deep)\n",
        ),
        daily(
            wednesday,
            "- **09:00**: started [[harbour]] \u{2014} Check the berths (medium)\n\
             - **10:00**: action paused on [[harbour]] \u{2014} Check the berths (medium)\n\
             - **14:00**: resumed [[harbour]] \u{2014} Check the berths (medium)\n",
        ),
    ]);

    let ctx = vault.weekly_context(friday).unwrap();

    let harbour = ctx.projects.iter().find(|p| p.slug == "harbour").unwrap();
    assert_eq!(harbour.focus_days_while_held, 2, "days, not lines");
}

#[test]
fn a_map_not_held_reports_zero_days() {
    // Work started on the person's own map on two days is not evidence
    // about anyone else: the count is reported for a held map only.
    let monday = last_monday();
    let vault = vault_with(&[
        active_map("quay", None, "", "(nothing yet)\n"),
        daily(
            monday,
            "- **09:00**: started [[quay]] \u{2014} Draft the plan (deep)\n",
        ),
        daily(
            monday + Duration::days(1),
            "- **09:00**: started [[quay]] \u{2014} Revise the plan (deep)\n",
        ),
    ]);

    let ctx = vault.weekly_context(monday + Duration::days(4)).unwrap();

    let quay = ctx.projects.iter().find(|p| p.slug == "quay").unwrap();
    assert_eq!(quay.held_by, None);
    assert_eq!(quay.focus_days_while_held, 0);
}

#[test]
fn changed_this_week_follows_the_map_mtime() {
    // The map is written now. Read in the current week its mtime falls
    // inside the ISO week; read a week later it falls in the week before.
    let vault = vault_with(&[active_map("quay", None, "", "(nothing yet)\n")]);
    let today = real_today();

    let this_week = vault.weekly_context(today).unwrap();
    assert!(this_week.projects[0].changed_this_week);

    let next_week = vault.weekly_context(today + Duration::days(7)).unwrap();
    assert!(!next_week.projects[0].changed_this_week);
}

#[test]
fn review_projects_carry_held_by_counts_and_waiting_on_empty() {
    // Every active map, held ones included and sorted by slug; parked
    // and closed maps are not the weekly review's.
    let vault = vault_with(&[
        active_map(
            "harbour",
            Some("the harbour office"),
            "",
            "- Berth survey from the harbour office\n",
        ),
        active_map(
            "quay",
            None,
            "core_question: \"[[questions/research/tides]]\"\n",
            "(nothing yet)\n",
        ),
        active_map("ferry", Some("the ferry office"), "", ""),
        (
            "projects/_parked/orchard.md".to_owned(),
            "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-01-01\n---\n\n# Orchard\n".to_owned(),
        ),
    ]);

    let ctx = vault.weekly_context(real_today()).unwrap();

    let slugs: Vec<&str> = ctx.projects.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(slugs, vec!["ferry", "harbour", "quay"]);

    let ferry = &ctx.projects[0];
    assert_eq!(ferry.held_by.as_deref(), Some("the ferry office"));
    assert!(!ferry.counts);
    assert!(ferry.waiting_on_empty, "a name over an empty Waiting On");
    assert_eq!(ferry.core_question, None);

    let harbour = &ctx.projects[1];
    assert_eq!(harbour.held_by.as_deref(), Some("the harbour office"));
    assert!(!harbour.counts);
    assert!(!harbour.waiting_on_empty);

    let quay = &ctx.projects[2];
    assert_eq!(quay.held_by, None);
    assert!(quay.counts);
    assert!(quay.waiting_on_empty, "the placeholder says nothing");
    assert_eq!(
        quay.core_question.as_deref(),
        Some("[[questions/research/tides]]")
    );
    assert!(quay.changed_this_week, "written now, read this week");
}
