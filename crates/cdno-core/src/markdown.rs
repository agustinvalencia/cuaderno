use std::ops::Range;

use chrono::NaiveDate;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::error::{ManipulationError, ParseError};
use crate::frontmatter::Frontmatter;
use crate::index::MilestoneEntry;

/// A parsed markdown document with section-level manipulation.
///
/// The raw text is the authoritative representation. Section
/// operations locate headings via `pulldown-cmark`'s byte offsets and
/// splice directly into the raw string — we never rebuild the
/// document from an AST, so formatting (whitespace, blank lines,
/// soft breaks) survives round-trips exactly.
///
/// A "section" is the content following a heading up to the next
/// heading of equal-or-higher level. That means a level-1 heading
/// owns its nested level-2 subsections, which matches how project
/// maps and daily notes are authored in practice.
#[derive(Debug, Clone)]
pub struct MarkdownDocument {
    raw: String,
    frontmatter: Frontmatter,
    // Byte offset at which the body begins inside `raw` — i.e. the
    // first byte after the closing `---` delimiter of the frontmatter
    // block. Heading offsets stored below are body-relative; we add
    // this when splicing into `raw`.
    body_offset: usize,
    headings: Vec<HeadingSpan>,
}

#[derive(Debug, Clone)]
struct HeadingSpan {
    level: u8,
    text: String,
    // Byte range of the heading token as reported by pulldown-cmark,
    // relative to the body (not `raw`). Used for disambiguation only.
    heading_range: Range<usize>,
    // Byte range of the section content: from the end of the heading
    // line through the byte just before the next heading of
    // equal-or-higher level, or to the end of the body if this is
    // the last such heading. Body-relative.
    content_range: Range<usize>,
}

impl MarkdownDocument {
    /// Parse a complete document (frontmatter + body).
    ///
    /// Takes ownership of the raw string so the document can splice
    /// without fighting the borrow checker on later mutations.
    pub fn parse(raw: impl Into<String>) -> Result<Self, ParseError> {
        let raw: String = raw.into();

        // Frontmatter::parse borrows `raw`; we compute the body
        // offset from the returned body slice, then immediately drop
        // the borrow so we can move `raw` into the struct.
        let body_offset = {
            let (_fm, body) = Frontmatter::parse(&raw)?;
            raw.len() - body.len()
        };
        // Re-parse to get an owned Frontmatter without a borrow on `raw`.
        let (frontmatter, _body) = Frontmatter::parse(&raw)?;

        let headings = scan_headings(&raw[body_offset..]);

        Ok(Self {
            raw,
            frontmatter,
            body_offset,
            headings,
        })
    }

    pub fn frontmatter(&self) -> &Frontmatter {
        &self.frontmatter
    }

    /// Render the current document text.
    pub fn render(&self) -> &str {
        &self.raw
    }

    /// Return the section content for the given heading text.
    ///
    /// Errors with [`ManipulationError::SectionNotFound`] if no
    /// heading matches, or [`ManipulationError::AmbiguousSection`]
    /// if more than one heading has the same text.
    pub fn section(&self, heading: &str) -> Result<&str, ManipulationError> {
        let idx = self.find_unique_heading(heading)?;
        let range = self.body_range_to_raw(&self.headings[idx].content_range);
        Ok(&self.raw[range])
    }

    /// Replace the content of the named section with `new_content`.
    pub fn replace_section(
        &mut self,
        heading: &str,
        new_content: &str,
    ) -> Result<(), ManipulationError> {
        let idx = self.find_unique_heading(heading)?;
        let raw_range = self.body_range_to_raw(&self.headings[idx].content_range);
        self.raw.replace_range(raw_range, new_content);
        self.rescan();
        Ok(())
    }

    /// Append `content` to the end of the named section.
    ///
    /// The text is inserted at the end of the section's content,
    /// immediately before the next heading (or at the end of the
    /// body). The caller controls trailing newlines, so appending
    /// `"- [ ] new\n"` to a section that ends with a newline yields
    /// a clean concatenation without extra blank lines.
    pub fn append_to_section(
        &mut self,
        heading: &str,
        content: &str,
    ) -> Result<(), ManipulationError> {
        let idx = self.find_unique_heading(heading)?;
        let insert_at_body = self.headings[idx].content_range.end;
        let insert_at_raw = insert_at_body + self.body_offset;
        self.raw.insert_str(insert_at_raw, content);
        self.rescan();
        Ok(())
    }

    /// Ensure a level-2 section with the given heading exists. If
    /// any heading already matches (even ambiguously), this is a
    /// no-op. Otherwise the heading is appended at the end of the
    /// body with one blank line above it, leaving an empty section
    /// for callers to populate.
    ///
    /// Used by domain mutations (e.g. `add_milestone`,
    /// `add_waiting_on`, `add_action`) so they remain robust against
    /// projects that have drifted from the canonical template — a
    /// migrated or hand-edited file missing `## Milestones` will
    /// have it auto-created on first use rather than rejecting the
    /// write.
    pub fn ensure_section(&mut self, heading: &str) -> Result<(), ManipulationError> {
        if self.headings.iter().any(|h| h.text == heading) {
            return Ok(());
        }
        let trimmed_len = self.raw.trim_end().len();
        self.raw.truncate(trimmed_len);
        let separator = if self.raw.is_empty() { "" } else { "\n\n" };
        self.raw.push_str(separator);
        self.raw.push_str("## ");
        self.raw.push_str(heading);
        self.raw.push('\n');
        self.rescan();
        Ok(())
    }

    /// Move the named level-2 section to the end of the body, so it
    /// stays last regardless of the order sections were created in.
    ///
    /// A normalisation helper, not an assertion: this is a no-op when
    /// the heading is absent (benign — nothing to pin), when it's
    /// ambiguous (two same-named headings — left alone rather than
    /// guessing), or when the section is already last (nothing but
    /// whitespace follows it). So it's safe and idempotent to call on
    /// every write. The daily note uses it to keep `## Logs` (the
    /// running history) pinned to the bottom even when a planning
    /// section like `## Meeting` is created after the first log line.
    ///
    /// The section's content is moved verbatim; only the surrounding
    /// blank lines are normalised (one blank line before, a single
    /// trailing newline). Note this pins the section to the *body* end
    /// without regard to heading level: it's intended for the daily
    /// note's flat `##` layout, where there's a single top-level `#`
    /// heading. A second top-level heading after the target would
    /// capture the moved section — not a case the daily writers hit.
    pub fn move_section_to_end(&mut self, heading: &str) -> Result<(), ManipulationError> {
        let idx = match self.find_unique_heading(heading) {
            Ok(i) => i,
            Err(_) => return Ok(()),
        };
        let span = &self.headings[idx];
        let start = span.heading_range.start + self.body_offset;
        let end = span.content_range.end + self.body_offset;

        // Already last — only whitespace after it. No-op keeps the
        // call idempotent and avoids needless rewrites.
        if self.raw[end..].trim().is_empty() {
            return Ok(());
        }

        // Cut the `## <heading>\n<body>` block (including the blank line
        // that trailed it, up to the next heading) and re-append it at
        // the tail. The blank line that *preceded* the heading stays in
        // place, becoming the separator for whatever section now follows
        // the cut.
        let section_text = self.raw[start..end].trim_end().to_owned();
        self.raw.replace_range(start..end, "");
        let trimmed_len = self.raw.trim_end().len();
        self.raw.truncate(trimmed_len);
        if !self.raw.is_empty() {
            self.raw.push_str("\n\n");
        }
        self.raw.push_str(&section_text);
        self.raw.push('\n');
        self.rescan();
        Ok(())
    }

    fn find_unique_heading(&self, heading: &str) -> Result<usize, ManipulationError> {
        let matches: Vec<usize> = self
            .headings
            .iter()
            .enumerate()
            .filter(|(_, h)| h.text == heading)
            .map(|(i, _)| i)
            .collect();

        match matches.len() {
            0 => Err(ManipulationError::SectionNotFound(heading.to_owned())),
            1 => Ok(matches[0]),
            _ => Err(ManipulationError::AmbiguousSection(heading.to_owned())),
        }
    }

    fn body_range_to_raw(&self, body_range: &Range<usize>) -> Range<usize> {
        (body_range.start + self.body_offset)..(body_range.end + self.body_offset)
    }

    /// Recompute heading spans after a mutation. Parsing a full body
    /// on every edit is cheap for vault-sized notes and avoids the
    /// complexity of incremental range fix-ups.
    fn rescan(&mut self) {
        self.headings = scan_headings(&self.raw[self.body_offset..]);
    }
}

/// Walk the markdown body through `pulldown-cmark` and collect one
/// [`HeadingSpan`] per heading, including its content range.
///
/// Using the offset iterator (rather than a line scanner) is what
/// lets section operations ignore `#`-like content inside fenced
/// code blocks, inline HTML, and other non-heading contexts — the
/// parser only emits `Event::Start(Heading)` for genuine headings.
fn scan_headings(body: &str) -> Vec<HeadingSpan> {
    let mut spans: Vec<HeadingSpan> = Vec::new();
    // A heading event spans a Start ... End pair. Between them we
    // collect the inline text so later `section()` calls can look
    // up by heading label.
    let mut current: Option<(u8, String, Range<usize>)> = None;

    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((level_to_u8(level), String::new(), range));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text, heading_range)) = current.take() {
                    spans.push(HeadingSpan {
                        level,
                        text,
                        heading_range,
                        // Filled in after all headings are known.
                        content_range: 0..0,
                    });
                }
            }
            Event::Text(text) if current.is_some() => {
                current.as_mut().unwrap().1.push_str(&text);
            }
            Event::Code(text) if current.is_some() => {
                // Inline code inside a heading — treat as part of the
                // label. `## Project \`foo\`` looks up as `Project foo`.
                current.as_mut().unwrap().1.push_str(&text);
            }
            _ => {}
        }
    }

    // Second pass: fill in `content_range` for each heading. The
    // section ends at the first subsequent heading of equal-or-higher
    // level, or at the end of the body if there is none.
    let body_len = body.len();
    for i in 0..spans.len() {
        let level = spans[i].level;
        let content_start = spans[i].heading_range.end;
        let content_end = spans
            .iter()
            .skip(i + 1)
            .find(|later| later.level <= level)
            .map(|later| later.heading_range.start)
            .unwrap_or(body_len);
        spans[i].content_range = content_start..content_end;
    }

    spans
}

fn level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Extract hard-deadline milestones from a `## Milestones`-style
/// section body.
///
/// Matches lines of the form `- [ ] <title> — hard: YYYY-MM-DD`
/// (em-dash or plain hyphen before `hard:` are both accepted).
/// Checked list items (`- [x]`) are skipped — they represent
/// completed milestones, not pending commitments. Soft milestones
/// (`target: …`) are skipped because only hard deadlines flow into
/// the commitments aggregation per `docs/design.md §5.3`.
///
/// The date must be a valid calendar date in `YYYY-MM-DD` form; any
/// other shape, or an impossible date like `2026-02-30`, is silently
/// dropped so the commitments query never sees a ghost.
///
/// Returns pairs of `(title, date_string)` in the order encountered
/// in the source.
pub fn extract_hard_deadlines(section: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in section.lines() {
        if let Some(deadline) = parse_hard_deadline_line(line) {
            out.push(deadline);
        }
    }
    out
}

/// Parse one line of a milestones section. Returns the title and
/// validated date on success; returns `None` for any non-matching
/// shape (including completed items, soft targets, malformed dates,
/// and lines that aren't list items at all).
fn parse_hard_deadline_line(line: &str) -> Option<(String, String)> {
    // List-item prefix — we only consider unchecked bullets.
    // Checking `- [x]` explicitly skips completed milestones.
    let after_marker = line.trim_start().strip_prefix("- [ ] ")?;

    // Must contain `hard:`; split there. Anything to the left is the
    // candidate title (with a trailing separator to trim), anything
    // to the right should start with a YYYY-MM-DD date.
    let hard_idx = after_marker.find("hard:")?;
    let (before, rest) = after_marker.split_at(hard_idx);
    let after_hard = rest.strip_prefix("hard:")?.trim_start();

    let date = parse_iso_date_prefix(after_hard)?;

    // The title is whatever precedes `hard:` with trailing whitespace
    // and em-dash / hyphen separators trimmed. Empty titles are
    // rejected — a deadline without a label is useless.
    let title = before
        .trim_end_matches(|c: char| c.is_whitespace() || c == '-' || c == '—')
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }

    Some((title, date))
}

/// Read a `YYYY-MM-DD` date at the start of `s` and validate it as a
/// real calendar date. Returns the 10-character slice on success.
fn parse_iso_date_prefix(s: &str) -> Option<String> {
    if s.len() < 10 {
        return None;
    }
    let candidate = &s[..10];
    // `NaiveDate` rejects impossible dates like 2026-02-30, giving us
    // calendar validation for free alongside the format check.
    NaiveDate::parse_from_str(candidate, "%Y-%m-%d").ok()?;
    Some(candidate.to_owned())
}

/// Extract every milestone from a `## Milestones`-style section body.
///
/// Broader than [`extract_hard_deadlines`] — it captures the full
/// project timeline (design §5.3), not just the hard deadlines that
/// flow into commitments:
///
/// - `- [ ] <name> — hard: YYYY-MM-DD` → pending, hard, dated
/// - `- [ ] <name> — target: <date|marker>` → pending, soft; dated only
///   when the marker is a valid ISO date (`target: TBD` parses to no date)
/// - `- [x] <name> — YYYY-MM-DD` → completed, soft, dated
///
/// The checkbox sets `completed`; `hard:` sets `is_hard`. Any line in
/// the section that is a checklist item with a non-empty name becomes a
/// milestone — an undated milestone (no parseable date) is valid, it
/// just can't appear in [`MilestoneEntry`]-by-date queries. Lines that
/// aren't `- [ ]` / `- [x]` checklist items are ignored.
///
/// Returns entries in source order.
pub fn extract_milestones_from_body(section: &str) -> Vec<MilestoneEntry> {
    section.lines().filter_map(parse_milestone_line).collect()
}

/// Parse one milestone line. `None` for lines that aren't checklist
/// items or that have an empty name.
fn parse_milestone_line(line: &str) -> Option<MilestoneEntry> {
    let trimmed = line.trim_start();
    let (completed, rest) = if let Some(r) = trimmed.strip_prefix("- [ ] ") {
        (false, r)
    } else if let Some(r) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("- [X] "))
    {
        (true, r)
    } else {
        return None;
    };

    // A `hard:` or `target:` keyword splits name from date marker. The
    // date is `None` when the marker isn't a valid ISO date (e.g.
    // `target: April`). With no keyword, the line is a plain completed
    // marker whose date, if any, sits at the end (`<name> — YYYY-MM-DD`).
    let (name, date, is_hard) = if let Some(idx) = rest.find("hard:") {
        let after = rest[idx + "hard:".len()..].trim_start();
        (
            trim_milestone_name(&rest[..idx]),
            parse_iso_date_prefix(after),
            true,
        )
    } else if let Some(idx) = rest.find("target:") {
        let after = rest[idx + "target:".len()..].trim_start();
        (
            trim_milestone_name(&rest[..idx]),
            parse_iso_date_prefix(after),
            false,
        )
    } else {
        let (name, date) = split_trailing_iso_date(rest);
        (name, date, false)
    };

    if name.is_empty() {
        return None;
    }
    Some(MilestoneEntry {
        name,
        date,
        is_hard,
        completed,
    })
}

/// Trim trailing separators (whitespace, hyphen, em-dash) and
/// surrounding whitespace from a candidate milestone name.
fn trim_milestone_name(s: &str) -> String {
    s.trim_end_matches(|c: char| c.is_whitespace() || c == '-' || c == '\u{2014}')
        .trim()
        .to_owned()
}

/// Split a keyword-less milestone line into its name and an optional
/// trailing ISO date: `Baseline trained — 2026-02-10` → ("Baseline
/// trained", Some("2026-02-10")). The 10-char tail is only treated as
/// a date when it is itself a valid date *and* preceded by a separator
/// (or the whole remainder), so a date embedded mid-name isn't sliced.
fn split_trailing_iso_date(s: &str) -> (String, Option<String>) {
    let trimmed = s.trim_end();
    if trimmed.len() >= 10 {
        let (before, candidate) = trimmed.split_at(trimmed.len() - 10);
        let separated = before.is_empty()
            || before.ends_with(|c: char| c.is_whitespace() || c == '-' || c == '\u{2014}');
        if separated && parse_iso_date_prefix(candidate).is_some() {
            let name = trim_milestone_name(before);
            if !name.is_empty() {
                return (name, Some(candidate.to_owned()));
            }
        }
    }
    (trimmed.trim().to_owned(), None)
}

/// One GFM pipe table lifted out of a markdown body: the header cells
/// and the data rows, all as trimmed strings. Consumers decide what
/// the cells mean — e.g. the tracking-series query parses numeric
/// columns out of the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownTable {
    pub headers: Vec<String>,
    /// Data rows in source order. Row widths follow the source — a
    /// hand-edited row with a missing cell is shorter than `headers`,
    /// so consumers index with `get`, not `[]`.
    pub rows: Vec<Vec<String>>,
}

/// Extract the first GFM pipe table from a markdown body, or `None`
/// when the body has no table.
///
/// A table is the first run of consecutive `|`-prefixed lines whose
/// second line is a delimiter row (`| --- | :---: | ... |`). Rows
/// after the delimiter become data rows until the first line that no
/// longer starts with `|`. This is a line scanner in the same spirit
/// as the other extractors in this module — it handles the tables our
/// templates write. Known limits: it does not track ``` fences (a
/// pipe table inside a code block would be matched first), and an
/// escaped `\|` still splits the cell — acceptable for tracking-note
/// bodies, which the templates keep fence-free and plain.
pub fn extract_first_table(body: &str) -> Option<MarkdownTable> {
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        if !is_table_line(line) {
            continue;
        }
        // Candidate header — a table only starts here if the next
        // line is a delimiter row. No next line at all means no table
        // can follow, so `?` ends the whole scan.
        let next = lines.peek()?;
        if !is_delimiter_row(next) {
            continue;
        }
        lines.next(); // consume the delimiter row
        let headers = split_table_cells(line);
        let mut rows = Vec::new();
        for row_line in lines {
            if !is_table_line(row_line) {
                break;
            }
            rows.push(split_table_cells(row_line));
        }
        return Some(MarkdownTable { headers, rows });
    }
    None
}

/// A line participates in a pipe table when its first non-blank
/// character is `|`. GFM also allows tables without outer pipes, but
/// every table our templates emit uses them — keep the scanner strict
/// so prose containing a stray `a | b` is never misread as a table.
fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A GFM delimiter row: every cell is dashes with optional leading /
/// trailing colons (`---`, `:--`, `:-:`), at least one dash, nothing
/// else. This is what distinguishes a real table from two adjacent
/// lines that merely start with `|`.
fn is_delimiter_row(line: &str) -> bool {
    if !is_table_line(line) {
        return false;
    }
    let cells = split_table_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let inner = cell.trim_start_matches(':').trim_end_matches(':');
            !inner.is_empty() && inner.chars().all(|c| c == '-')
        })
}

/// Split one table line into trimmed cell strings, dropping the empty
/// fragments produced by the outer pipes: `| a | b |` → `["a", "b"]`.
/// Interior empty cells (`| a |  | c |`) survive as empty strings —
/// only the outermost two fragments are structural.
fn split_table_cells(line: &str) -> Vec<String> {
    let mut inner = line.trim();
    inner = inner.strip_prefix('|').unwrap_or(inner);
    inner = inner.strip_suffix('|').unwrap_or(inner);
    inner
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}
