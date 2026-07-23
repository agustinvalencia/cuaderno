//! The path → area classifier the invalidation map keys on.

use cdno_core::path::VaultPath;
use cdno_tauri::events::{VaultArea, classify};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

#[test]
fn classify_maps_top_level_directories() {
    let cases = [
        ("projects/alpha.md", VaultArea::Projects),
        ("projects/_parked/beta.md", VaultArea::Projects),
        ("actions/draft-methods.md", VaultArea::Actions),
        ("commitments/pay-invoice.md", VaultArea::Commitments),
        ("portfolios/turbulence/_index.md", VaultArea::Portfolios),
        (
            "stewardships/health/tracking/2026-04-10-gym.md",
            VaultArea::Stewardships,
        ),
        ("questions/research/sparse.md", VaultArea::Questions),
        ("inbox/2026-07-07-thought.md", VaultArea::Inbox),
    ];
    for (path, want) in cases {
        assert_eq!(classify(&vp(path)), Some(want), "{path}");
    }
}

#[test]
fn classify_splits_journal_by_daily_weekly_and_monthly() {
    assert_eq!(
        classify(&vp("journal/2026/daily/2026-07-07.md")),
        Some(VaultArea::Daily)
    );
    assert_eq!(
        classify(&vp("journal/2026/weekly/2026-W27.md")),
        Some(VaultArea::Weekly)
    );
    // The monthly note (#228) must classify too, so a calendar edit or an
    // external nvim edit to it refreshes the calendar's month panel
    // (#340).
    assert_eq!(
        classify(&vp("journal/2026/monthly/2026-07.md")),
        Some(VaultArea::Monthly)
    );
    assert_eq!(classify(&vp("journal/2026/notes.md")), None);
}

#[test]
fn classify_cuaderno_config_and_templates_are_config() {
    assert_eq!(
        classify(&vp(".cuaderno/config.toml")),
        Some(VaultArea::Config)
    );
    // Template edits change the log form's fields, so they refresh the
    // config area alongside config.toml.
    assert_eq!(
        classify(&vp(".cuaderno/templates/daily.md")),
        Some(VaultArea::Config)
    );
    assert_eq!(
        classify(&vp(".cuaderno/templates/tracking-gym.md")),
        Some(VaultArea::Config)
    );
    // But the index db and other non-markdown churn stays invisible.
    assert_eq!(classify(&vp(".cuaderno/index.db")), None);
}

#[test]
fn classify_unknown_paths_are_none() {
    assert_eq!(classify(&vp("README.md")), None);
    assert_eq!(classify(&vp("assets/photo.png")), None);
}

// ---------------------------------------------------------------------
// Index-exclusion notice (#440). A file absent from the index is absent
// from search, lint and backlinks too, so an over-broad `ignore` glob has
// to be visible. The threshold is proportional with a floor: both halves
// carry weight, and getting either wrong makes the notice useless — too
// eager and it is dismissed reflexively, too shy and it never fires on
// the case it exists for.
// ---------------------------------------------------------------------

use cdno_tauri::events::{IndexExclusions, glob_looks_over_broad};

#[test]
fn a_lone_housekeeping_exclusion_is_not_over_broad() {
    // `CLAUDE.md` in a real vault: 1 of 168, deliberate, and it must stay
    // silent or the notice becomes noise on every launch.
    assert!(!glob_looks_over_broad(1, 168));
}

#[test]
fn a_glob_swallowing_a_quarter_of_the_vault_is_over_broad() {
    // The shape that motivated #440: `portfolios/*/**` matched at every
    // depth below and evicted 54 of 221 files.
    assert!(glob_looks_over_broad(54, 221));
}

#[test]
fn a_tiny_vault_does_not_trip_the_percentage_on_one_file() {
    // 1 of 3 is 33%, well over the share — the floor is what stops a
    // scratch vault crying wolf.
    assert!(!glob_looks_over_broad(1, 3));
    assert!(!glob_looks_over_broad(4, 8));
}

#[test]
fn the_floor_and_the_share_must_both_be_met() {
    // At the floor but a small share: 5 of 500 is 1%.
    assert!(!glob_looks_over_broad(5, 500));
    // At the floor and over the share: 5 of 40 is 12.5%.
    assert!(glob_looks_over_broad(5, 40));
}

#[test]
fn excluding_nothing_is_never_over_broad() {
    assert!(!glob_looks_over_broad(0, 0));
    assert!(!glob_looks_over_broad(0, 500));
}

#[test]
fn artefacts_never_make_the_exclusions_look_over_broad() {
    // Artefacts are excluded by design, not by configuration, so however
    // many there are they must not raise a "check your config" notice.
    let report = cdno_core::reconcile::ReconciliationReport {
        scanned: 10,
        ignored: 0,
        artefacts: 90,
        ..Default::default()
    };
    let exclusions = IndexExclusions::from_report(&report, 0);

    assert_eq!(exclusions.artefacts, 90);
    assert!(!exclusions.ignore_looks_over_broad);
}

#[test]
fn the_share_is_measured_against_every_walked_markdown_file() {
    // The denominator is indexed + ignored + artefacts, not just the
    // indexed count — otherwise a vault that is mostly artefacts would
    // make a modest ignore list look enormous.
    let report = cdno_core::reconcile::ReconciliationReport {
        scanned: 50,
        ignored: 6,
        artefacts: 44,
        ..Default::default()
    };
    let exclusions = IndexExclusions::from_report(&report, 0);

    assert_eq!(exclusions.indexed, 50);
    assert_eq!(exclusions.ignored, 6);
    // 6 of 100 is 6% — under the share, so no notice.
    assert!(!exclusions.ignore_looks_over_broad);
}
