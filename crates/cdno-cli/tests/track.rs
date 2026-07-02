//! Tests for `cdno track`'s newcomer-hint gate (#282, #287). The "did this
//! entry use the generic template" decision now comes from the domain's
//! resolved `TemplateSource` (see the domain `template_source_*` tests); this
//! seam is the remaining UX gate — the nudge is silenced once the vault has any
//! custom `tracking-<slug>.md` or `tracking.md`, so only a true newcomer sees it.

use std::fs;

use cdno_cli::commands::{init, track};
use tempfile::tempdir;

#[test]
fn newcomer_hint_fires_without_a_custom_template() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let hint = track::newcomer_template_hint(dir.path(), "gym").expect("hint on generic fallback");
    assert!(hint.contains("tracking-gym.md"), "hint: {hint}");
    assert!(
        hint.contains("examples/templates/tracking/"),
        "hint: {hint}"
    );
}

#[test]
fn newcomer_hint_silenced_once_any_tracking_template_exists() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::write(
        dir.path().join(".cuaderno/templates/tracking-gym.md"),
        "---\ntype: tracking\n---\n# Gym\n",
    )
    .unwrap();

    // A user who has authored any tracking template knows the mechanism, so the
    // nudge goes quiet — for that activity AND for others (no per-activity nag).
    assert!(track::newcomer_template_hint(dir.path(), "gym").is_none());
    assert!(track::newcomer_template_hint(dir.path(), "swim").is_none());
}

#[test]
fn newcomer_hint_silenced_by_a_base_override() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::write(
        dir.path().join(".cuaderno/templates/tracking.md"),
        "---\ntype: tracking\n---\n# X\n",
    )
    .unwrap();

    assert!(track::newcomer_template_hint(dir.path(), "deadlift").is_none());
}

#[test]
fn newcomer_hint_uses_the_slugified_activity_in_the_filename() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    // Multi-word activity → slug in the hint matches the file the resolver
    // looks for (both use `cdno_domain::slugify`).
    let slug = cdno_domain::slugify("Weight Training");
    let hint = track::newcomer_template_hint(dir.path(), &slug).expect("hint");
    assert!(hint.contains("tracking-weight-training.md"), "hint: {hint}");
}
