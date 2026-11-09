//! The lint rows for Typst notes (RFC 0008 §5.10).
//!
//! Lint is index-driven, so a `.typ` the classifier left out, or that reconciliation refused, is
//! never in the per-note loop. [`Vault::typst_walk_issues`] re-runs the classifier (plan P1) and
//! reports those files once each; [`typst_file_issues`] is the one reading of a labelled file's
//! bytes, used by the loop for an indexed note and by the walk for one that is not.

use std::collections::{HashMap, HashSet};

use cdno_core::artefacts::{is_attachment_stub, is_typst_note, owning_note};
use cdno_core::classify::{SkipReason, classify};
use cdno_core::frontmatter::split_frontmatter;
use cdno_core::path::VaultPath;
use cdno_core::paths::{NoteFormat, note_format};
use cdno_core::typst::{self, TypstError};

use crate::error::DomainError;
use crate::lint::LintIssue;
use crate::type_registry::NoteTypeDescriptor;

use super::Vault;

const OBSIDIAN_CAUSE: &str =
    "an editor that resolves `[[x]]` only to `x.md` may have created the empty file";

/// Every row one labelled `.typ` can raise. `admitted` is the note types that admit Typst; empty
/// means no check, as in reconciliation.
pub(super) fn typst_file_issues(
    vault: &Vault,
    path: &VaultPath,
    content: &str,
    admitted: &[String],
) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let error = |message: String| LintIssue::error(path.clone(), message);

    match typst::metadata_block(content) {
        Ok(block) => {
            let note_type = block.value.get("type").and_then(|v| v.as_str());
            match note_type {
                None => issues.push(error(missing_key("type"))),
                Some(note_type) => {
                    if !admitted.is_empty() && !admitted.iter().any(|t| t == note_type) {
                        issues.push(error(format!(
                            "Typst note of type `{note_type}` does not admit Typst, so it is not \
                             indexed; admit the type under `[typst] note_types` or write the note \
                             in Markdown"
                        )));
                    } else if requires_title(vault, note_type) && !has_title(&block.value) {
                        issues.push(error(missing_key("title")));
                    }
                }
            }
            if typst::label_node_count(content).is_some_and(|n| n > 1) {
                issues.push(error(
                    "a second `<cdno>` label sits outside the metadata block; remove it, the \
                     label belongs to the `#metadata((…))` call only"
                        .to_owned(),
                ));
            }
        }
        Err(e) => issues.push(error(block_error(&e, content))),
    }

    if let Err(fault) = typst::parses(content) {
        issues.push(LintIssue::warning(
            path.clone(),
            format!(
                "Typst note does not parse (line {}: {}); fix the syntax, as a hand edit has \
                 broken what creation accepted",
                fault.line, fault.message
            ),
        ));
    }
    for fault in typst::wikilink_spans(content).faults {
        let remedy = match fault.leaf.as_str() {
            "end of run" => {
                "close the link with `]]`, or backslash-escape the `[[` if it is not a link"
            }
            "Space" | "Parbreak" | "Escape" => "keep the whole `[[…]]` on one line",
            _ => {
                "backslash-escape the markup character inside the `[[…]]`, such as `\\#` for an anchor"
            }
        };
        issues.push(error(format!(
            "link interior not one inline run at line {}: {}; {remedy}",
            fault.line,
            crate::error::link_fault_reason(&fault.leaf)
        )));
    }
    issues
}

fn missing_key(key: &str) -> String {
    format!(
        "metadata block missing a required key: `{key}`; add `{key}: \"…\"` to the \
         `#metadata((…)) <cdno>` block"
    )
}

fn block_error(e: &TypstError, content: &str) -> String {
    match e {
        TypstError::NoBlock if typst::has_label_node(content) => "`<cdno>` is not the label of \
             a `#metadata((…))` call; put the label directly after the call's closing parenthesis"
            .to_owned(),
        TypstError::NoBlock => "no metadata block: `<cdno>` appears only inside a comment or a \
             string; write `#metadata((type: \"…\")) <cdno>` at the top level"
            .to_owned(),
        TypstError::LabelBroken { line } => broken_label(*line),
        TypstError::Nested { .. } | TypstError::NotFirst { .. } | TypstError::Duplicate { .. } => {
            format!(
                "metadata block not first or not top-level ({e}); keep a single block at the \
                 top of the file, outside any content block"
            )
        }
        TypstError::NotLiteral { .. } | TypstError::NotADictionary { .. } => format!(
            "metadata block not literal ({e}); write only strings, arrays of strings, booleans \
             and numbers"
        ),
        TypstError::Unwritable { .. } | TypstError::Fault { .. } => {
            format!("could not read the metadata block ({e})")
        }
    }
}

fn broken_label(line: usize) -> String {
    format!(
        "metadata present but the `<cdno>` label is missing, mistyped or nested (line {line}); \
         put `<cdno>` directly after the `#metadata((…))` call"
    )
}

/// A type whose Markdown form carries a title heading: a custom type with a `title_field`, or
/// `concept` (§5.3).
fn requires_title(vault: &Vault, note_type: &str) -> bool {
    match vault.type_registry().resolve(note_type) {
        Some(NoteTypeDescriptor::Custom { name, def }) => {
            def.title_field.is_some() || name == "concept"
        }
        _ => false,
    }
}

fn has_title(value: &serde_json::Value) -> bool {
    value
        .get("title")
        .and_then(|v| v.as_str())
        .is_some_and(|t| !t.trim().is_empty())
}

impl Vault {
    /// Rows for the `.typ` files the per-note loop cannot see: the classifier's report, the
    /// labelled files reconciliation refused, a stem shared with a `.md`, and Markdown notes
    /// evicted into a Typst note's assets folder. `indexed` is the index's path set.
    pub(super) fn typst_walk_issues(
        &self,
        indexed: &HashSet<VaultPath>,
    ) -> Result<Vec<LintIssue>, DomainError> {
        let scope = self.note_scope()?;
        if scope.typst_folders.is_empty() && scope.typst_excluded_folders.is_empty() {
            return Ok(Vec::new());
        }
        let classified = classify(&self.store, &scope)?;
        let mut issues = Vec::new();

        for skipped in &classified.skipped {
            let message = match skipped.reason {
                SkipReason::TypstUnlabelled => {
                    "Typst file is neither a note nor an attachment: it \
                     has no `<cdno>` block and no note owns its folder; add a block to make it a \
                     note, or move it into the assets folder of the note that uses it"
                        .to_owned()
                }
                SkipReason::TypstOutsideScope => "labelled Typst file outside the configured \
                     types, so it is not indexed; admit its type under `[typst] note_types` or \
                     move it to a folder of a type that admits Typst"
                    .to_owned(),
                SkipReason::TypstLabelBroken { line } => broken_label(line),
                SkipReason::Attachment | SkipReason::Ignored => continue,
            };
            let issue = match skipped.reason {
                SkipReason::TypstLabelBroken { .. } => {
                    LintIssue::error(skipped.path.clone(), message)
                }
                _ => LintIssue::warning(skipped.path.clone(), message),
            };
            issues.push(issue);
        }

        let typ_notes = classified
            .notes
            .iter()
            .filter(|p| note_format(p) == Some(NoteFormat::Typst));
        for path in typ_notes {
            if !indexed.contains(path) {
                // A failed read is reconciliation's to report.
                if let Ok(content) = self.store.read_file(path) {
                    issues.extend(typst_file_issues(self, path, &content, &scope.typst_types));
                }
            }
            let sibling = path.as_path().with_extension("md");
            if VaultPath::new(&sibling).is_ok_and(|md| classified.files.contains(&md)) {
                let stem = path.as_path().with_extension("");
                issues.push(LintIssue::error(
                    path.clone(),
                    format!(
                        "stem shared between `{stem}.md` and `{stem}.typ`, so `[[{stem}]]` names \
                         only one of them; remove or merge the `.md`: {OBSIDIAN_CAUSE}",
                        stem = stem.display()
                    ),
                ));
            }
        }

        // A Markdown note inside the assets folder of a Typst note is evicted from the index
        // silently by the classifier; name each one.
        let mut owner_cache: HashMap<VaultPath, bool> = HashMap::new();
        for file in &classified.files {
            if note_format(file) != Some(NoteFormat::Markdown)
                || classified.notes.contains(file)
                || scope.ignore.is_match(file.as_path())
            {
                continue;
            }
            let Some(owner) = owning_note(file, &scope, |o| {
                *owner_cache.entry(o.clone()).or_insert_with(|| {
                    is_attachment_stub(&self.store, o) || is_typst_note(&self.store, o)
                })
            }) else {
                continue;
            };
            if note_format(&owner) != Some(NoteFormat::Typst) {
                continue;
            }
            let has_frontmatter = self
                .store
                .read_file(file)
                .is_ok_and(|c| split_frontmatter(&c).is_some());
            if has_frontmatter {
                issues.push(LintIssue::warning(
                    file.clone(),
                    format!(
                        "this Markdown note sits in the assets folder of Typst note `{owner}`, so \
                         it is not indexed; move it out of `{}/`",
                        owner.as_path().with_extension("").display()
                    ),
                ));
            }
        }
        Ok(issues)
    }
}
