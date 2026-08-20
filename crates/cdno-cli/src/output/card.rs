//! Gutter-bar cards: the house style for lists whose items carry prose.
//!
//! A table works when every cell is short and the columns mean the same
//! thing on every row. It stops working when one column holds a
//! paragraph — the reader loses the boundary between one record and the
//! next, and identifier, classification, and body all read at the same
//! weight. A card restores the boundary with a coloured gutter running
//! down the left of every line an item owns:
//!
//! ```text
//! ▎ surrogate    side-project
//! ▎ Six contributors settled; scope fixed to the solver
//! ▎ rather than the mesher…
//!
//! ▎ mesh         work
//! ▎ Coarse-mesh run validated end to end…
//! ▎ next: profile the assembly step
//! ```
//!
//! **Cards are for lists, not for detail views.** The gutter earns its
//! two columns by marking where one item ends and the next begins; a
//! `show` verb renders one record and has no boundary to mark, so it
//! keeps its plain line shape and gains only colour.
//!
//! **Wrap plain, paint last.** Every width measurement here runs over
//! unpainted text, and styling is applied to whole lines afterwards.
//! `textwrap` would happily prefix the gutter for us via
//! `Options::initial_indent`, but an indent string carrying ANSI would
//! be counted as visible columns and throw the layout off by the length
//! of an escape sequence. Wrapping narrow and prefixing afterwards keeps
//! the arithmetic honest, and means a coloured card and a plain one wrap
//! at exactly the same column.

use textwrap::core::display_width;

use super::style::{Accent, Palette, Role};

/// The gutter glyph (U+258E LEFT ONE QUARTER BLOCK) and the space after
/// it. Two display columns, subtracted from the width available to
/// every line of a card.
const GUTTER: &str = "▎";
const GUTTER_WIDTH: usize = 2;

/// Columns between the longest title in a set and the badge column.
/// Wide enough that the badge reads as a separate field rather than a
/// second word of the title, which is the confusion the card exists to
/// remove.
const BADGE_GAP: usize = 4;

/// Narrowest body we will wrap to. Below this, wrapping produces more
/// noise than it removes, so an over-narrow terminal gets overflow
/// rather than a column of single words.
const MIN_BODY_WIDTH: usize = 20;

/// One item in a card list: a title, an optional badge, and body blocks
/// that wrap under them.
#[derive(Debug, Clone)]
pub struct Card {
    title: String,
    badge: Option<String>,
    accent: Accent,
    body: Vec<(Role, String)>,
}

impl Card {
    pub fn new(title: impl Into<String>) -> Self {
        Card {
            title: title.into(),
            badge: None,
            accent: Accent::default(),
            body: Vec::new(),
        }
    }

    /// The short classification shown right of the title, aligned into a
    /// column shared by every card in the set.
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// The gutter colour. Usually [`Accent::for_context`].
    pub fn accent(mut self, accent: Accent) -> Self {
        self.accent = accent;
        self
    }

    /// A block of body text, wrapped to the available width.
    pub fn prose(self, text: impl Into<String>) -> Self {
        self.block(Role::Prose, text)
    }

    /// A supporting line — `next: …`, a path, a count.
    pub fn meta(self, text: impl Into<String>) -> Self {
        self.block(Role::Meta, text)
    }

    /// A body block that stands in for content the note doesn't have.
    pub fn muted(self, text: impl Into<String>) -> Self {
        self.block(Role::Muted, text)
    }

    /// A body block in an explicit role.
    pub fn block(mut self, role: Role, text: impl Into<String>) -> Self {
        self.body.push((role, text.into()));
        self
    }

    /// The title's display width, for computing the shared badge column.
    /// Uses `textwrap`'s own measurement so the badge column and the
    /// body wrap agree about how wide a character is.
    fn title_width(&self) -> usize {
        display_width(&self.title)
    }
}

/// Render a set of cards, separated by a blank line.
///
/// Takes the whole slice rather than one card because the badge column
/// is shared: it sits at the width of the *longest* title in the set, so
/// badges line up down the page. A per-card function could not know that.
///
/// Returns the empty string for an empty slice, mirroring the contract
/// [`super::render`] already has for an empty table. No line carries
/// trailing whitespace, and there is no trailing blank line.
pub fn render_cards(cards: &[Card], palette: &Palette, width: u16) -> String {
    if cards.is_empty() {
        return String::new();
    }

    let badge_column = cards
        .iter()
        .filter(|card| card.badge.is_some())
        .map(Card::title_width)
        .max()
        .map(|widest| widest + BADGE_GAP);

    let body_width = usize::from(width)
        .saturating_sub(GUTTER_WIDTH)
        .max(MIN_BODY_WIDTH);

    let mut out = String::new();
    for (index, card) in cards.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        render_card(&mut out, card, palette, badge_column, body_width);
    }
    out
}

fn render_card(
    out: &mut String,
    card: &Card,
    palette: &Palette,
    badge_column: Option<usize>,
    body_width: usize,
) {
    let mut header = palette.paint(Role::Slug, &card.title);
    if let (Some(badge), Some(column)) = (card.badge.as_deref(), badge_column) {
        // Pad against the *plain* title width — `header` may already
        // carry escape bytes, which are not columns on screen.
        let pad = column.saturating_sub(card.title_width()).max(1);
        header.push_str(&" ".repeat(pad));
        header.push_str(&palette.paint(Role::Badge, badge));
    }
    push_gutter_line(out, card.accent, palette, &header);

    for (role, text) in &card.body {
        for line in wrap_block(text, body_width) {
            let painted = palette.paint(*role, &line);
            push_gutter_line(out, card.accent, palette, &painted);
        }
    }
}

/// Emit one already-painted line behind the card's gutter.
///
/// A line with no content gets a bare gutter rather than `"▎ "`, so the
/// no-trailing-whitespace contract holds for blank lines inside a body
/// block as well as at the end of a card.
fn push_gutter_line(out: &mut String, accent: Accent, palette: &Palette, content: &str) {
    let bar = palette.paint_accent(accent, GUTTER);
    out.push_str(&bar);
    if !content.is_empty() {
        out.push(' ');
        out.push_str(content);
    }
    out.push('\n');
}

/// Wrap one body block to `width` display columns.
///
/// Newlines in the source are hard breaks — a project's `## Current
/// State` may already be several paragraphs, and flattening them would
/// lose structure the author put there deliberately. Each resulting
/// segment is then greedily word-wrapped. An over-long single token
/// (a URL, a long slug) is emitted whole and allowed to overflow, for
/// the same reason `no_wrap_columns` exists: an identifier broken across
/// lines is worse than one that runs past the edge.
fn wrap_block(text: &str, width: usize) -> Vec<String> {
    let options = textwrap::Options::new(width)
        .break_words(false)
        .word_splitter(textwrap::WordSplitter::NoHyphenation);

    text.split('\n')
        .flat_map(|segment| {
            let trimmed = segment.trim_end();
            if trimmed.is_empty() {
                return vec![String::new()];
            }
            textwrap::wrap(trimmed, options.clone())
                .into_iter()
                .map(|line| line.trim_end().to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}
