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

use std::ops::Range;

use xxhash_rust::xxh3::xxh3_64;

use crate::extractors::wikilink_spans;
use crate::frontmatter::split_frontmatter;
use crate::frontmatter_edit::frontmatter_scalar_spans;

/// Stable 64-bit fingerprint of `content`, rendered as a 16-character
/// lowercase hex string. Length and case are load-bearing for the
/// `notes.content_hash` column — downstream SQL comparisons assume
/// both.
pub fn content_hash(content: &str) -> String {
    format!("{:016x}", xxh3_64(content.as_bytes()))
}

/// Version of the masking rule behind [`masked_content_hash`]. Stamped next
/// to every masked hash; a change to what is masked bumps it, so a stored
/// hash is never compared against a different rule.
pub const MASK_VERSION: u32 = 1;

/// What every masked range is replaced with before hashing.
const MASK_TOKEN: &str = "\u{0}masked\u{0}";

/// Like [`content_hash`], but over `prefix` with the references masked
/// (RFC 0007 §5.6): every wikilink **address** (the part before a `#` or
/// `|`; anchors and labels stay) and every scalar under each of
/// `reference_keys` in the frontmatter block become one fixed token. A
/// reference rewritten by a rename therefore does not change the hash;
/// any other byte does.
///
/// The frontmatter block is found with
/// [`split_frontmatter`]; its scalar ranges are relative to the block, so
/// they are shifted by the block's offset in `prefix`. Wikilinks are
/// located over the whole prefix (frontmatter included, so `project:
/// "[[x]]"` is masked as an address). A scalar that holds a wikilink is
/// left to the address mask, which keeps its label. Core knows no key
/// names: the list is the caller's.
pub fn masked_content_hash(prefix: &str, reference_keys: &[&str]) -> String {
    let mut ranges: Vec<Range<usize>> = wikilink_spans(prefix)
        .into_iter()
        .map(|s| s.address)
        .collect();
    let addresses = ranges.clone();
    if let Some((block, _)) = split_frontmatter(prefix) {
        let base = block.as_ptr() as usize - prefix.as_ptr() as usize;
        for key in reference_keys {
            for r in frontmatter_scalar_spans(block, key) {
                let r = base + r.start..base + r.end;
                let holds_link = addresses.iter().any(|a| a.start < r.end && r.start < a.end)
                    || addresses
                        .iter()
                        .any(|a| a.start == a.end && r.contains(&a.start));
                if !holds_link {
                    ranges.push(r);
                }
            }
        }
    }
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut masked = String::with_capacity(prefix.len());
    let mut at = 0;
    for r in ranges {
        if r.start < at {
            continue; // overlapping; already masked
        }
        masked.push_str(&prefix[at..r.start]);
        masked.push_str(MASK_TOKEN);
        at = r.end;
    }
    masked.push_str(&prefix[at..]);
    content_hash(&masked)
}
