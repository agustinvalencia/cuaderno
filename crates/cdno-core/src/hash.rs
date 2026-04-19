//! Non-cryptographic content hashing for change detection.
//!
//! Reconciliation uses `(mtime, content_hash)` pairs to decide
//! whether a note has changed since the last index. The hash has to
//! be stable across runs, sensitive to whitespace-only edits (which
//! `mtime` alone can miss on a rapid write that wraps to the same
//! second), and cheap — xxh3 fits all three.
//!
//! The hash is **not** cryptographic. Do not use it for tamper
//! evidence, signing, or anything that requires collision resistance
//! against an adversary. Collision probability at vault scale
//! (thousands of notes, random content) is vanishing.

use xxhash_rust::xxh3::xxh3_64;

/// Stable 64-bit fingerprint of `content`, rendered as a 16-character
/// lowercase hex string. Length and case are load-bearing for the
/// `notes.content_hash` column — downstream SQL comparisons assume
/// both.
pub fn content_hash(content: &str) -> String {
    format!("{:016x}", xxh3_64(content.as_bytes()))
}
