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
        out.contains("(project: alpha)"),
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
    assert!(
        out.contains("alpha — next: Draft the methods section (deep)"),
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
