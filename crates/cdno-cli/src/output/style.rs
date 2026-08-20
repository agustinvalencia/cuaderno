//! Colour for human-readable CLI output: what may be painted, and
//! whether painting happens at all.
//!
//! Two ideas carry this module.
//!
//! **Roles, not colours.** Callers ask for [`Role::Slug`], never for
//! "bold cyan". Every colour decision therefore lives in the private
//! `palette` function below and nowhere else, so the house style is
//! retuned in one place rather than re-derived at thirty call sites.
//!
//! **One gate, decided once.** [`init`] records the `--color` choice
//! before any command runs; [`colour_enabled`] answers from it. When
//! colour is off, [`Palette::paint`] returns its input *unchanged* —
//! byte-for-byte, not "visually equivalent". That is what lets the whole
//! existing test surface keep asserting on plain substrings: integration
//! tests and `assert_cmd` subprocesses both run off a tty, so they see
//! exactly the text they saw before this module existed.
//!
//! The precedence ladder (`NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, tty)
//! is not re-implemented here — `anstyle-query` already encodes it, and
//! `colorchoice` already provides the process-wide slot to hold the
//! flag's answer. Both build as part of clap's `color` feature, so
//! neither is a new crate.

use std::io::IsTerminal;

use anstyle::{AnsiColor, Style};
use cdno_domain::frontmatter::{Context, QuestionDomain};

/// The `--color` flag's three settings.
///
/// Distinct from [`colorchoice::ColorChoice`] so clap's `ValueEnum`
/// derive can hang off it without a newtype; [`init`] converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColourChoice {
    /// Colour only when stdout is a terminal, honouring `NO_COLOR`,
    /// `CLICOLOR`, and `CLICOLOR_FORCE`.
    #[default]
    Auto,
    /// Always colour, even when redirected — for piping into a pager.
    Always,
    /// Never colour.
    Never,
}

impl From<ColourChoice> for colorchoice::ColorChoice {
    fn from(choice: ColourChoice) -> Self {
        match choice {
            ColourChoice::Auto => colorchoice::ColorChoice::Auto,
            ColourChoice::Always => colorchoice::ColorChoice::Always,
            ColourChoice::Never => colorchoice::ColorChoice::Never,
        }
    }
}

/// Record the `--color` choice for the rest of the process. Call once
/// from `main`, straight after argument parsing and before any command
/// runs — this is the single point where colour is decided.
pub fn init(choice: ColourChoice) {
    colorchoice::ColorChoice::from(choice).write_global();
}

/// Whether human-readable output should carry ANSI styling.
///
/// `Auto` defers to `anstyle-query` for the environment ladder and to
/// `is_terminal` for the rest. `NO_COLOR` is checked first and wins over
/// `CLICOLOR_FORCE`: a user exports `NO_COLOR` once, globally, as a
/// preference about their own terminal, whereas `CLICOLOR_FORCE` is
/// typically set by a harness that has no standing to override it.
pub fn colour_enabled() -> bool {
    match colorchoice::ColorChoice::global() {
        colorchoice::ColorChoice::Never => false,
        colorchoice::ColorChoice::Always | colorchoice::ColorChoice::AlwaysAnsi => true,
        colorchoice::ColorChoice::Auto => {
            !anstyle_query::no_color()
                && anstyle_query::clicolor().unwrap_or(true)
                && (anstyle_query::clicolor_force() || std::io::stdout().is_terminal())
        }
    }
}

/// What a run of text *is*, which is what decides how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A slug or other identifier the reader scans for.
    Slug,
    /// A short classification beside an identifier — a context, a
    /// status, a kind.
    Badge,
    /// A section heading.
    Heading,
    /// Body text. Deliberately unstyled: prose is the baseline every
    /// other role is read against, and colouring it would flatten the
    /// contrast the other roles depend on.
    Prose,
    /// Supporting detail — dates, paths, counts, `next:` lines.
    Meta,
    /// A placeholder standing in for absent content.
    Muted,
    /// Something the reader should notice: overdue, lapsed, blocked.
    Warn,
    /// Something wrong.
    Error,
    /// Something completed.
    Success,
}

/// Days without an entry after which a dated collection reads as stale.
const STALE_AFTER_DAYS: i64 = 30;

/// A gutter colour. Named by hue rather than by meaning: at this layer
/// it *is* a colour choice, and callers own the mapping from their own
/// domain (see [`Accent::for_context`]). Keeping the names neutral lets
/// one accent serve projects, portfolios, and stewardships alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accent {
    Blue,
    Green,
    Magenta,
    Yellow,
    Cyan,
    Red,
    #[default]
    Grey,
}

impl Accent {
    /// The gutter colour for a note's context, so a list of mixed
    /// contexts is separable by colour alone before a word is read.
    pub fn for_context(context: Context) -> Self {
        match context {
            Context::Work => Accent::Blue,
            Context::SideProject => Accent::Cyan,
            Context::University => Accent::Magenta,
            Context::Family => Accent::Green,
            Context::Household => Accent::Yellow,
            Context::Legal => Accent::Red,
            Context::Personal => Accent::Grey,
        }
    }

    /// The gutter colour for a question's domain, so research and life
    /// questions stay distinguishable when the two lists sit together.
    pub fn for_question(domain: QuestionDomain) -> Self {
        match domain {
            QuestionDomain::Research => Accent::Cyan,
            QuestionDomain::Life => Accent::Green,
        }
    }

    /// The gutter colour for a dated collection — a portfolio's evidence,
    /// a stewardship's tracking notes.
    ///
    /// One function rather than a conditional at each call site: the two
    /// listings were picking their own hues for the same idea, so a
    /// healthy portfolio was cyan and a healthy stewardship green, for
    /// no reason a reader could infer.
    pub fn for_staleness(count: usize, days: Option<i64>) -> Self {
        match (count, days) {
            (0, _) => Accent::Grey,
            // A standing commitment nobody has fed in a month is what
            // these listings exist to surface.
            (_, Some(d)) if d > STALE_AFTER_DAYS => Accent::Yellow,
            _ => Accent::Green,
        }
    }

    fn style(self) -> Style {
        let colour = match self {
            Accent::Blue => AnsiColor::Blue,
            Accent::Green => AnsiColor::Green,
            Accent::Magenta => AnsiColor::Magenta,
            Accent::Yellow => AnsiColor::Yellow,
            Accent::Cyan => AnsiColor::Cyan,
            Accent::Red => AnsiColor::Red,
            Accent::Grey => return Style::new().dimmed(),
        };
        Style::new().fg_color(Some(colour.into()))
    }
}

/// The house style, in one place. Every colour the CLI emits is here.
fn palette(role: Role) -> Style {
    match role {
        Role::Slug => Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
        Role::Badge => Style::new()
            .dimmed()
            .fg_color(Some(AnsiColor::Magenta.into())),
        Role::Heading => Style::new().bold(),
        Role::Prose => Style::new(),
        Role::Meta | Role::Muted => Style::new().dimmed(),
        Role::Warn => Style::new().fg_color(Some(AnsiColor::Yellow.into())),
        Role::Error => Style::new().bold().fg_color(Some(AnsiColor::Red.into())),
        Role::Success => Style::new().fg_color(Some(AnsiColor::Green.into())),
    }
}

/// Whether a given render is painting, carried explicitly so pure
/// renderers stay pure.
///
/// The process gate is set once and never changes, which makes it
/// useless for testing a styled path from a test binary that runs off a
/// tty. Passing the palette in as a value instead lets a test render the
/// *same* function both ways, in parallel, touching no global and no
/// environment variable.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    enabled: bool,
}

impl Palette {
    /// The palette the process is configured for.
    pub fn active() -> Self {
        Palette {
            enabled: colour_enabled(),
        }
    }

    /// A palette that never paints.
    pub fn plain() -> Self {
        Palette { enabled: false }
    }

    /// A palette that always paints, whatever the terminal thinks.
    pub fn forced() -> Self {
        Palette { enabled: true }
    }

    /// Paint `text` in `role`'s style, or return it untouched when this
    /// palette is off.
    ///
    /// The disabled arm allocates rather than borrowing so callers have
    /// one return type to handle; the point is that the *bytes* are
    /// identical, not that the allocation is avoided.
    pub fn paint(self, role: Role, text: &str) -> String {
        self.wrap(palette(role), text)
    }

    /// Paint `text` in `accent`'s gutter colour.
    pub fn paint_accent(self, accent: Accent, text: &str) -> String {
        self.wrap(accent.style(), text)
    }

    fn wrap(self, style: Style, text: &str) -> String {
        if !self.enabled || style == Style::new() {
            return text.to_owned();
        }
        format!("{}{text}{}", style.render(), style.render_reset())
    }
}

/// A table cell painted for `role`.
///
/// Tables take this route rather than [`Palette::paint`] because
/// comfy-table measures column widths from the raw cell text: handing it
/// a string with escape bytes already in it would make every styled
/// column believe it is several characters wider than it looks. Styling
/// the `Cell` instead lets comfy-table measure the content and emit the
/// escapes itself, at render time.
///
/// The colours are *derived* from [`palette`] rather than restated, so a
/// role cannot mean one thing on a card and another in a table. It did
/// exactly that before: `Role::Slug` came out as SGR 36 through anstyle
/// and SGR 38;5;14 through comfy-table, because crossterm's `Color::Cyan`
/// is the bright variant and anstyle's is the normal one — so the same
/// slug was two different cyans depending on which renderer drew it.
///
/// Only base-16 foregrounds and the bold/dim effects are carried across,
/// which is everything [`palette`] uses. If a role ever gains a truecolor
/// foreground or another effect, it will render on cards and not in
/// tables — the two tests below compare the paths per role and would
/// catch it.
///
/// There is no palette parameter and no gate here: [`super::styled_table`]
/// has already told the table whether to emit styling at all, via
/// `enforce_styling` / `force_no_tty`. A cell built here is therefore
/// safe to use unconditionally.
pub fn cell(role: Role, text: impl Into<String>) -> comfy_table::Cell {
    use comfy_table::{Attribute, Cell};

    let style = palette(role);
    let mut cell = Cell::new(text.into());
    if let Some(anstyle::Color::Ansi(colour)) = style.get_fg_color() {
        cell = cell.fg(to_comfy(colour));
    }
    let effects = style.get_effects();
    if effects.contains(anstyle::Effects::BOLD) {
        cell = cell.add_attribute(Attribute::Bold);
    }
    if effects.contains(anstyle::Effects::DIMMED) {
        cell = cell.add_attribute(Attribute::Dim);
    }
    cell
}

/// Map an `anstyle` base-16 colour onto comfy-table's equivalent.
///
/// The `Dark*` names are not a typo. crossterm — which comfy-table
/// renders through — calls the *normal* 30-37 range `DarkX` and reserves
/// the plain name for the bright 90-97 range, the opposite of anstyle's
/// convention. Mapping name-to-name would silently brighten every
/// colour on the table path.
fn to_comfy(colour: anstyle::AnsiColor) -> comfy_table::Color {
    use anstyle::AnsiColor;
    use comfy_table::Color;
    match colour {
        AnsiColor::Black => Color::Black,
        AnsiColor::Red => Color::DarkRed,
        AnsiColor::Green => Color::DarkGreen,
        AnsiColor::Yellow => Color::DarkYellow,
        AnsiColor::Blue => Color::DarkBlue,
        AnsiColor::Magenta => Color::DarkMagenta,
        AnsiColor::Cyan => Color::DarkCyan,
        AnsiColor::White => Color::Grey,
        AnsiColor::BrightBlack => Color::DarkGrey,
        AnsiColor::BrightRed => Color::Red,
        AnsiColor::BrightGreen => Color::Green,
        AnsiColor::BrightYellow => Color::Yellow,
        AnsiColor::BrightBlue => Color::Blue,
        AnsiColor::BrightMagenta => Color::Magenta,
        AnsiColor::BrightCyan => Color::Cyan,
        AnsiColor::BrightWhite => Color::White,
    }
}
