//! `Vault::lint_all_notes`.

use std::str::FromStr;

use crate::error::DomainError;
use crate::lint::{LintIssue, LintReport};
use crate::note_type::NoteType;

use super::Vault;

impl Vault {
    /// Validate every indexed note and return a structured report.
    ///
    /// The pass is read-only and skips any file that's not in the
    /// index — non-markdown attachments (PDFs, notebooks) and any
    /// file under `.cuaderno/` are by definition not present, so the
    /// scope of `lint` is exactly "the notes the index knows about".
    ///
    /// Today's checks:
    /// - the entry's `type` field parses as a known [`NoteType`];
    /// - every field listed in the type's `[schemas.<type>]
    ///   extra_required` config section is present in the
    ///   frontmatter.
    ///
    /// Per-type structural checks (e.g. `ProjectFrontmatter` invariants)
    /// land alongside their domain code in Phase 2/3.
    pub fn lint_all_notes(&self) -> Result<LintReport, DomainError> {
        let mut issues: Vec<LintIssue> = Vec::new();

        for path in self.index.list_all_paths()? {
            // A concurrent writer could remove a note between the
            // listing and the lookup. Treat that as "nothing to lint
            // here" rather than a hard error — the next pass will see
            // a coherent state.
            let Some(entry) = self.index.find_by_path(&path)? else {
                continue;
            };

            // The reconciler enforces that every indexed note has a
            // `type` field, but it does not constrain the value to a
            // known variant. An unknown type means downstream code
            // can't pick a schema, so don't bother checking
            // `extra_required` — the report would just compound a
            // problem the user already needs to fix.
            if NoteType::from_str(&entry.note_type).is_err() {
                issues.push(LintIssue {
                    path,
                    message: format!("unknown note type: `{}`", entry.note_type),
                });
                continue;
            }

            for required in self.config.extra_required_fields(&entry.note_type) {
                let present = entry
                    .frontmatter
                    .as_object()
                    .and_then(|obj| obj.get(required))
                    .is_some_and(|v| !v.is_null());
                if !present {
                    issues.push(LintIssue {
                        path: path.clone(),
                        message: format!(
                            "missing required field `{required}` for note type `{}`",
                            entry.note_type
                        ),
                    });
                }
            }

            // Append-only-after-completion (design §5.11, #111).
            // Archived action notes (`actions/_done/<year>/`) may grow
            // new lines but their pre-archival prefix is frozen. We
            // verify by re-hashing the first `frozen_size` bytes and
            // comparing to the snapshot recorded at archival time.
            if entry.note_type == "action"
                && path.as_path().starts_with(cdno_core::paths::ACTIONS_DONE)
                && let Some(snap) = self.index.find_archival_snapshot(&path)?
                && let Some(msg) = check_append_only(&self.store, &path, &snap)?
            {
                issues.push(LintIssue {
                    path: path.clone(),
                    message: msg,
                });
            }
        }

        Ok(LintReport { issues })
    }
}

/// Compare the current file against an archival snapshot. Returns
/// `None` when the file is unchanged or has only grown past the frozen
/// prefix (both allowed); returns a `Some(message)` describing the
/// violation otherwise.
fn check_append_only(
    store: &std::sync::Arc<dyn cdno_core::store::VaultStore>,
    path: &cdno_core::path::VaultPath,
    snap: &cdno_core::index::ArchivalSnapshot,
) -> Result<Option<String>, DomainError> {
    let content = store.read_file(path)?;
    let bytes = content.as_bytes();
    let frozen = snap.frozen_size as usize;
    if bytes.len() < frozen {
        return Ok(Some(format!(
            "archived action note was truncated below its frozen prefix \
             (was {} bytes at archival, now {})",
            snap.frozen_size,
            bytes.len(),
        )));
    }
    // The frozen prefix was valid UTF-8 (it's exactly the file content
    // at archival), so slicing on `frozen` lands on a char boundary
    // both then and now — any insert that disturbs the boundary would
    // already perturb the hash and flag below.
    let prefix = &content[..frozen];
    if cdno_core::hash::content_hash(prefix) != snap.frozen_hash {
        return Ok(Some(
            "archived action note modified an existing line (append-only after completion)"
                .to_owned(),
        ));
    }
    Ok(None)
}
