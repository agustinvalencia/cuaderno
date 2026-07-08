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
