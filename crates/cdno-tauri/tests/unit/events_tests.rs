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
fn classify_splits_journal_by_daily_and_weekly() {
    assert_eq!(
        classify(&vp("journal/2026/daily/2026-07-07.md")),
        Some(VaultArea::Daily)
    );
    assert_eq!(
        classify(&vp("journal/2026/weekly/2026-W27.md")),
        Some(VaultArea::Weekly)
    );
    assert_eq!(classify(&vp("journal/2026/notes.md")), None);
}

#[test]
fn classify_cuaderno_dir_is_config_only() {
    assert_eq!(
        classify(&vp(".cuaderno/config.toml")),
        Some(VaultArea::Config)
    );
    assert_eq!(classify(&vp(".cuaderno/index.db")), None);
    assert_eq!(classify(&vp(".cuaderno/templates/daily.md")), None);
}

#[test]
fn classify_unknown_paths_are_none() {
    assert_eq!(classify(&vp("README.md")), None);
    assert_eq!(classify(&vp("assets/photo.png")), None);
}
