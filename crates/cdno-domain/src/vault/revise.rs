//! `Vault::revise_note` — the logged, hash-guarded in-place edit for
//! mutable custom notes (RFC 0002 §5.3, §6.2).
//!
//! A concept note is the current best account of something, so it is
//! refined in place. Every refinement made through the tool leaves one
//! `revised [[path]] — reason` (or `revised [[path#Heading]] — reason`)
//! line in today's daily log, in the same transaction as the note
//! write. There is deliberately no `was:`/`now:` block: the log records
//! *when* and *why*, git records *what*.
//!
//! The lost-update guard is the point of the operation: the caller
//! passes the `content_hash` it got from [`Vault::read_note`], and the
//! hash of the bytes on disk is recomputed and compared **after** the
//! transaction (and so the vault write lock) is taken. An edit that
//! landed between the caller's read and this call is refused rather
//! than silently overwritten.
//!
//! This is the transactional successor of
//! [`Vault::write_note_raw`](super::Vault::write_note_raw).

use chrono::NaiveDateTime;

use cdno_core::error::ManipulationError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::hash::content_hash;
use cdno_core::markdown::{MarkdownDocument, headings};
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::type_registry::NoteTypeDescriptor;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::log::flatten_for_log;

/// What a [`Vault::revise_note`] call changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// Replace the whole body after the frontmatter. The frontmatter
    /// block is preserved byte-for-byte and the body is written exactly
    /// as given, so a body taken from [`Vault::read_note`] round-trips
    /// unchanged. "Verbatim" includes a body that itself starts with
    /// `---`: it is not parsed as frontmatter, so it lands in the body
    /// looking like a second frontmatter block.
    Body(String),
    /// Upsert the section under `heading` (heading text as
    /// [`Vault::read_note`]'s `headings` lists it, without the `#`
    /// markers).
    ///
    /// When the heading exists, its content is replaced. A section
    /// spans from its heading to the next heading of **equal or higher
    /// level**, so any sub-sections under it are part of its content and
    /// are replaced with it. The content is trimmed and re-framed with
    /// the section's existing leading and trailing blank lines, so the
    /// note's layout around the section is kept; a blank section gets one
    /// blank line after its heading.
    ///
    /// When no heading matches, a level-2 `## <heading>` section holding
    /// the content is appended at the end of the body. A heading that
    /// matches more than once is refused.
    ///
    /// `heading` becomes a wikilink anchor in the log line, so it must be
    /// non-empty, on one line, free of `[`, `]`, `|` and `#`, and must not
    /// start with `^`. `content` may hold headings only deeper than the
    /// target section's own level (level 2 for a new section): a heading
    /// at that level or higher would split the section and restructure
    /// the note. Headings inside fenced code are not headings.
    Section { heading: String, content: String },
}

/// Result of a [`Vault::revise_note`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviseOutcome {
    /// The revised note.
    pub path: VaultPath,
    /// `false` when the revision produced text identical to the note's
    /// current content: nothing was written and nothing was logged.
    pub changed: bool,
    /// `content_hash` of the note's content after the call — the new
    /// bytes when `changed`, the unchanged bytes otherwise. A caller
    /// chaining revisions passes this as the next `expected_hash`.
    pub new_hash: String,
    /// The daily-log entry written (without the `- **HH:MM**: ` prefix
    /// the daily log adds), or `None` when nothing changed.
    pub log_line: Option<String>,
    /// The anchored wikilink target `<path without .md>#<heading>` the
    /// log line links to, for a section revision that changed the note;
    /// `None` for a whole-body revision or when nothing changed. Built
    /// from the same value the log line is formatted from, so a caller
    /// never has to parse it back out of `log_line`.
    pub section_target: Option<String>,
}

impl Vault {
    /// Revise a mutable custom note in place, logging the revision to
    /// today's daily note in the same transaction.
    ///
    /// Refusals, none of which write anything:
    /// - an empty or whitespace-only `reason` → [`DomainError::EmptyField`];
    /// - a built-in note type (their sections are owned by behaviour) or a
    ///   custom type declared `append_only = true` →
    ///   [`DomainError::NoteNotRevisable`]; a type the registry does not
    ///   know → [`DomainError::UnknownNoteType`];
    /// - `expected_hash` is `Some` and differs from the hash of the bytes
    ///   on disk, read under the vault write lock →
    ///   [`DomainError::StaleRevision`]. `None` skips the check;
    /// - a [`Revision::Section`] whose heading is not a valid wikilink
    ///   anchor, or whose content holds a heading at the target section's
    ///   level or higher → [`DomainError::RevisionInvalid`];
    /// - a [`Revision::Section`] whose heading matches more than one
    ///   heading → the `AmbiguousSection` manipulation error. A heading
    ///   that matches none is not refused: the section is appended.
    ///
    /// The note's type is read from the `type:` field of the bytes just
    /// read (the source of truth), not from the index row.
    ///
    /// When the revised text equals the current text, returns
    /// `changed == false` and writes and logs nothing. Otherwise the note
    /// write, its index row, and the daily-log line
    /// `revised [[<path without .md>]] — <reason>` (with `#<heading>`
    /// appended inside the link for a section revision; the reason
    /// flattened to one line) commit together.
    ///
    /// The note's `links` and tag facets are refreshed only by reconcile
    /// until #646 lands, so [`Vault::read_note`]'s `backlinks` can lag a
    /// wikilink added by a revision.
    ///
    /// `at` is a parameter so tests can pin the log timestamp and the
    /// daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn revise_note(
        &self,
        path: &VaultPath,
        expected_hash: Option<&str>,
        revision: Revision,
        reason: &str,
        at: NaiveDateTime,
    ) -> Result<ReviseOutcome, DomainError> {
        if reason.trim().is_empty() {
            return Err(DomainError::EmptyField { field: "reason" });
        }
        if let Revision::Section { heading, .. } = &revision {
            validate_anchor(heading)?;
        }

        // Lock first: the read, the hash and the compare below must all
        // happen under the vault write lock, or an edit landing between
        // the compare and the commit would be silently overwritten.
        let mut tx = self.transaction()?;

        // Same store call as `read_note`, so the hash is over identical
        // bytes to the one the caller was handed.
        let raw = self.store.read_file(path)?;
        let actual_hash = content_hash(&raw);

        // Compare before anything else looks at the bytes: a concurrent
        // edit that broke the frontmatter or changed `type:` must surface
        // as `StaleRevision` ("read it again"), not as a parse error.
        if let Some(expected) = expected_hash
            && expected != actual_hash
        {
            return Err(DomainError::StaleRevision {
                path: path.to_string(),
                expected: expected.to_owned(),
                actual: actual_hash,
            });
        }

        let (fm, body) = Frontmatter::parse(&raw)?;
        let note_type = fm.require_field::<String>("type")?;
        self.ensure_revisable(path, &note_type)?;

        let link_target = path
            .to_string()
            .strip_suffix(".md")
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string());
        let (new_content, anchor) = match &revision {
            Revision::Body(new_body) => {
                let frontmatter_block = &raw[..raw.len() - body.len()];
                (format!("{frontmatter_block}{new_body}"), None)
            }
            Revision::Section { heading, content } => {
                let mut doc = MarkdownDocument::parse(raw.clone())?;
                // Upsert: a heading that matches nothing gets a new
                // level-2 section at the end of the body. Ambiguity is
                // still an error.
                let target_level = match doc.section(heading) {
                    Ok(_) => headings(body)
                        .into_iter()
                        .find(|(_, text)| text == heading)
                        .map_or(2, |(level, _)| level),
                    Err(ManipulationError::SectionNotFound(_)) => {
                        doc.ensure_section(heading)?;
                        2
                    }
                    Err(e) => return Err(e.into()),
                };
                ensure_no_restructuring_heading(content, target_level)?;

                let current = doc.section(heading)?;
                // Whether the heading line ends in a newline: false only
                // for a heading on the last line of a file with no final
                // newline, where the framing must supply it.
                let start = current.as_ptr() as usize - doc.render().as_ptr() as usize;
                let terminated = doc.render()[..start].ends_with('\n');
                let framed = frame_section_content(current, content, terminated);
                doc.replace_section(heading, &framed)?;
                let mut rendered = doc.render().to_owned();
                // A blank section framed with a trailing blank line may
                // leave one the original document did not end with.
                if rendered.ends_with("\n\n") && !raw.ends_with("\n\n") {
                    let trimmed = rendered.trim_end().len();
                    rendered.truncate(trimmed);
                    rendered.push('\n');
                }
                (rendered, Some(heading.as_str()))
            }
        };

        if new_content == raw {
            return Ok(ReviseOutcome {
                path: path.clone(),
                changed: false,
                new_hash: actual_hash,
                log_line: None,
                section_target: None,
            });
        }

        let section_target = anchor.map(|heading| format!("{link_target}#{heading}"));
        let target = section_target.as_deref().unwrap_or(&link_target);
        let log_line = format!("revised [[{target}]] \u{2014} {}", flatten_for_log(reason));

        let new_hash = content_hash(&new_content);
        let entry_meta = build_index_entry_for(path, &new_content, &note_type)?;
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_line, &mut tx)?;
        tx.commit()?;

        Ok(ReviseOutcome {
            path: path.clone(),
            changed: true,
            new_hash,
            log_line: Some(log_line),
            section_target,
        })
    }

    /// Refuse built-in and append-only types. `append_only` on a custom
    /// type is accepted by the registry but not enforced anywhere else
    /// yet; this is where a revision honours it.
    fn ensure_revisable(&self, path: &VaultPath, note_type: &str) -> Result<(), DomainError> {
        let registry = self.type_registry();
        let refuse = |reason: String| DomainError::NoteNotRevisable {
            path: path.to_string(),
            reason,
        };
        match registry.resolve(note_type) {
            None => Err(DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            }),
            Some(NoteTypeDescriptor::Builtin(_)) => Err(refuse(format!(
                "'{note_type}' is a built-in note type; its sections are owned by its own commands"
            ))),
            Some(descriptor) if descriptor.append_only() => {
                Err(refuse(format!("note type '{note_type}' is append-only")))
            }
            Some(_) => Ok(()),
        }
    }
}

/// Refuse a section heading that cannot be a wikilink anchor in the
/// `revised [[path#Heading]]` log line: `[`/`]` nest or close the link,
/// `|` starts an alias, `#` starts another anchor, a leading `^` makes
/// it a block reference, and a newline breaks the line.
fn validate_anchor(heading: &str) -> Result<(), DomainError> {
    let invalid = |why: String| DomainError::RevisionInvalid {
        reason: format!("section heading '{heading}' {why}"),
    };
    if heading.trim().is_empty() {
        return Err(DomainError::RevisionInvalid {
            reason: "section heading is empty".to_owned(),
        });
    }
    if heading.contains(['\n', '\r']) {
        return Err(invalid("spans more than one line".to_owned()));
    }
    if let Some(c) = heading.chars().find(|c| matches!(c, '[' | ']' | '|' | '#')) {
        return Err(invalid(format!(
            "contains '{c}', which cannot appear in a wikilink anchor"
        )));
    }
    if heading.starts_with('^') {
        return Err(invalid(
            "starts with '^', which would make the anchor a block reference".to_owned(),
        ));
    }
    Ok(())
}

/// Refuse section content holding a heading at `target_level` or higher
/// (a smaller number): it would end the section early and add sections
/// to the note behind the revision's back.
fn ensure_no_restructuring_heading(content: &str, target_level: u8) -> Result<(), DomainError> {
    match headings(content)
        .into_iter()
        .find(|(level, _)| *level <= target_level)
    {
        Some((level, text)) => Err(DomainError::RevisionInvalid {
            reason: format!(
                "content contains the level-{level} heading '{text}'; a section's content may \
                 only hold headings deeper than its own level ({target_level})"
            ),
        }),
        None => Ok(()),
    }
}

/// Frame caller-supplied section content with the section's existing
/// leading and trailing whitespace, so replacing a section keeps the
/// blank lines around it (and never glues the content onto the next
/// heading). A blank section gets one blank line either side; cleared
/// content leaves exactly one blank line before whatever follows.
/// `terminated` says whether the heading line already ends in a newline.
fn frame_section_content(current: &str, content: &str, terminated: bool) -> String {
    let content = content.trim();
    let blank = current.trim().is_empty();
    if content.is_empty() {
        // Clearing an already-blank section keeps it byte-identical, so
        // it is a no-op rather than a whitespace-only rewrite.
        return if blank {
            current.to_owned()
        } else {
            "\n".to_owned()
        };
    }
    let (lead, trail) = if blank {
        (if terminated { "\n" } else { "\n\n" }, "\n\n")
    } else {
        let lead = &current[..current.len() - current.trim_start().len()];
        let trail = &current[current.trim_end().len()..];
        (lead, trail)
    };
    format!("{lead}{content}{trail}")
}
