//! Startup vault-path resolution: ordering and the stale-stored rule.
//!
//! `resolve` takes the vault check as a closure, so these exercise the
//! branching without a real vault or a dialog.

use std::path::{Path, PathBuf};

use cdno_tauri::vault_locator::{Resolution, read_setting, resolve, write_setting};

/// Write a `vault.json` into `dir` naming `vault_path`.
fn seed_setting(dir: &Path, vault_path: &str) {
    write_setting(dir, Path::new(vault_path)).unwrap();
}

#[test]
fn env_override_wins_before_any_stored_lookup() {
    let dir = tempfile::tempdir().unwrap();
    // A stored path exists, but the env override must take precedence and
    // is returned unvalidated — the validator would reject it here.
    seed_setting(dir.path(), "/stored/vault");

    let resolution = resolve(
        Some(PathBuf::from("/env/vault")),
        dir.path(),
        |_| false, // even a rejecting validator must not be consulted
    );

    assert_eq!(resolution, Resolution::Env(PathBuf::from("/env/vault")));
}

#[test]
fn stored_used_when_valid() {
    let dir = tempfile::tempdir().unwrap();
    seed_setting(dir.path(), "/stored/vault");

    let resolution = resolve(None, dir.path(), |_| true);

    assert_eq!(
        resolution,
        Resolution::Stored(PathBuf::from("/stored/vault"))
    );
}

#[test]
fn invalid_stored_falls_through_to_picker() {
    let dir = tempfile::tempdir().unwrap();
    // The path was persisted once but no longer opens (moved/deleted).
    seed_setting(dir.path(), "/stored/vault");

    let resolution = resolve(None, dir.path(), |_| false);

    assert_eq!(resolution, Resolution::NeedsPicker);
}

#[test]
fn no_env_and_no_setting_needs_picker() {
    let dir = tempfile::tempdir().unwrap();
    // Empty config dir: nothing to read.
    let resolution = resolve(None, dir.path(), |_| true);

    assert_eq!(resolution, Resolution::NeedsPicker);
}

#[test]
fn write_then_read_round_trips_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let vault = PathBuf::from("/some/where/notebook");

    write_setting(dir.path(), &vault).unwrap();

    assert_eq!(read_setting(dir.path()), Some(vault));
}

#[test]
fn write_setting_creates_a_missing_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    // A nested dir that does not exist yet (first launch).
    let config_dir = dir.path().join("cuaderno").join("nested");
    let vault = PathBuf::from("/some/where/notebook");

    write_setting(&config_dir, &vault).unwrap();

    assert_eq!(read_setting(&config_dir), Some(vault));
}

#[test]
fn read_setting_returns_none_on_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("vault.json"), "not json {{{").unwrap();

    assert_eq!(read_setting(dir.path()), None);
}
