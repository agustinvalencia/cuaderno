//! Read-back verification for the mutating tools (GH #539).
//!
//! Every write tool used to end in `json_result(WriteResultDto::new(…))`,
//! which reports success from the fact that the domain call returned —
//! not from anything on disk. Over a remote transport that is a silent
//! failure mode: the client cannot tell a write that landed from one
//! that did not, and the loss surfaces much later, if at all.
//!
//! So the shared shape verifies instead of the ~27 handlers each doing
//! it: a handler hands its path, its summary line and the *shape* of
//! the write to [`CuadernoServer::verified_write`], which re-reads the
//! target and either attaches [`WriteVerificationDto`] to the success
//! result or returns a tool **error**. There is no third outcome — an
//! unverifiable write is never reported as a success.
//!
//! # What this does and does not prove
//!
//! It proves the target is readable and non-empty after the write, and
//! it hands the caller the bytes, the fingerprint, and (for the
//! log-append shape) the trailing text now on disk. It does **not**
//! diff against an intended content: the handlers do not have one — the
//! domain composes the final bytes (the log line's clock stamp, the
//! template render, the section fold). Content-level judgement stays
//! with the caller, which is exactly what the returned tail and hash
//! are for.
//!
//! # Cost
//!
//! One whole-file read per write. That is deliberately not optimised
//! into a seek-to-tail read, because the write it verifies already
//! costs more: `FsVaultStore::append_to_file` is a
//! read-concat-atomic-rewrite (see `cdno-core/src/store.rs`), so even
//! an append has already read the whole file and written it back before
//! verification starts. A tail-only read would need a new `VaultStore`
//! method whose sole consumer is this module, and would still need a
//! `stat` for the total size. Vault notes are markdown at note scale —
//! the largest are daily logs of a few kilobytes — and the one tool
//! that can touch a large artefact (`file_to_portfolio` with `attach`)
//! returns the path of the markdown *stub*, never the artefact.

use rmcp::model::{CallToolResult, ErrorData};

use cdno_core::path::VaultPath;
use cdno_domain::Vault;
use cdno_domain::error::DomainError;

use crate::dto::{WriteResultDto, WriteVerificationDto};
use crate::server::CuadernoServer;
use crate::util::json_result;

/// How much trailing text an append-shaped result carries back. Big
/// enough for the appended log line plus the surrounding context that
/// makes it legible, small enough that it never becomes the bulk of a
/// tool result.
const TAIL_BYTES: usize = 512;

/// What the write did to its target, which is what decides how it can
/// be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteShape {
    /// The file was created or rewritten in place. Verified by
    /// re-reading it whole.
    Rewritten,
    /// Content was appended at the **end** of the file. Verified like
    /// [`Rewritten`](Self::Rewritten), and additionally carries the
    /// trailing window back so the caller can see what landed.
    ///
    /// Only for writes that genuinely land at EOF. The daily note's
    /// `## Logs` section is pinned to the bottom (`cdno-domain`'s
    /// `log` module), so `append_to_log` qualifies; a tool that appends
    /// a bullet into a mid-file section does not, and uses
    /// [`Rewritten`](Self::Rewritten) — a tail that is not the changed
    /// part would be worse than no tail at all.
    Appended,
    /// The write **deleted** its target. Verified by confirming the
    /// file is gone; there is nothing to hash.
    Removed,
}

impl CuadernoServer {
    /// Finish a mutating tool: re-read `path`, and either return the
    /// success result with verification attached or a tool error.
    ///
    /// This is the single place a write result is built, so every
    /// mutating tool inherits verification without touching its
    /// handler.
    pub(crate) async fn verified_write(
        &self,
        path: VaultPath,
        message: String,
        shape: WriteShape,
    ) -> Result<CallToolResult, ErrorData> {
        let target = path.clone();
        let verification = self
            .with_vault(move |vault| verify(vault, &target, shape))
            .await??;
        json_result(WriteResultDto::new(path.to_string(), message, verification))
    }
}

/// Re-read `path` and describe what is on disk, or explain why it could
/// not be verified.
fn verify(
    vault: &Vault,
    path: &VaultPath,
    shape: WriteShape,
) -> Result<WriteVerificationDto, ErrorData> {
    if shape == WriteShape::Removed {
        return match vault.read_note_raw(path) {
            Err(DomainError::Store(cdno_core::error::StoreError::NotFound(_))) => {
                Ok(WriteVerificationDto {
                    verified: "removed".to_owned(),
                    bytes_written: 0,
                    content_hash: None,
                    appended_tail: None,
                })
            }
            Ok(_) => Err(unverified(
                path,
                "the file is still present after the delete",
            )),
            Err(e) => Err(unverified(path, &e.to_string())),
        };
    }

    let content = vault
        .read_note_raw(path)
        .map_err(|e| unverified(path, &e.to_string()))?;
    // Every write path in the domain emits at least a frontmatter
    // block, so an empty file where a note should be is a write that
    // did not land (or landed truncated) rather than a legitimate
    // state.
    if content.is_empty() {
        return Err(unverified(path, "the file is empty"));
    }

    Ok(WriteVerificationDto {
        verified: "content".to_owned(),
        bytes_written: content.len() as u64,
        content_hash: Some(cdno_core::hash::content_hash(&content)),
        appended_tail: match shape {
            WriteShape::Appended => Some(tail(&content, TAIL_BYTES).to_owned()),
            WriteShape::Rewritten | WriteShape::Removed => None,
        },
    })
}

/// The last `max_bytes` of `s`, moved forward to the nearest character
/// boundary so the window is always valid UTF-8 (a note's tail can end
/// mid-multibyte-character otherwise).
fn tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// The error a caller gets when the write cannot be verified. Worded so
/// an agent does the right thing with it: the write is of *unknown*
/// outcome, not known-failed, so the next step is to re-read the note
/// rather than to blindly retry a write that may have landed.
fn unverified(path: &VaultPath, reason: &str) -> ErrorData {
    ErrorData::internal_error(
        format!(
            "write to {path} could not be verified ({reason}); the change may not have landed \u{2014} \
             re-read the note before retrying"
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn tail_is_the_whole_string_when_shorter_than_the_window() {
        assert_eq!(tail("short", 512), "short");
    }

    #[test]
    fn tail_never_splits_a_multibyte_character() {
        // "é" is two bytes; a naive `len - max_bytes` slice lands
        // inside it and would panic.
        let s = "aéaéaéaé";
        for window in 1..=s.len() {
            let got = tail(s, window);
            assert!(s.ends_with(got), "the tail must be a suffix of the input");
            assert!(got.len() <= window, "the tail must fit the window");
        }
    }
}
