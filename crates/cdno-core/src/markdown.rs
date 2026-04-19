use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::error::{ManipulationError, ParseError};
use crate::frontmatter::Frontmatter;

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
