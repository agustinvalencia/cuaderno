//! Tests for `cdno track`'s generic-template hint seam (#282). The hint fires
//! only when the note used the built-in generic template — i.e. the vault has
//! no custom `tracking-<slug>.md` or `tracking.md` override.

use std::fs;

use cdno_cli::commands::{init, track};
use tempfile::tempdir;

#[test]
fn generic_template_hint_fires_without_a_custom_template() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let hint = track::generic_template_hint(dir.path(), "gym").expect("hint on generic fallback");
    assert!(hint.contains("tracking-gym.md"), "hint: {hint}");
    assert!(
        hint.contains("examples/templates/tracking/"),
        "hint: {hint}"
    );
}

#[test]
fn generic_template_hint_suppressed_by_a_matching_variant_template() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::write(
        dir.path().join(".cuaderno/templates/tracking-gym.md"),
        "---\ntype: tracking\n---\n# Gym\n",
    )
    .unwrap();

    assert!(track::generic_template_hint(dir.path(), "gym").is_none());
    // A different activity with no template of its own still gets the hint.
    assert!(track::generic_template_hint(dir.path(), "swim").is_some());
}

#[test]
fn generic_template_hint_suppressed_by_a_base_override() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();
    fs::write(
        dir.path().join(".cuaderno/templates/tracking.md"),
        "---\ntype: tracking\n---\n# X\n",
    )
    .unwrap();

    // A base `tracking.md` override applies to every activity, so no activity
    // falls back to the built-in generic — no hint.
    assert!(track::generic_template_hint(dir.path(), "deadlift").is_none());
}
