//! The Strategic / Monthly view seam against the Memory doubles (M9,
//! #57): the composed bundle view-model and the backend-side habit
//! sparkline bucketing, no Tauri runtime involved.

use std::sync::Arc;

use cdno_core::config::{VaultConfig, VaultMeta};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::StewardshipVariant;
use cdno_tauri::commands::strategic::{entries_per_week, get_strategic_bundle_impl};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with_config(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new");
    vault
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    vault_with_config(notes, VaultConfig::default())
}

/// A vault config capping active projects at `max` — the cheapest way
/// to prove `max_active` rides the bundle from config, not a hardcode.
fn config_capped_at(max: u8) -> VaultConfig {
    VaultConfig {
        vault: VaultMeta {
            name: "test-vault".to_owned(),
            max_active_projects: max,
        },
        ..VaultConfig::default()
    }
}

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n";
const BETA_PARKED: &str = "---\ntype: project\ncontext: personal\nstatus: parked\ncreated: 2026-03-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n";
const QUESTION: &str = "---\ntype: question\ndomain: research\nstatus: active\ncreated: 2026-06-01\nupdated: 2026-06-15\n---\n\n# How faithful is the surrogate?\n";
const PORTFOLIO_INDEX: &str = "---\ntype: portfolio\nquestion: How does the surrogate behave?\ncreated: 2026-06-01\n---\n\n# How does the surrogate behave?\n\n## Evidence\n";
const PORTFOLIO_EVIDENCE: &str = "---\ntype: evidence\ncreated: 2026-07-01\nsource: Smith 2024\nportfolio: surrogate\norigin: \"[[projects/alpha]]\"\n---\n\n# Smith 2024\n\nBounded.\n";
const HEALTH_INDEX: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Current Status\nConsistent.\n";
// Two gym entries, dated so that (as of Wed 2026-07-08) one falls in the
// previous ISO week and one in the current one — the sparkline test
// pins them to their week buckets.
const GYM_PREV_WEEK: &str = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-07-01\nduration_min: 60\nroutine: null\n---\n\n# Gym\n\n| Sets | Reps |\n|------|------|\n| 3 | 5 |\n";
const GYM_THIS_WEEK: &str = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-07-07\nduration_min: 55\nroutine: null\n---\n\n# Gym\n\n| Sets | Reps |\n|------|------|\n| 4 | 6 |\n";
const FINANCES: &str =
    "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n\n## Current Status\nSteady.\n";
const COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-07-20\ncreated: 2026-06-01\ncompleted: null\ncontext: work\n---\n\n# Submit the grant report\n";

/// The whole bundle composes from every source: questions, portfolios,
/// the filled + parked project slots, the configured cap, the
/// stewardship rows (with their sparklines), and the commitments window.
#[test]
fn bundle_composes_every_strategic_source() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("projects/_parked/beta.md", BETA_PARKED),
        ("questions/research/surrogate-fidelity.md", QUESTION),
        ("portfolios/surrogate/_index.md", PORTFOLIO_INDEX),
        (
            "portfolios/surrogate/2026-07-01-smith-2024.md",
            PORTFOLIO_EVIDENCE,
        ),
        ("stewardships/health/_index.md", HEALTH_INDEX),
        (
            "stewardships/health/tracking/2026-07-01-gym.md",
            GYM_PREV_WEEK,
        ),
        (
            "stewardships/health/tracking/2026-07-07-gym.md",
            GYM_THIS_WEEK,
        ),
        ("commitments/grant-report.md", COMMITMENT),
    ]);

    let today = ymd(2026, 7, 8);
    let bundle = get_strategic_bundle_impl(&vault, today).unwrap();

    assert_eq!(bundle.today, today);
    // Default config → the design's 5-slot allocator.
    assert_eq!(bundle.max_active, 5);

    // One active slot, one parked shelf entry, each with its context.
    assert_eq!(bundle.active.len(), 1);
    assert_eq!(bundle.active[0].slug, "alpha");
    assert_eq!(bundle.active[0].context, cdno_domain::Context::Work);
    assert_eq!(bundle.parked.len(), 1);
    assert_eq!(bundle.parked[0].slug, "beta");

    // The active research question is carried with its domain for the grid.
    assert_eq!(bundle.questions.len(), 1);
    assert_eq!(bundle.questions[0].domain.as_str(), "research");

    // The portfolio-health row rides through with its evidence count.
    let surrogate = bundle
        .portfolios
        .iter()
        .find(|p| p.slug == "surrogate")
        .expect("the surrogate portfolio is listed");
    assert_eq!(surrogate.evidence_count, 1);

    // The commitment falls inside the six-week window.
    assert!(
        bundle
            .commitments
            .iter()
            .any(|c| c.title == "Submit the grant report"),
        "the commitment is aggregated: {:?}",
        bundle.commitments,
    );

    // The expanded stewardship row carries a 12-week sparkline with the
    // two entries in their respective week buckets (prev week + this
    // week are the last two indices).
    let health = bundle
        .stewardships
        .iter()
        .find(|s| s.summary.slug == "health")
        .expect("the health stewardship is listed");
    assert_eq!(health.summary.variant, StewardshipVariant::Expanded);
    assert_eq!(health.sparkline.len(), 12);
    assert_eq!(
        health.sparkline[10], 1,
        "prev-week bucket: {:?}",
        health.sparkline
    );
    assert_eq!(
        health.sparkline[11], 1,
        "this-week bucket: {:?}",
        health.sparkline
    );
    assert_eq!(health.sparkline.iter().sum::<u32>(), 2);
}

/// A flat stewardship draws no spark — an empty vec, not a row of zeros,
/// so the frontend can omit the spark entirely (the detail view's
/// "charts only when there's data" rule).
#[test]
fn flat_stewardship_has_an_empty_sparkline() {
    let vault = vault_with(&[("stewardships/finances.md", FINANCES)]);
    let bundle = get_strategic_bundle_impl(&vault, ymd(2026, 7, 8)).unwrap();

    let finances = bundle
        .stewardships
        .iter()
        .find(|s| s.summary.slug == "finances")
        .expect("the flat stewardship is listed");
    assert_eq!(finances.summary.variant, StewardshipVariant::Flat);
    assert!(finances.sparkline.is_empty());
}

/// `max_active` is the vault's configured cap, not a hardcoded 5 — a
/// vault that lowered the cap lays out that many slots.
#[test]
fn max_active_reflects_the_configured_cap() {
    let vault = vault_with_config(&[("projects/alpha.md", ALPHA)], config_capped_at(3));
    let bundle = get_strategic_bundle_impl(&vault, ymd(2026, 7, 8)).unwrap();
    assert_eq!(bundle.max_active, 3);
}

/// The bucketing keys each date to its ISO week: oldest in-window week
/// at index 0, the current week last. Out-of-window and future dates are
/// dropped, never clamped into an edge bucket.
#[test]
fn entries_per_week_buckets_by_iso_week() {
    let today = ymd(2026, 7, 8); // Wed; ISO Monday 2026-07-06.
    let dates = [
        ymd(2026, 7, 7),  // this week → last bucket
        ymd(2026, 7, 6),  // this week too (same Monday)
        ymd(2026, 7, 1),  // previous week
        ymd(2026, 4, 20), // exactly 11 weeks back → first bucket
        ymd(2026, 4, 19), // one day older → out of window, dropped
        ymd(2026, 7, 20), // future → dropped, never inflates this week
    ];

    let counts = entries_per_week(&dates, today, 12);

    assert_eq!(counts.len(), 12);
    assert_eq!(
        counts[11], 2,
        "both this-week dates land in the last bucket"
    );
    assert_eq!(counts[10], 1, "the previous week");
    assert_eq!(counts[0], 1, "the 11-weeks-back date opens the window");
    assert_eq!(
        counts.iter().sum::<u32>(),
        4,
        "the out-of-window and future dates are dropped: {counts:?}",
    );
}
