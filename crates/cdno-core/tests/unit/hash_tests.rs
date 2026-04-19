use cdno_core::hash::content_hash;

#[test]
fn hash_is_deterministic_for_same_input() {
    let a = content_hash("hello world");
    let b = content_hash("hello world");
    assert_eq!(a, b);
}

#[test]
fn hash_differs_for_different_inputs() {
    let a = content_hash("hello");
    let b = content_hash("world");
    assert_ne!(a, b);
}

#[test]
fn hash_is_sensitive_to_trailing_whitespace() {
    // mtime alone can't detect a trailing-whitespace-only change, so
    // the hash has to — otherwise reconciliation would silently skip
    // the edit.
    let a = content_hash("text");
    let b = content_hash("text ");
    assert_ne!(a, b);
}

#[test]
fn hash_is_sensitive_to_newline_difference() {
    // CRLF vs LF is a real file change and the hash must surface it.
    let lf = content_hash("line one\nline two");
    let crlf = content_hash("line one\r\nline two");
    assert_ne!(lf, crlf);
}

#[test]
fn hash_is_16_lowercase_hex_chars() {
    let h = content_hash("anything");
    assert_eq!(h.len(), 16);
    assert!(
        h.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
}

#[test]
fn hash_of_empty_string_is_stable() {
    // The empty string is a valid input — an empty note should still
    // get a well-defined fingerprint.
    let h = content_hash("");
    assert_eq!(h.len(), 16);
    assert_eq!(h, content_hash(""));
}

#[test]
fn hash_handles_utf8_multibyte() {
    let a = content_hash("café");
    let b = content_hash("cafe");
    assert_ne!(a, b);
}
