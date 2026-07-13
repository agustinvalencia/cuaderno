//! Parsing of `cuaderno://note/<path>` deep links. The parser only extracts
//! the path; confinement (rejecting `..`/absolute) is the reader's `VaultPath`
//! guard downstream, so a traversing path parses here but can't read out.

use cdno_tauri::deeplink::note_path_from_deeplink;

#[test]
fn extracts_a_multi_segment_vault_path() {
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/journal/2026/daily/2026-07-13.md"),
        Some("journal/2026/daily/2026-07-13.md".to_owned())
    );
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/portfolios/x/_index.md"),
        Some("portfolios/x/_index.md".to_owned())
    );
}

#[test]
fn drops_a_query_or_fragment_and_trailing_slash() {
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/projects/alpha.md?foo=bar"),
        Some("projects/alpha.md".to_owned())
    );
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/projects/alpha.md#frag"),
        Some("projects/alpha.md".to_owned())
    );
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/projects/alpha.md/"),
        Some("projects/alpha.md".to_owned())
    );
}

#[test]
fn rejects_other_schemes_hosts_and_empty_paths() {
    assert_eq!(note_path_from_deeplink("cuaderno://open/foo.md"), None);
    assert_eq!(note_path_from_deeplink("https://note/foo.md"), None);
    assert_eq!(note_path_from_deeplink("cuaderno://note/"), None);
    assert_eq!(note_path_from_deeplink("cuaderno://note"), None);
    assert_eq!(note_path_from_deeplink("not a url"), None);
}

#[test]
fn a_traversing_path_parses_here_but_stays_for_the_vaultpath_guard() {
    // The parser is not the confinement boundary; it faithfully returns the
    // path, and the reader's VaultPath::new rejects the `..` downstream.
    assert_eq!(
        note_path_from_deeplink("cuaderno://note/../../etc/passwd"),
        Some("../../etc/passwd".to_owned())
    );
}
