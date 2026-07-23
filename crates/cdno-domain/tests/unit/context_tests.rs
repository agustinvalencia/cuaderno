//! Unit tests for the eight context-gathering domain queries
//! introduced in GH #142. Each method gets a happy-path test plus a
//! targeted edge case (window boundaries, missing files, malformed
//! input). All run against `MemoryVaultStore` + `MemoryIndex`.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::Context;
use cdno_domain::vault::{days_since_mtime_in, mtime_threshold_ns_in};
use cdno_domain::{
    CompletedActionEntry, DailyLogLine, ProjectBacklinks, ProjectStateChange, QuestionBacklinks,
    TrackingEntry,
};
use chrono::{FixedOffset, NaiveDate, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

// Pre-built daily-note bodies. `## Logs` section is the one all
// context queries consume; we keep the surrounding scaffold minimal.
fn daily_with_logs(date: NaiveDate, log_lines: &str) -> String {
    format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{lines}",
        date = date.format("%Y-%m-%d"),
        lines = log_lines,
    )
}

fn daily_path(date: NaiveDate) -> String {
    cdno_core::paths::daily_note_relpath(date)
}

// ---------------------------------------------------------------------
// weekly_logs
// ---------------------------------------------------------------------

#[test]
fn weekly_logs_returns_entries_from_every_day_in_iso_week() {
    // 2026-04-08 is a Wednesday → ISO week is Mon 2026-04-06 to Sun 2026-04-12.
    let monday = ymd(2026, 4, 6);
    let wednesday = ymd(2026, 4, 8);
    let sunday = ymd(2026, 4, 12);
    let (vault, _store) = vault_with(&[
        (
            &daily_path(monday),
            &daily_with_logs(monday, "- **08:00**: standup\n"),
        ),
        (
            &daily_path(wednesday),
            &daily_with_logs(wednesday, "- **14:30**: deep work\n"),
        ),
        (
            &daily_path(sunday),
            &daily_with_logs(sunday, "- **10:00**: weekly review\n"),
        ),
        // Outside the week — must be excluded.
        (
            &daily_path(ymd(2026, 4, 13)),
            &daily_with_logs(ymd(2026, 4, 13), "- **08:00**: next week\n"),
        ),
    ]);
    let lines = vault.weekly_logs(wednesday).unwrap();
    assert_eq!(lines.len(), 3, "{lines:?}");
    let dates: Vec<NaiveDate> = lines.iter().map(|l| l.date).collect();
    assert_eq!(dates, vec![monday, wednesday, sunday]);
}

#[test]
fn weekly_logs_returns_empty_when_no_dailies_in_week() {
    let (vault, _store) = vault_with(&[]);
    assert!(vault.weekly_logs(ymd(2026, 4, 8)).unwrap().is_empty());
}

#[test]
fn weekly_logs_folds_multi_line_log_entries_into_one_text() {
    let date = ymd(2026, 4, 8);
    let logs = "- **14:30**: state on [[surrogate]]\n  was: blocked\n  now: sweep B running\n";
    let (vault, _store) = vault_with(&[(&daily_path(date), &daily_with_logs(date, logs))]);
    let lines = vault.weekly_logs(date).unwrap();
    assert_eq!(lines.len(), 1);
    let text = &lines[0].text;
    assert!(text.contains("state on [[surrogate]]"));
    assert!(text.contains("was: blocked"));
    assert!(text.contains("now: sweep B running"));
}

// ---------------------------------------------------------------------
// completed_actions_between
// ---------------------------------------------------------------------

fn action_note(slug: &str, project: &str, status: &str, completed: &str) -> String {
    format!(
        "---\ntype: action\nstatus: {status}\nproject: {project}\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2026-05-01\ncompleted: {completed}\nblocker: null\ncriteria: null\ntags: []\n---\n\n# {slug}\n"
    )
}

#[test]
fn completed_actions_between_filters_by_date_and_status() {
    let (vault, _store) = vault_with(&[
        // Completed in window
        (
            "actions/_done/2026/win.md",
            &action_note("Win", "alpha", "completed", "2026-05-15"),
        ),
        // Completed before window
        (
            "actions/_done/2026/early.md",
            &action_note("Early", "alpha", "completed", "2026-04-30"),
        ),
        // Still active
        (
            "actions/active.md",
            &action_note("Active", "alpha", "active", "null"),
        ),
    ]);
    let got: Vec<CompletedActionEntry> = vault
        .completed_actions_between(ymd(2026, 5, 1), ymd(2026, 5, 31))
        .unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].slug, "win");
    assert_eq!(got[0].project, "alpha");
    assert_eq!(got[0].completed, ymd(2026, 5, 15));
}

#[test]
fn completed_actions_between_sorts_oldest_first() {
    let (vault, _store) = vault_with(&[
        (
            "actions/_done/2026/late.md",
            &action_note("Late", "alpha", "completed", "2026-05-20"),
        ),
        (
            "actions/_done/2026/early.md",
            &action_note("Early", "alpha", "completed", "2026-05-05"),
        ),
    ]);
    let got = vault
        .completed_actions_between(ymd(2026, 5, 1), ymd(2026, 5, 31))
        .unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].slug, "early");
    assert_eq!(got[1].slug, "late");
}

// ---------------------------------------------------------------------
// project_state_changes_between
// ---------------------------------------------------------------------

#[test]
fn project_state_changes_between_parses_was_now_log_entries() {
    let date = ymd(2026, 5, 10);
    let logs = "- **14:30**: state on [[surrogate]]\n  was: blocked on data\n  now: sweep B underway\n- **15:00**: other entry\n";
    let (vault, _store) = vault_with(&[(&daily_path(date), &daily_with_logs(date, logs))]);
    let changes: Vec<ProjectStateChange> = vault.project_state_changes_between(date, date).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].project, "surrogate");
    assert_eq!(changes[0].old_state, "blocked on data");
    assert_eq!(changes[0].new_state, "sweep B underway");
}

#[test]
fn project_state_changes_between_excludes_dates_outside_window() {
    let in_range = ymd(2026, 5, 10);
    let out_of_range = ymd(2026, 5, 20);
    let logs = "- **14:30**: state on [[alpha]]\n  was: a\n  now: b\n";
    let (vault, _store) = vault_with(&[
        (&daily_path(in_range), &daily_with_logs(in_range, logs)),
        (
            &daily_path(out_of_range),
            &daily_with_logs(out_of_range, logs),
        ),
    ]);
    let changes = vault
        .project_state_changes_between(ymd(2026, 5, 1), ymd(2026, 5, 15))
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].date, in_range);
}

// ---------------------------------------------------------------------
// stuck_projects
// ---------------------------------------------------------------------

#[test]
fn stuck_projects_excludes_parked_projects() {
    // mtime is hard to control in tests (MemoryVaultStore stamps
    // construction time). What we CAN reliably test: parked projects
    // are excluded regardless of mtime, and the empty-vault case.
    let project = |status, name: &str| {
        format!(
            "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-01-01\n---\n\n# {name}\n\n## Current State\nN/A.\n\n## Next Actions\n"
        )
    };
    let (vault, _store) = vault_with(&[(
        "projects/_parked/parked-thing.md",
        &project("parked", "Parked"),
    )]);
    // 0-day threshold means "anything modified today or earlier" —
    // everything that exists would qualify if not for the parked
    // filter.
    let today = chrono::Local::now().date_naive();
    let stuck = vault.stuck_projects(today, 0).unwrap();
    assert!(
        stuck.iter().all(|p| p.slug != "parked-thing"),
        "parked projects must be filtered: {stuck:?}"
    );
}

#[test]
fn stuck_projects_returns_empty_when_threshold_far_in_future() {
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n---\n\n# Fresh\n\n## Current State\nN/A.\n\n## Next Actions\n";
    let (vault, _store) = vault_with(&[("projects/fresh.md", project)]);
    // 36500-day threshold (~100 years) — no real file qualifies,
    // and the date subtraction stays well within chrono's range.
    let today = chrono::Local::now().date_naive();
    let stuck = vault.stuck_projects(today, 36500).unwrap();
    assert!(stuck.is_empty(), "{stuck:?}");
}

// ---------------------------------------------------------------------
// get_project_full
// ---------------------------------------------------------------------

#[test]
fn get_project_full_returns_frontmatter_and_body_for_active() {
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate model\n\n## Current State\nSweep B running.\n\n## Next Actions\n- [ ] Run sweep B (deep)\n";
    let (vault, _store) = vault_with(&[("projects/surrogate-model.md", body)]);
    let (fm, body) = vault.get_project_full("surrogate-model").unwrap();
    assert_eq!(fm.context, Context::Work);
    assert!(body.contains("# Surrogate model"));
    assert!(body.contains("## Current State"));
}

#[test]
fn get_project_full_resolves_parked_projects() {
    let body =
        "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-05-01\n---\n\n# Parked\n";
    let (vault, _store) = vault_with(&[("projects/_parked/parked-thing.md", body)]);
    let (fm, body) = vault.get_project_full("parked-thing").unwrap();
    use cdno_domain::frontmatter::ProjectStatus;
    assert_eq!(fm.status, ProjectStatus::Parked);
    assert!(body.contains("# Parked"));
}

#[test]
fn get_project_full_errors_on_missing_slug() {
    let (vault, _store) = vault_with(&[]);
    let err = vault.get_project_full("nonexistent").unwrap_err();
    use cdno_core::error::StoreError;
    use cdno_domain::error::DomainError;
    assert!(matches!(err, DomainError::Store(StoreError::NotFound(_))));
}

// ---------------------------------------------------------------------
// daily_log_mentions
// ---------------------------------------------------------------------

#[test]
fn daily_log_mentions_matches_bare_and_qualified_wikilinks() {
    let (vault, _store) = vault_with(&[(
        &daily_path(ymd(2026, 5, 10)),
        &daily_with_logs(
            ymd(2026, 5, 10),
            "- **09:00**: bare mention [[surrogate]]\n- **10:00**: qualified [[projects/surrogate]]\n- **11:00**: irrelevant\n",
        ),
    )]);
    let mentions: Vec<DailyLogLine> = vault
        .daily_log_mentions("surrogate", ymd(2026, 5, 1))
        .unwrap();
    assert_eq!(mentions.len(), 2);
    assert!(mentions[0].text.contains("[[surrogate]]"));
    assert!(mentions[1].text.contains("[[projects/surrogate]]"));
}

#[test]
fn daily_log_mentions_excludes_dailies_before_since() {
    let (vault, _store) = vault_with(&[
        (
            &daily_path(ymd(2026, 4, 1)),
            &daily_with_logs(ymd(2026, 4, 1), "- **09:00**: [[surrogate]] kickoff\n"),
        ),
        (
            &daily_path(ymd(2026, 5, 10)),
            &daily_with_logs(ymd(2026, 5, 10), "- **09:00**: [[surrogate]] follow-up\n"),
        ),
    ]);
    let mentions = vault
        .daily_log_mentions("surrogate", ymd(2026, 5, 1))
        .unwrap();
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].date, ymd(2026, 5, 10));
}

// ---------------------------------------------------------------------
// project_backlinks
// ---------------------------------------------------------------------

#[test]
fn project_backlinks_groups_body_wikilinks_by_source_note_type() {
    // This case pins the body-link path: a question references the project
    // via a `## Related Projects` body section. (The frontmatter-link path —
    // a portfolio's `project:`, an evidence note's `origin:` — is indexed
    // too since #395; see `project_backlinks_includes_a_frontmatter_link`.)
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate\n\n## Current State\nN/A.\n\n## Next Actions\n";
    let question = "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# q?\n\n## Related Projects\n- [[projects/surrogate]]\n";
    let (vault, _store) = vault_with(&[
        ("projects/surrogate.md", project),
        ("questions/research/q.md", question),
    ]);
    let bl: ProjectBacklinks = vault.project_backlinks("surrogate").unwrap();
    assert_eq!(bl.questions.len(), 1, "{bl:?}");
    assert!(bl.portfolios.is_empty());
    assert!(bl.evidence.is_empty());
}

#[test]
fn project_backlinks_returns_empty_when_no_links() {
    let project =
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Lonely\n";
    let (vault, _store) = vault_with(&[("projects/lonely.md", project)]);
    let bl = vault.project_backlinks("lonely").unwrap();
    assert!(bl.portfolios.is_empty());
    assert!(bl.questions.is_empty());
}

#[test]
fn project_backlinks_includes_a_frontmatter_link() {
    // A portfolio links its project via the `project:` FRONTMATTER field,
    // not the body; since #395 that surfaces in the `portfolios` bucket.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate\n";
    let portfolio = "---\ntype: portfolio\nquestion: How does it behave?\nproject: \"[[projects/surrogate]]\"\ncreated: 2026-05-01\n---\n\n# Surrogate dossier\n";
    let (vault, _store) = vault_with(&[
        ("projects/surrogate.md", project),
        ("portfolios/surrogate/_index.md", portfolio),
    ]);
    let bl = vault.project_backlinks("surrogate").unwrap();
    assert_eq!(bl.portfolios.len(), 1, "frontmatter project: link: {bl:?}");
}

// ---------------------------------------------------------------------
// question_backlinks (#354)
// ---------------------------------------------------------------------

#[test]
fn question_backlinks_groups_body_wikilinks_by_source_note_type() {
    // A project that references the question in its body lands in the
    // `projects` bucket.
    let question = "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# q?\n";
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate\n\n## Current State\nExploring [[questions/research/q]].\n\n## Next Actions\n";
    let (vault, _store) = vault_with(&[
        ("questions/research/q.md", question),
        ("projects/surrogate.md", project),
    ]);
    let bl: QuestionBacklinks = vault.question_backlinks("q").unwrap();
    assert_eq!(bl.projects.len(), 1, "{bl:?}");
    assert!(bl.portfolios.is_empty());
    assert!(bl.evidence.is_empty());
    assert!(bl.other.is_empty());
}

#[test]
fn question_backlinks_includes_a_projects_core_question_frontmatter_link() {
    // A project's `core_question:` is a FRONTMATTER wikilink; since #395
    // frontmatter links are indexed too, so the project backlinks the
    // question it answers — the common case that makes the Strategic grid's
    // project chips (#354) actually populate.
    let question = "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# q?\n";
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\ncore_question: \"[[questions/research/q]]\"\n---\n\n# Surrogate\n\n## Current State\nGoing.\n\n## Next Actions\n";
    let (vault, _store) = vault_with(&[
        ("questions/research/q.md", question),
        ("projects/surrogate.md", project),
    ]);
    let bl = vault.question_backlinks("q").unwrap();
    assert_eq!(
        bl.projects.len(),
        1,
        "core_question should backlink: {bl:?}"
    );
}

#[test]
fn question_backlinks_returns_empty_when_no_links() {
    let question = "---\ntype: question\ndomain: life\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# lonely q?\n";
    let (vault, _store) = vault_with(&[("questions/life/lonely.md", question)]);
    let bl = vault.question_backlinks("lonely").unwrap();
    assert!(bl.projects.is_empty());
    assert!(bl.portfolios.is_empty());
    assert!(bl.evidence.is_empty());
    assert!(bl.other.is_empty());
}

#[test]
fn question_backlinks_errors_on_missing_question() {
    let (vault, _store) = vault_with(&[]);
    assert!(vault.question_backlinks("nope").is_err());
}

// ---------------------------------------------------------------------
// list_tracking
// ---------------------------------------------------------------------

fn tracking_note(stewardship: &str, activity: &str, date: &str, body: &str) -> String {
    format!(
        "---\ntype: tracking\nstewardship: {stewardship}\nactivity: {activity}\ndate: {date}\n---\n\n# {activity} {date}\n{body}"
    )
}

#[test]
fn list_tracking_filters_by_stewardship_and_window() {
    let (vault, _store) = vault_with(&[
        (
            "stewardships/health/tracking/2026-04-10-gym.md",
            &tracking_note("health", "gym", "2026-04-10", "Felt strong"),
        ),
        (
            "stewardships/health/tracking/2026-05-01-gym.md",
            &tracking_note("health", "gym", "2026-05-01", "Steady"),
        ),
        (
            "stewardships/finance/tracking/2026-04-15-budget.md",
            &tracking_note("finance", "budget", "2026-04-15", "Reviewed"),
        ),
    ]);
    let got: Vec<TrackingEntry> = vault
        .list_tracking("health", None, ymd(2026, 4, 1), ymd(2026, 4, 30))
        .unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].stewardship, "health");
    assert_eq!(got[0].date, ymd(2026, 4, 10));
    assert!(got[0].body_excerpt.contains("Felt strong"));
}

#[test]
fn list_tracking_filters_by_activity_when_supplied() {
    let (vault, _store) = vault_with(&[
        (
            "stewardships/health/tracking/2026-04-10-gym.md",
            &tracking_note("health", "gym", "2026-04-10", ""),
        ),
        (
            "stewardships/health/tracking/2026-04-11-body.md",
            &tracking_note("health", "body", "2026-04-11", ""),
        ),
    ]);
    let got = vault
        .list_tracking("health", Some("body"), ymd(2026, 4, 1), ymd(2026, 4, 30))
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].activity, "body");
}

#[test]
fn list_tracking_caps_body_excerpt_at_200_chars() {
    // Build a body line >200 chars to verify truncation.
    let long_line: String = std::iter::repeat_n('x', 300).collect();
    let (vault, _store) = vault_with(&[(
        "stewardships/h/tracking/2026-04-10-gym.md",
        &tracking_note("h", "gym", "2026-04-10", &long_line),
    )]);
    let got = vault
        .list_tracking("h", None, ymd(2026, 4, 1), ymd(2026, 4, 30))
        .unwrap();
    assert_eq!(got.len(), 1);
    let excerpt = &got[0].body_excerpt;
    // 200 chars + the ellipsis suffix character.
    let char_count = excerpt.chars().count();
    assert!(char_count <= 201, "excerpt should be bounded: {char_count}");
    assert!(excerpt.ends_with('…'));
}

// ---------------------------------------------------------------------
// tracking_series
// ---------------------------------------------------------------------

#[test]
fn tracking_series_sums_numeric_columns_per_note() {
    let session_1 = "\n| Exercise | Sets | Reps | Weight (kg) | Notes |\n|----------|------|------|-------------|-------|\n| Squat    | 3    | 8    | 80          | ok    |\n| Bench    | 3    | 10   | 60          |       |\n";
    let session_2 = "\n| Exercise | Sets | Reps | Weight (kg) | Notes |\n|----------|------|------|-------------|-------|\n| Squat    | 4    | 8    | 85          |       |\n";
    let (vault, _store) = vault_with(&[
        (
            "stewardships/health/tracking/2026-04-10-gym.md",
            &tracking_note("health", "gym", "2026-04-10", session_1),
        ),
        (
            "stewardships/health/tracking/2026-04-17-gym.md",
            &tracking_note("health", "gym", "2026-04-17", session_2),
        ),
    ]);

    let series = vault.tracking_series("health").unwrap();

    // Sets, Reps, Weight are numeric; Exercise and Notes never parse.
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "gym \u{b7} Reps",
            "gym \u{b7} Sets",
            "gym \u{b7} Weight (kg)"
        ]
    );
    let weight = series
        .iter()
        .find(|s| s.name == "gym \u{b7} Weight (kg)")
        .unwrap();
    assert_eq!(weight.points.len(), 2);
    assert_eq!(weight.points[0].date, ymd(2026, 4, 10));
    assert_eq!(weight.points[0].value, 140.0, "80 + 60 summed");
    assert_eq!(weight.points[1].value, 85.0);
}

#[test]
fn tracking_series_single_row_measurement_is_the_value_itself() {
    let body = "\n| Metric | Value |\n|--------|-------|\n| Weight | 82.5  |\n";
    let (vault, _store) = vault_with(&[(
        "stewardships/health/tracking/2026-04-10-body.md",
        &tracking_note("health", "body", "2026-04-10", body),
    )]);

    let series = vault.tracking_series("health").unwrap();

    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "body \u{b7} Value");
    assert_eq!(series[0].points[0].value, 82.5);
}

#[test]
fn tracking_series_skips_other_stewardships_and_tableless_notes() {
    let table = "\n| Laps |\n|------|\n| 20   |\n";
    let (vault, _store) = vault_with(&[
        (
            "stewardships/health/tracking/2026-04-10-swim.md",
            &tracking_note("health", "swim", "2026-04-10", table),
        ),
        (
            "stewardships/health/tracking/2026-04-11-gym.md",
            &tracking_note("health", "gym", "2026-04-11", "no table, just prose"),
        ),
        (
            "stewardships/finance/tracking/2026-04-12-budget.md",
            &tracking_note("finance", "budget", "2026-04-12", table),
        ),
    ]);

    let series = vault.tracking_series("health").unwrap();

    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "swim \u{b7} Laps");
    assert_eq!(series[0].points.len(), 1);
}

#[test]
fn tracking_series_ignores_non_finite_numerics() {
    // "inf"/"NaN" parse as f64 but would poison sums and serialise as
    // JSON null — they must not count as numeric cells.
    let body = "\n| Metric | Value | Mood |\n|--------|-------|------|\n| Weight | 82.5  | inf  |\n| Rest   | NaN   | good |\n";
    let (vault, _store) = vault_with(&[(
        "stewardships/health/tracking/2026-04-10-body.md",
        &tracking_note("health", "body", "2026-04-10", body),
    )]);

    let series = vault.tracking_series("health").unwrap();

    // Value keeps only the finite 82.5; Mood never yields a finite
    // number so no series exists for it.
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "body \u{b7} Value");
    assert_eq!(series[0].points[0].value, 82.5);
}

// -------------------------------------------------------------------
// Timezone-injected staleness boundary (#380 — the #379 regression,
// made deterministic). The production helpers read `chrono::Local`;
// these exercise the tz-injected seams with an explicit `FixedOffset`,
// so the assertions hold no matter the runner's own zone or the
// wall-clock time the suite happens to run at.
// -------------------------------------------------------------------

/// Nanoseconds since the Unix epoch for an RFC-3339 instant.
fn utc_ns(rfc3339: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .expect("valid rfc3339")
        .timestamp_nanos_opt()
        .expect("timestamp in range") as u64
}

#[test]
fn days_since_mtime_counts_in_the_injected_zone_not_utc() {
    // UTC+2. An mtime of 22:30Z on 2026-07-09 is 00:30 *local* on
    // 2026-07-10 — the same local calendar day as `today`. The correct
    // count is 0. The pre-#379 logic read the mtime's UTC date
    // (2026-07-09) against a local `today` and reported 1.
    let tz = FixedOffset::east_opt(2 * 3600).unwrap();
    let today = ymd(2026, 7, 10);
    let mtime_ns = utc_ns("2026-07-09T22:30:00Z");

    assert_eq!(days_since_mtime_in(today, mtime_ns, &tz), 0);

    // The same instant read in UTC lands a day earlier — the exact
    // off-by-one the local conversion fixes. Pinning it here documents
    // the boundary the fix moved.
    assert_eq!(days_since_mtime_in(today, mtime_ns, &chrono::Utc), 1);
}

#[test]
fn mtime_threshold_boundary_follows_the_injected_zone() {
    // At a zero-day threshold, "stuck" means mtime <= end of `today`.
    // In UTC+2 that boundary is 2026-07-10T21:59:59Z, not 23:59:59Z.
    let tz = FixedOffset::east_opt(2 * 3600).unwrap();
    let today = ymd(2026, 7, 10);
    let threshold = mtime_threshold_ns_in(today, 0, &tz);

    // 23:30 local *today* is within the window (the project counts as
    // touched today, so it registers as stuck at a zero-day threshold).
    assert!(utc_ns("2026-07-10T21:30:00Z") <= threshold);
    // 00:30 local *tomorrow* is past the window and must be excluded.
    assert!(utc_ns("2026-07-10T22:30:00Z") > threshold);

    // A UTC-interpreted threshold would wrongly admit the
    // tomorrow-local file — the membership side of the same bug.
    let utc_threshold = mtime_threshold_ns_in(today, 0, &chrono::Utc);
    assert!(utc_ns("2026-07-10T22:30:00Z") <= utc_threshold);
}

#[test]
fn project_backlinks_carry_a_frontmatter_title_when_the_source_has_one() {
    // The index's `title` is the frontmatter field, not the body H1 (the H1
    // feeds the FTS row instead). Most RLM note types carry their name in
    // the H1 and have no `title:` field, so this is `None` far more often
    // than not — the renderer falls back to the path. Pinned here so the
    // absence reads as a known shape rather than a bug.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate\n";
    let titled = "---\ntype: zettel\ntitle: Sparse variants hold up\n---\n\n# Sparse\n\nSee [[projects/surrogate]].\n";
    let untitled = "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# Does it hold up?\n\n## Related Projects\n- [[projects/surrogate]]\n";
    let (vault, _store) = vault_with(&[
        ("projects/surrogate.md", project),
        ("zettels/sparse.md", titled),
        ("questions/research/holds-up.md", untitled),
    ]);

    let bl = vault.project_backlinks("surrogate").unwrap();

    assert_eq!(bl.other.len(), 1, "the zettel lands in `other`: {bl:?}");
    assert_eq!(
        bl.other[0].title.as_deref(),
        Some("Sparse variants hold up"),
        "a frontmatter title is carried through"
    );
    assert_eq!(bl.questions.len(), 1, "{bl:?}");
    assert_eq!(
        bl.questions[0].title, None,
        "a note whose name lives in its H1 has no frontmatter title"
    );
}

#[test]
fn project_backlinks_are_ordered_newest_first() {
    // A project accrues backlinks for as long as it runs; the recent ones
    // are the context a reader wants. The contract is mtime descending with
    // the path as tiebreak, and that is what is asserted — an in-memory
    // store stamps `SystemTime::now()` per write, so three writes can land
    // in the same nanosecond and asserting a fixed order would flake.
    //
    // Teeth: the notes are written in ascending path order, so newest-first
    // is path-DESCENDING whenever the clock separates them. A sort that
    // used the path (or left the index order) fails then, and matches only
    // in the degenerate all-tied case.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate\n";
    let ev = |n: u32| {
        format!(
            "---\ntype: evidence\ncreated: 2026-05-0{n}\nsource: Note {n}\nportfolio: demo\norigin: \"[[projects/surrogate]]\"\n---\n\n# Note {n}\n"
        )
    };
    let (e1, e2, e3) = (ev(1), ev(2), ev(3));
    let (vault, _store) = vault_with(&[
        ("projects/surrogate.md", project),
        ("portfolios/demo/ev-1.md", &e1),
        ("portfolios/demo/ev-2.md", &e2),
        ("portfolios/demo/ev-3.md", &e3),
    ]);

    let bl = vault.project_backlinks("surrogate").unwrap();
    assert_eq!(bl.evidence.len(), 3, "{bl:?}");

    let mut expected = bl.evidence.clone();
    expected.sort_by(|a, b| {
        b.modified_ns
            .cmp(&a.modified_ns)
            .then_with(|| a.path.as_path().cmp(b.path.as_path()))
    });
    assert_eq!(bl.evidence, expected, "newest first, path as tiebreak");
}

// ---------------------------------------------------------------------
// current_focus (#442) — what you are in the middle of, read back from
// the day's own log rather than held as parallel state. Starting and
// completing an action already write it; this only reads.
// ---------------------------------------------------------------------

/// A daily note whose `## Logs` holds `lines` verbatim.
fn daily_with(date: NaiveDate, lines: &[&str]) -> String {
    let body = lines
        .iter()
        .map(|l| format!("- {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{body}\n")
}

fn focus_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 13).unwrap()
}

fn focus_vault(lines: &[&str]) -> Vault {
    let (vault, _store) = vault_with(&[(
        "journal/2026/daily/2026-07-13.md",
        &daily_with(focus_day(), lines),
    )]);
    vault
}

#[test]
fn current_focus_is_none_without_a_daily_note() {
    let (vault, _store) = vault_with(&[]);

    assert_eq!(vault.current_focus(focus_day()).unwrap(), None);
}

#[test]
fn current_focus_finds_a_started_action() {
    let vault = focus_vault(&["**09:30**: started [[alpha]] \u{2014} Draft the methods section"]);

    let focus = vault.current_focus(focus_day()).unwrap().expect("a focus");

    assert_eq!(focus.project, "alpha");
    assert_eq!(focus.action, "Draft the methods section");
    assert_eq!(focus.started, NaiveTime::from_hms_opt(9, 30, 0).unwrap());
}

#[test]
fn a_completed_action_is_no_longer_the_focus() {
    let vault = focus_vault(&[
        "**09:30**: started [[alpha]] \u{2014} Draft the methods section",
        "**11:00**: action done on [[alpha]] \u{2014} Draft the methods section",
    ]);

    assert_eq!(vault.current_focus(focus_day()).unwrap(), None);
}

#[test]
fn the_most_recent_open_start_wins() {
    // A day interleaves several: pick something up, put it down, pick up
    // something else. The one still standing is what you are on.
    let vault = focus_vault(&[
        "**09:30**: started [[alpha]] \u{2014} Draft the methods section",
        "**10:15**: started [[beta]] \u{2014} Chase the venue",
        "**11:00**: action done on [[beta]] \u{2014} Chase the venue",
        "**11:30**: started [[gamma]] \u{2014} Review the replies",
    ]);

    let focus = vault.current_focus(focus_day()).unwrap().expect("a focus");

    assert_eq!(focus.project, "gamma");
    assert_eq!(focus.action, "Review the replies");
}

#[test]
fn completing_one_action_leaves_an_earlier_start_standing() {
    // The completion clears its own start, not simply the latest.
    let vault = focus_vault(&[
        "**09:30**: started [[alpha]] \u{2014} Draft the methods section",
        "**10:15**: started [[beta]] \u{2014} Chase the venue",
        "**11:00**: action done on [[beta]] \u{2014} Chase the venue",
    ]);

    let focus = vault.current_focus(focus_day()).unwrap().expect("a focus");

    assert_eq!(focus.project, "alpha");
}

#[test]
fn prose_that_merely_mentions_starting_is_not_a_focus() {
    // A hand-written log line is ordinary; only the shape the writers
    // produce counts, or the band would report someone's sentence.
    let vault = focus_vault(&[
        "**09:30**: started thinking about the venue problem",
        "**10:00**: started [[alpha]] without a dash",
    ]);

    assert_eq!(vault.current_focus(focus_day()).unwrap(), None);
}

#[test]
fn the_energy_suffix_is_preserved_and_still_matches_on_completion() {
    // `start` logs the bullet verbatim, energy tag and all, and `complete`
    // logs the same text — so the two must still pair up.
    let vault = focus_vault(&[
        "**09:30**: started [[alpha]] \u{2014} Draft the methods section (deep)",
        "**11:00**: action done on [[alpha]] \u{2014} Draft the methods section (deep)",
    ]);

    assert_eq!(vault.current_focus(focus_day()).unwrap(), None);
}
