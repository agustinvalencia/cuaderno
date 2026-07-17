//! The external-link scheme allowlist: only `http`/`https`/`mailto` may be
//! handed to the OS opener from a note's link, so a note can't launch a
//! `file://` / `javascript:` / custom-scheme URL. The `open_external_url`
//! command itself needs a Tauri runtime (it calls the opener), so the pure
//! `is_openable_external_url` predicate is what carries the security contract
//! and is tested directly here.

use cdno_tauri::commands::capture::is_openable_external_url;

#[test]
fn allows_http_https_and_mailto() {
    assert!(is_openable_external_url("http://example.com"));
    assert!(is_openable_external_url(
        "https://example.com/path?q=1#frag"
    ));
    assert!(is_openable_external_url("mailto:someone@example.com"));
    // A hand-authored link may carry a mixed-case scheme or leading space.
    assert!(is_openable_external_url("HTTPS://Example.com"));
    assert!(is_openable_external_url("  https://example.com"));
}

#[test]
fn refuses_dangerous_and_non_web_schemes() {
    // The load-bearing refusals: a note must not launch these.
    assert!(!is_openable_external_url("file:///etc/passwd"));
    assert!(!is_openable_external_url("javascript:alert(1)"));
    assert!(!is_openable_external_url(
        "data:text/html,<script>alert(1)</script>"
    ));
    // The app's own custom scheme is not a browser link.
    assert!(!is_openable_external_url("cuaderno://note/x"));
    // Relative paths, bare fragments, and empties have no openable scheme.
    assert!(!is_openable_external_url("../secret"));
    assert!(!is_openable_external_url("#section"));
    assert!(!is_openable_external_url(""));
    // `https` without `//` is not a real web URL.
    assert!(!is_openable_external_url("https:evil"));
}
