//! Unit tests for `Vault::orientation_context` — the composed daily
//! orient snapshot. `MemoryVaultStore` / `MemoryIndex` keep it fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::ProjectStatus;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

/// Active project with a state snippet, a top action, and a hard
/// milestone one day out.
const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nMaking progress on the core loop.\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n\n## Milestones\n- [ ] Submit abstract — hard: 2026-05-27\n";

const PARKED_BETA: &str = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n\n## Next Actions\n- [ ] Resume someday (light)\n";

fn commitment(due: &str) -> String {
    format!(
        "---\ntype: commitment\nstatus: active\ndue: {due}\ncreated: 2026-05-01\ncompleted: null\ncontext: work\n---\n\n# Pay invoice\n"
    )
}

#[test]
fn orientation_context_composes_active_projects_and_commitments() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("projects/_parked/beta.md", PARKED_BETA),
        ("commitments/pay-invoice.md", &commitment("2026-05-27")),
    ]);

    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();

    // Only the active project, summarised with its state + top action.
    assert_eq!(ctx.projects.len(), 1, "parked beta is excluded");
    assert_eq!(ctx.projects[0].slug, "alpha");
    assert_eq!(ctx.projects[0].status, ProjectStatus::Active);
    assert!(!ctx.projects[0].state_snippet.is_empty());
    let top = ctx.projects[0].top_action.as_ref().expect("a top action");
    assert!(top.text.contains("Draft the methods section"));

    // Both the milestone (2026-05-27) and the commitment (2026-05-27)
    // fall in the 48h window.
    let titles: Vec<&str> = ctx.commitments.iter().map(|c| c.title.as_str()).collect();
    assert!(titles.contains(&"Submit abstract"), "milestone: {titles:?}");
    assert!(titles.contains(&"Pay invoice"), "commitment: {titles:?}");

    // Lapsed habits are empty until the stewardship layer (Phase 3).
    assert!(ctx.lapsed_habits.is_empty());
}

#[test]
fn orientation_context_commitments_honour_the_48h_window() {
    let vault = vault_with(&[
        // 1 day out — inside the 48h window.
        ("commitments/soon.md", &commitment("2026-05-27")),
        // 4 days out — beyond it.
        ("commitments/later.md", &commitment("2026-05-30")),
    ]);

    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();
    assert_eq!(
        ctx.commitments.len(),
        1,
        "only the near commitment: {:?}",
        ctx.commitments
    );
    assert_eq!(ctx.commitments[0].date, ymd(2026, 5, 27));
}

#[test]
fn orientation_context_is_empty_on_a_fresh_vault() {
    let vault = vault_with(&[]);
    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();
    assert!(ctx.commitments.is_empty());
    assert!(ctx.projects.is_empty());
    assert!(ctx.lapsed_habits.is_empty());
}

// ---------------------------------------------------------------------
// lapsed_habits
// ---------------------------------------------------------------------

const HEALTH: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Current Status\nHolding steady.\n\n## Periodic Commitments\n- Dental check-up \u{2014} every 6 months \u{2014} next: 2099-04-01\n\n## Active Habits\n- Resistance training 3x/week \u{2014} on track\n- Swimming 1x/week \u{2014} lapsed since March\n- Sleep before midnight \u{2014} inconsistent\n";

const FINANCES: &str =
    "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n\n## Current Status\nFine.\n";

#[test]
fn orientation_surfaces_declared_lapsed_habits() {
    let vault = vault_with(&[
        ("stewardships/health.md", HEALTH),
        ("stewardships/finances.md", FINANCES),
    ]);

    let ctx = vault.orientation_context(ymd(2026, 5, 26)).unwrap();

    assert_eq!(ctx.lapsed_habits.len(), 1);
    assert_eq!(ctx.lapsed_habits[0].stewardship, "health");
    assert_eq!(
        ctx.lapsed_habits[0].detail,
        "Swimming 1x/week \u{2014} lapsed since March"
    );
}

#[test]
fn lapsed_habits_matches_case_insensitively_and_only_status_segments() {
    let steward = "---\ntype: stewardship\ncontext: personal\n---\n\n# Hobbies\n\n## Active Habits\n- Piano practice \u{2014} Lapsed (2w)\n- lapsed-thing cleanup \u{2014} on track\n- Journalling\n";
    let vault = vault_with(&[("stewardships/hobbies.md", steward)]);

    let lapsed = vault.lapsed_habits().unwrap();

    // "Lapsed (2w)" matches despite the capital; the habit merely
    // *named* lapsed-thing does not; the status-less line is skipped.
    assert_eq!(lapsed.len(), 1);
    assert_eq!(lapsed[0].detail, "Piano practice \u{2014} Lapsed (2w)");
}

#[test]
fn lapsed_habits_sorted_and_expanded_variant_included() {
    let a = "---\ntype: stewardship\ncontext: personal\n---\n\n# Zeta\n\n## Active Habits\n- Stretching \u{2014} lapsed since May\n";
    let b = "---\ntype: stewardship\ncontext: personal\n---\n\n# Aqua\n\n## Active Habits\n- Laps \u{2014} lapsed since April\n";
    let vault = vault_with(&[
        ("stewardships/zeta.md", a),
        ("stewardships/aqua/_index.md", b),
    ]);

    let lapsed = vault.lapsed_habits().unwrap();

    assert_eq!(lapsed.len(), 2);
    // Sorted by stewardship slug; the expanded variant's slug is its
    // folder name.
    assert_eq!(lapsed[0].stewardship, "aqua");
    assert_eq!(lapsed[1].stewardship, "zeta");
}

// Tests for focus and last_paused (RFC 0005 T9)

#[test]
fn orientation_carries_the_focus_and_each_projects_last_pause() {
    let today = ymd(2026, 5, 26);
    let daily = "---\ntype: daily\ndate: 2026-05-26\n---\n\n# Daily\n\n## Logs\n- **14:05**: started [[alpha]] — Draft the methods section (deep)\n- **14:15**: action paused on [[beta]] — Review the outline (medium)\n  next: Polish section 2\n- **14:30**: started [[gamma]] — Refresh data (light)\n";

    let gamma = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Gamma\n\n## Current State\nActive.\n\n## Next Actions\n- [ ] First task (light)\n";

    let beta_active = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nOn track.\n\n## Next Actions\n- [ ] Review the outline (medium)\n";

    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("projects/beta.md", beta_active),
        ("projects/gamma.md", gamma),
        ("journal/2026/daily/2026-05-26.md", daily),
    ]);

    let ctx = vault.orientation_context(today).unwrap();

    // Focus should be the last started action of the day: gamma at 14:30.
    assert!(ctx.focus.is_some(), "should have a focus");
    let focus = ctx.focus.unwrap();
    assert_eq!(focus.project, "gamma", "focus project");
    assert_eq!(focus.action, "Refresh data (light)", "focus action");

    // Three active projects in the summary (alpha, beta, gamma).
    assert_eq!(ctx.projects.len(), 3, "three active projects");

    // Alpha: has a started entry at 14:05 (not the current focus), no last_paused.
    assert_eq!(ctx.projects[0].slug, "alpha");
    assert!(ctx.projects[0].last_paused.is_none(), "alpha has no pause");

    // Beta: has a paused entry at 14:15 with a next continuation, no focus.
    assert_eq!(ctx.projects[1].slug, "beta");
    let beta_pause = ctx.projects[1]
        .last_paused
        .as_ref()
        .expect("beta has last_paused");
    assert_eq!(beta_pause.project, "beta");
    assert_eq!(beta_pause.action, "Review the outline (medium)");
    assert_eq!(beta_pause.next, Some("Polish section 2".to_owned()));

    // Gamma: has a started entry at 14:30 (the current focus), no last_paused.
    assert_eq!(ctx.projects[2].slug, "gamma");
    assert!(ctx.projects[2].last_paused.is_none(), "gamma has no pause");
}
