//! Confinement of the note-asset path resolver (`read_note_asset`, image
//! embedding). The security claim is that a markdown image `src` — resolved
//! relative to its note — can never read outside the vault: `resolve_asset_path`
//! itself never emits a `..` (excess `..` pops are bounded, so the result
//! stays vault-relative), and any residual `..`/absolute is refused by the
//! downstream `VaultPath::new` guard. These pin both halves.

use std::path::{Path, PathBuf};

use cdno_core::path::VaultPath;
use cdno_tauri::commands::notes::{mime_for, resolve_asset_path};

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn resolves_relative_to_the_notes_folder() {
    assert_eq!(
        resolve_asset_path("notes/a.md", "assets/fig.png"),
        Some(pb("notes/assets/fig.png"))
    );
}

#[test]
fn normalises_curdir_segments() {
    assert_eq!(
        resolve_asset_path("notes/a.md", "./assets/fig.png"),
        Some(pb("notes/assets/fig.png"))
    );
}

#[test]
fn parentdir_resolves_within_the_note_tree() {
    assert_eq!(
        resolve_asset_path("portfolios/x/note.md", "../y/fig.png"),
        Some(pb("portfolios/y/fig.png"))
    );
}

#[test]
fn excess_parentdir_cannot_escape_the_vault() {
    // Excess `..` pops are bounded (pop on an empty buf is a no-op), so the
    // result stays vault-relative — `<vault>/etc/passwd`, never `/etc/passwd`.
    let resolved = resolve_asset_path("a.md", "../../../../etc/passwd").unwrap();
    assert_eq!(resolved, pb("etc/passwd"));
    // It never carries a leading `..`, so the VaultPath guard accepts it.
    assert!(VaultPath::new(&resolved).is_ok());
}

#[test]
fn absolute_src_is_rejected_outright() {
    assert_eq!(resolve_asset_path("a.md", "/etc/passwd"), None);
}

#[test]
fn a_traversing_note_path_is_caught_by_the_vaultpath_guard() {
    // The resolver faithfully carries a `..`-prefixed note_path, but the
    // downstream `VaultPath::new` guard (applied to the result before any
    // read) rejects it — the second half of the confinement.
    let resolved = resolve_asset_path("../../etc/a.md", "x.png").unwrap();
    assert!(VaultPath::new(&resolved).is_err());
}

#[test]
fn mime_is_derived_from_the_extension() {
    assert_eq!(mime_for(Path::new("a/b.png")), "image/png");
    assert_eq!(mime_for(Path::new("a/b.JPG")), "image/jpeg");
    assert_eq!(mime_for(Path::new("a/b.svg")), "image/svg+xml");
    assert_eq!(mime_for(Path::new("a/b.bin")), "application/octet-stream");
    assert_eq!(mime_for(Path::new("noext")), "application/octet-stream");
}
