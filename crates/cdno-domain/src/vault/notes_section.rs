//! `Vault::note_to_daily` — substance to the daily note's `## Notes`, a
//! pointer to `## Logs` (RFC 0002 §5.4, T2).
//!
//! `## Logs` is the day's *sequence*: one line per event, pointers
//! only. `## Notes` is its *substance*: worked-out material (a
//! derivation, a procedure, a page of reasoning) that would otherwise sit
//! as a block of text inside the sequence. `note_to_daily` writes both
//! halves of one such event in a single transaction:
//!
//! ```markdown
//! ## Notes
//!
//! ### Woodbury identity
//! (A + UCV)^-1 = … today's use was the k=3 refit on [[projects/surrogate-model]].
//!
//! ## Logs
//! - **14:32**: noted [[journal/2026/daily/2026-09-27#Woodbury identity]] ([[projects/surrogate-model]])
//! ```
//!
//! so an agent never has to edit the daily note directly, and the
//! sequence shows *that* a derivation happened and *what it touched*
//! while the substance stays out of it. `noted [[` joins the reserved
//! log-line prefix family.

use chrono::NaiveDateTime;

use cdno_core::extractors::extract_wikilinks;
use cdno_core::markdown::headings;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::daily::DailySection;
use super::index_entry::build_index_entry_for;
use super::log::daily_note_path;

/// What [`Vault::note_to_daily`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteToDailyOutcome {
    /// The daily note both halves landed in
    /// (`journal/<year>/daily/<YYYY-MM-DD>.md`).
    pub path: VaultPath,
    /// The anchored wikilink target the pointer line links to —
    /// `journal/<year>/daily/<YYYY-MM-DD>#<heading>`, without the
    /// surrounding `[[…]]` — for callers that want to cite the entry.
    pub target: String,
    /// The pointer text appended to `## Logs`, without the
    /// `- **HH:MM**: ` prefix every log line carries:
    /// `noted [[<target>]]`, then ` (<links>)` when the body links out.
    pub log_line: String,
}

impl Vault {
    /// Append a `### <heading>` entry to the `## Notes` section of the
    /// daily note for `at`'s date, and a `noted [[…#<heading>]]` pointer
    /// line to its `## Logs`, in **one** transaction and one file write.
    ///
    /// # The entry
    ///
    /// The entry is `### <heading>\n<body>`, appended exactly as
    /// [`Vault::upsert_daily_section`] appends to [`DailySection::Notes`]
    /// (the shared `fold_daily_section` path): the daily note is
    /// scaffolded if absent, and a first `## Notes` section is placed
    /// where T1 puts it — immediately before the note's anchor section
    /// (`## Logs` by default), which stays pinned to the bottom.
    ///
    /// # The pointer line
    ///
    /// `noted [[journal/<year>/daily/<YYYY-MM-DD>#<heading>]]`, followed —
    /// when `body` contains any wikilinks — by a space and a
    /// parenthesised, space-separated list of them: in order of first
    /// appearance, deduplicated by **target** (the first occurrence of a
    /// target wins, with its label and embed marker; later links to the
    /// same target are dropped even when their label differs), each
    /// rendered as `[[target]]`, `[[target|label]]` or `![[target]]` as
    /// written (whitespace just inside the brackets is normalised away by
    /// the extractor). Links are taken from `body`
    /// only, never the heading. The line is timestamped with `at` in the
    /// `- **HH:MM**: ` form [`Vault::log_to_daily_note`] uses, and is
    /// folded into the same in-flight content as the entry, so the file
    /// is written once — two staged writes of the same file would have
    /// the second read the pre-change content back and drop the first.
    ///
    /// # The heading, and why it round-trips
    ///
    /// `heading` is trimmed and then stored verbatim, and the pointer's
    /// anchor uses that same verbatim text — raw heading text is the
    /// canonical anchor (RFC 0002 §5.4). So `[[…#<heading>]]` round-trips
    /// through `MarkdownDocument::section(<heading>)`, which is what T7
    /// (`revise_note`) and T15 rely on to address an entry. That lookup
    /// matches heading text with inline markup **stripped**, so a heading
    /// whose stripped text differs from its raw text (`**bold**`,
    /// `` `code` ``, `*em*`, a backslash escape or an entity) is refused
    /// rather than written with an anchor that would not resolve.
    ///
    /// # Errors
    ///
    /// [`DomainError::HistoryEntryHeadingInvalid`] (section `Notes`) when
    /// the trimmed heading:
    ///
    /// - is empty, or contains a line break;
    /// - contains `[`, `]`, `|` or `#` — each would break the
    ///   `[[…#<heading>]]` pointer: a bracket opens or closes a link, `|`
    ///   starts a label, and a second `#` makes an unsupported nested
    ///   anchor;
    /// - starts with `^` — `[[…#^x]]` is a block reference, not a
    ///   heading anchor; or
    /// - contains inline markup, i.e. `### <heading>` parses to a heading
    ///   whose stripped text is not `<heading>` itself.
    ///
    /// The composed entry (`### <heading>` plus `body`) is then checked by
    /// `validate_history_entry`, the rule T1 introduced, which compares
    /// headings as `MarkdownDocument` sees them (markup stripped, code
    /// fences skipped, setext headings included, Unicode case-folded). So
    /// a heading — the entry's own or any inside `body` — that is level
    /// 1–2, reuses a daily-section name, duplicates a heading already in
    /// that day's note, or duplicates another heading of the same entry
    /// (a `### <heading>` repeated inside `body`) is refused with the same
    /// variant. That check runs under the transaction's write lock, and
    /// on any error nothing is written.
    pub fn note_to_daily(
        &self,
        at: NaiveDateTime,
        heading: &str,
        body: &str,
    ) -> Result<NoteToDailyOutcome, DomainError> {
        let section = DailySection::Notes;
        let heading = validate_entry_heading(heading)?;
        let entry = format!("### {heading}\n{body}");

        let mut tx = self.transaction()?; // lock held across the read-modify-write
        self.validate_history_entry(at.date(), section, &entry)?;

        let path = daily_note_path(at.date())?;
        // The same relpath rule `daily_note_path` uses, minus `.md`.
        let relpath = cdno_core::paths::daily_note_relpath(at.date());
        let target = format!(
            "{}#{heading}",
            relpath.strip_suffix(".md").unwrap_or(&relpath)
        );
        let log_line = format!(
            "{}{}",
            pointer_line(&target, body),
            self.during_tag(at.date())
        );

        let base = self.read_or_scaffold_daily(at.date())?;
        let with_entry = self.fold_daily_section(base, section, &entry, true)?;
        let new_content = self.fold_daily_log_line(at.time(), with_entry, &log_line)?;

        let entry_meta = build_index_entry_for(&path, &new_content, "daily")?;
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(NoteToDailyOutcome {
            path,
            target,
            log_line,
        })
    }
}

/// Check the entry heading's own shape — the parts
/// `validate_history_entry` cannot see because they concern the
/// pointer's anchor rather than the note's headings — and return it
/// trimmed.
///
/// Refuses (as [`DomainError::HistoryEntryHeadingInvalid`]) an empty or
/// multi-line heading; one containing `[`, `]`, `|` or `#`; one starting
/// with `^` (block-reference syntax); and one whose canonical text —
/// `### <heading>` run through [`headings`], the scan
/// `MarkdownDocument::section` matches against — is not the raw heading
/// itself, i.e. one carrying inline markup, escapes or entities, whose
/// `[[…#<heading>]]` anchor would never resolve.
fn validate_entry_heading(heading: &str) -> Result<&str, DomainError> {
    let trimmed = heading.trim();
    let invalid = |reason: &str| DomainError::HistoryEntryHeadingInvalid {
        section: DailySection::Notes.heading().to_string(),
        heading: trimmed.to_string(),
        reason: reason.to_string(),
    };
    if trimmed.is_empty() {
        return Err(invalid("empty"));
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(invalid("a heading must be a single line"));
    }
    if trimmed.contains(['[', ']', '|', '#']) {
        return Err(invalid(
            "`[`, `]`, `|` and `#` would break the `[[…#heading]]` pointer to the entry",
        ));
    }
    if trimmed.starts_with('^') {
        return Err(invalid(
            "a leading `^` makes `[[…#^heading]]` a block reference, not a heading anchor",
        ));
    }
    // The pointer's anchor is the raw text, but `MarkdownDocument`
    // looks sections up by pulldown-cmark's markup-stripped text; the
    // two must be identical for the anchor to resolve.
    let parsed = headings(&format!("### {trimmed}"));
    if parsed != [(3, trimmed.to_string())] {
        return Err(invalid(
            "heading contains inline markup and would not round-trip as an anchor",
        ));
    }
    Ok(trimmed)
}

/// Build the `## Logs` pointer text: `noted [[<target>]]`, plus the
/// body's wikilinks in parentheses when it has any (see
/// [`Vault::note_to_daily`] for the rendering rules).
fn pointer_line(target: &str, body: &str) -> String {
    // Deduplicate by target: the first occurrence of each target keeps
    // its rendered form (label, embed marker); later ones are dropped.
    let mut seen_targets: Vec<String> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    for link in extract_wikilinks(body) {
        if seen_targets.contains(&link.target) {
            continue;
        }
        links.push(format!(
            "{}[[{}{}]]",
            if link.is_embed { "!" } else { "" },
            link.target,
            link.label.map(|l| format!("|{l}")).unwrap_or_default(),
        ));
        seen_targets.push(link.target);
    }
    if links.is_empty() {
        format!("noted [[{target}]]")
    } else {
        format!("noted [[{target}]] ({})", links.join(" "))
    }
}
