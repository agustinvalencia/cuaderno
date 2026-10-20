//! In-process tests for `cdno orient` / `cdno status`. They seed a
//! vault on disk, then assert on the rendered text returned by the
//! `build_*` seams (rather than capturing stdout from `run`).

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, orient, status};
use cdno_domain::frontmatter::EnergyLevel;
use chrono::NaiveDate;
use tempfile::tempdir;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 27).unwrap()
}

/// Active project with state, a deep top action, and a hard milestone
/// inside the 48h window.
const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nMaking progress on the core loop.\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n\n## Milestones\n- [ ] Submit abstract — hard: 2026-05-28\n";

/// Second active project whose top action is light (for the energy bias).
const BETA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nTicking over.\n\n## Next Actions\n- [ ] Tidy the references (light)\n";

const COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-05-28\ncreated: 2026-05-01\ncompleted: null\ncontext: work\n---\n\n# Pay invoice\n";

fn seed_alpha_vault(root: &Path) {
    init::run(root).expect("init");
    fs::write(root.join("projects/alpha.md"), ALPHA).unwrap();
    fs::write(root.join("commitments/pay-invoice.md"), COMMITMENT).unwrap();
}

#[test]
fn orient_renders_commitments_projects_and_suggestion() {
    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());

    let out = orient::build_orientation(dir.path(), today(), None).expect("orient builds");

    // Both commitment sources (milestone + standalone) in the window.
    assert!(
        out.contains("Submit abstract"),
        "milestone commitment:\n{out}"
    );
    assert!(
        out.contains("project: alpha"),
        "milestone source label:\n{out}"
    );
    assert!(out.contains("Pay invoice"), "standalone commitment:\n{out}");
    // Active project with its state and top action.
    assert!(
        out.contains("Draft the methods section (deep)"),
        "top action:\n{out}"
    );
    // Suggestion points at the only project with an open action.
    assert!(
        out.contains("Suggested start") && out.contains("alpha: Draft the methods section (deep)"),
        "suggestion:\n{out}"
    );
}

#[test]
fn orient_energy_flag_biases_the_suggestion() {
    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());
    fs::write(dir.path().join("projects/beta.md"), BETA).unwrap();

    // Asking for light work should suggest beta (its top action is
    // light), not alpha (deep) — even though alpha sorts first.
    let out =
        orient::build_orientation(dir.path(), today(), Some(EnergyLevel::Light)).expect("orient");
    assert!(
        out.contains("beta: Tidy the references (light)"),
        "energy-biased suggestion:\n{out}"
    );
}

#[test]
fn status_counts_and_lists_active_projects() {
    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());

    let out = status::build_status(dir.path(), today()).expect("status builds");
    // One project, two commitments (milestone + standalone).
    assert!(
        out.contains("1 active project,"),
        "singular project count:\n{out}"
    );
    assert!(
        out.contains("2 commitments due soon"),
        "commitment count:\n{out}"
    );
    // slug and its next-action now sit in adjacent table columns on the
    // same row rather than joined by an em-dash.
    assert!(
        out.lines()
            .any(|l| l.contains("alpha") && l.contains("next: Draft the methods section (deep)")),
        "project line:\n{out}"
    );
}

#[test]
fn orient_on_a_fresh_vault_shows_empty_placeholders() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let out = orient::build_orientation(dir.path(), today(), None).expect("orient");
    assert!(out.contains("(nothing due)"), "empty commitments:\n{out}");
    assert!(out.contains("(none — create one"), "empty projects:\n{out}");
    assert!(out.contains("nothing queued"), "empty suggestion:\n{out}");
}

#[test]
fn orient_renders_each_project_as_a_card_with_its_next_action() {
    // The existing assertions in this file are substrings that survived
    // the table-to-card conversion untouched — which also means they
    // would survive the `next:` line being blanked, or the gutter being
    // dropped. This pins the card shape itself.
    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());
    let out = orient::build_orientation(dir.path(), today(), None).expect("orientation");

    let header = out
        .lines()
        .map(str::trim_end)
        .find(|l| l.strip_prefix("▎ ").is_some_and(|r| r.starts_with("alpha")))
        .expect("a card header for alpha");
    assert!(header.ends_with("work"), "context is the badge: {header:?}");
    assert!(
        out.lines().any(|l| l.starts_with("▎ next: ")),
        "the top action reads as a body line:\n{out}"
    );
    // Commitments stay tabular — short aligned fields are what a table
    // is for, and turning them into cards was never the intent.
    let commitments = out
        .lines()
        .skip_while(|l| !l.starts_with("Commitments"))
        .take_while(|l| !l.starts_with("Active projects"));
    assert!(
        commitments.filter(|l| l.starts_with('▎')).count() == 0,
        "the commitments section must not grow a gutter:\n{out}"
    );
}

#[test]
fn orient_json_does_not_expose_focus_or_last_paused() {
    // RFC 0005 T9 adds focus and last_paused to domain types, but they are
    // internal and should not appear in CLI JSON output. Exposure in MCP
    // and CLI are decided later in RFC 0005. This test pins that focus and
    // last_paused are not in `cdno orient --json` output, even with a focus
    // open and a genuinely unconsumed pause logged.
    use assert_cmd::Command;

    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());
    // Add a paused action to the daily log. No later start/resume/done/drop
    // of the same text, so the pause remains genuinely unconsumed.
    let daily = format!("{}/journal/2026/daily/2026-05-27.md", dir.path().display());
    fs::write(
        &daily,
        "---\ndate: 2026-05-27\ntype: daily\n---\n\n# 2026-05-27\n\n## Logs\n\
         - **14:15**: action paused on [[alpha]] — Draft the methods section (deep)\n  \
         next: Polish abstract\n",
    )
    .unwrap();

    let out = Command::cargo_bin("cdno")
        .expect("cdno binary built")
        .args(["--vault"])
        .arg(dir.path())
        .args(["orient", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");

    // Check OrientationContext: no focus key.
    assert!(
        !json.get("focus").is_some(),
        "OrientationContext.focus must not be in JSON: {json}"
    );

    // Check each ProjectSummary: no last_paused key.
    if let Some(projects) = json.get("projects").and_then(|p| p.as_array()) {
        for proj in projects {
            assert!(
                !proj.get("last_paused").is_some(),
                "ProjectSummary.last_paused must not be in JSON: {proj}"
            );
        }
    }
}

#[test]
fn orient_json_projects_carry_the_cap_keys() {
    // RFC 0006 §8.2: each `projects` element gains `held_by`, `counts` and
    // `waiting_on_empty` beside the keys it already had, and nothing else.
    use assert_cmd::Command;

    let dir = tempdir().unwrap();
    seed_alpha_vault(dir.path());
    fs::write(
        dir.path().join("projects/harbour.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n\
         held_by: \"the harbour office\"\n---\n\n# Harbour\n\n## Current State\nWith the office.\n\n\
         ## Waiting On\n- Berth survey\n",
    )
    .unwrap();

    let out = Command::cargo_bin("cdno")
        .expect("cdno binary built")
        .args(["--vault"])
        .arg(dir.path())
        .args(["orient", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out).expect("stdout is valid JSON");
    let projects = json["projects"].as_array().expect("projects array");
    let by_slug = |slug: &str| {
        projects
            .iter()
            .find(|p| p["slug"] == slug)
            .unwrap_or_else(|| panic!("{slug} in {json}"))
    };

    let mut keys: Vec<&str> = by_slug("alpha")
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "context",
            "core_question",
            "counts",
            "held_by",
            "slug",
            "state_snippet",
            "status",
            "top_action",
            "waiting_on_empty",
        ]
    );

    let alpha = by_slug("alpha");
    assert_eq!(alpha["held_by"], serde_json::Value::Null);
    assert_eq!(alpha["counts"], true);
    assert_eq!(
        alpha["waiting_on_empty"], true,
        "no `## Waiting On` says nothing"
    );
    let harbour = by_slug("harbour");
    assert_eq!(harbour["held_by"], "the harbour office");
    assert_eq!(harbour["counts"], false);
    assert_eq!(harbour["waiting_on_empty"], false);
    assert_eq!(json["slots"]["active"], 2);
    assert_eq!(json["slots"]["counted"], 1);
}

#[test]
fn orient_renders_a_shared_question_as_one_line_with_its_fronts() {
    // Two maps of one inquiry read as one undertaking (RFC 0006 §7.3):
    // the question once, then one line carrying each front with its next
    // action. A map with no question keeps its own card.
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(
        dir.path().join("questions/research/surrogate.md"),
        "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# Does the surrogate generalise?\n",
    )
    .unwrap();
    let with_question = |body: &str, question: &str| {
        body.replacen(
            "created: 2026-04-01\n",
            &format!("created: 2026-04-01\ncore_question: \"[[{question}]]\"\n"),
            1,
        )
    };
    fs::write(
        dir.path().join("projects/alpha.md"),
        with_question(ALPHA, "questions/research/surrogate"),
    )
    .unwrap();
    fs::write(
        dir.path().join("projects/beta.md"),
        with_question(BETA, "surrogate"),
    )
    .unwrap();
    fs::write(
        dir.path().join("projects/gamma.md"),
        BETA.replace("# Beta", "# Gamma"),
    )
    .unwrap();

    let out = orient::build_orientation(dir.path(), today(), None).expect("orientation");
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();

    let header = lines
        .iter()
        .position(|l| l.starts_with("▎ questions/research/surrogate"))
        .unwrap_or_else(|| panic!("a card for the shared question:\n{out}"));
    assert!(
        lines[header].ends_with("question"),
        "the badge names the parent's kind: {:?}",
        lines[header]
    );
    assert_eq!(
        lines[header + 1],
        "▎ alpha: Draft the methods section (deep) · beta: Tidy the references (light)",
        "one line with its fronts:\n{out}"
    );
    // Apart from the fronts line, no line is led by a front's slug, so
    // neither front also has a card of its own.
    for front in ["alpha", "beta"] {
        assert!(
            !lines.iter().any(|l| l
                .strip_prefix("▎ ")
                .is_some_and(|r| r.starts_with(front) && !r.contains(" · "))),
            "{front} must not also have a card of its own:\n{out}"
        );
    }
    assert!(
        lines
            .iter()
            .any(|l| l.strip_prefix("▎ ").is_some_and(|r| r.starts_with("gamma"))),
        "a map with no question keeps its card:\n{out}"
    );
}

/// A held map (one handed to someone else, RFC 0006 §8.2) is not the
/// person's move, so the suggestion draws from the maps that count first,
/// even when the held one sorts ahead; the held map still shows, with its
/// holder named.
#[test]
fn the_suggestion_prefers_a_map_that_counts_over_a_held_one() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(
        dir.path().join("projects/alpha.md"),
        ALPHA.replace(
            "created: 2026-04-01\n",
            "created: 2026-04-01\nheld_by: \"Ana Ruiz\"\n",
        ),
    )
    .unwrap();
    fs::write(dir.path().join("projects/beta.md"), BETA).unwrap();

    let out = orient::build_orientation(dir.path(), today(), None).expect("orient");
    let suggested = out
        .split("Suggested start")
        .nth(1)
        .expect("a suggestion section");
    assert!(
        suggested.contains("beta: Tidy the references (light)"),
        "the counted map is suggested:\n{out}"
    );
    assert!(
        out.contains("held by: Ana Ruiz · does not count toward the cap"),
        "the held map names its holder:\n{out}"
    );

    // With only the held map left to move, it is suggested, holder named.
    fs::remove_file(dir.path().join("projects/beta.md")).unwrap();
    let out = orient::build_orientation(dir.path(), today(), None).expect("orient");
    let suggested = out.split("Suggested start").nth(1).unwrap();
    assert!(
        suggested.contains("alpha (held by Ana Ruiz): Draft the methods section (deep)"),
        "{out}"
    );
}

/// A held map inside a shared-question group names its holder on the
/// group's line of fronts, so the reader can tell whose move each front is.
#[test]
fn a_held_front_in_a_shared_question_names_its_holder() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(
        dir.path().join("questions/research/hull.md"),
        "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-05-01\nupdated: 2026-05-01\n---\n\n# Does the hull hold?\n",
    )
    .unwrap();
    let front = |body: &str, held: bool| {
        let holder = if held { "held_by: \"Ana Ruiz\"\n" } else { "" };
        body.replacen(
            "created: 2026-04-01\n",
            &format!(
                "created: 2026-04-01\ncore_question: \"[[questions/research/hull]]\"\n{holder}"
            ),
            1,
        )
    };
    fs::write(dir.path().join("projects/alpha.md"), front(ALPHA, true)).unwrap();
    fs::write(dir.path().join("projects/beta.md"), front(BETA, false)).unwrap();

    let out = orient::build_orientation(dir.path(), today(), None).expect("orient");
    let fronts = out
        .lines()
        .find(|l| l.contains("beta: "))
        .expect("the group's fronts line");
    assert!(
        fronts.contains("alpha (held by Ana Ruiz): Draft the methods section (deep)"),
        "{out}"
    );
    assert!(
        !fronts.contains("beta (held by"),
        "a front that is yours names no holder: {out}"
    );
}
