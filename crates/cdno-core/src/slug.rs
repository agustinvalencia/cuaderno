//! The shape rule for a slug the person typed (RFC 0007 §5.7), and for the
//! `aliases:` entries the index reads (`VaultIndex::list_aliases`). It lives
//! here so the validator and the index cannot disagree about what a slug is.

use crate::frontmatter::yaml_scalar_is_string;

/// Hard char cap on a slug.
pub const SLUG_MAX_CHARS: usize = 50;

/// Why a slug the person typed is refused (RFC 0007 §5.7). Each variant
/// names one rule, in the order [`validate_explicit_slug`] checks them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlugRejection {
    #[error("it is empty")]
    Empty,
    #[error("it has upper-case letters; use lower case")]
    UpperCase,
    #[error("'{0}' is not allowed; use only a-z, 0-9 and '-'")]
    BadChar(char),
    #[error("it starts or ends with '-'; put letters or digits at both ends")]
    LeadingOrTrailingDash,
    #[error("it has '--' in it; join words with a single '-'")]
    DoubleDash,
    #[error("it is longer than {max} characters")]
    TooLong { max: usize },
    #[error(
        "YAML would read it as a number, a date or true/false/null rather than text; add a word to it"
    )]
    YamlTyped,
}

/// The shape every slug has, however it was made: non-empty, at most
/// [`SLUG_MAX_CHARS`] chars, no leading or trailing `-`, no `--`, no upper
/// case. The charset is the one part that differs: with `ascii`, lowercase
/// ASCII letters and digits (what a person may type, RFC 0007 §5.7); without,
/// any alphanumeric char, which is what `slugify` keeps (`über-model`).
/// It says nothing about YAML: `no`, `007` and `2026` have this shape.
fn check_shape(slug: &str, ascii: bool) -> Result<(), SlugRejection> {
    if slug.is_empty() {
        return Err(SlugRejection::Empty);
    }
    let upper = |c: char| {
        if ascii {
            c.is_uppercase()
        } else {
            c.is_ascii_uppercase()
        }
    };
    if slug.chars().any(upper) {
        return Err(SlugRejection::UpperCase);
    }
    let allowed = |c: char| {
        c == '-'
            || if ascii {
                c.is_ascii_lowercase() || c.is_ascii_digit()
            } else {
                c.is_alphanumeric()
            }
    };
    if let Some(c) = slug.chars().find(|c| !allowed(*c)) {
        return Err(SlugRejection::BadChar(c));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(SlugRejection::LeadingOrTrailingDash);
    }
    if slug.contains("--") {
        return Err(SlugRejection::DoubleDash);
    }
    if slug.chars().count() > SLUG_MAX_CHARS {
        return Err(SlugRejection::TooLong {
            max: SLUG_MAX_CHARS,
        });
    }
    Ok(())
}

/// Check a slug the person typed, as opposed to one derived from a title:
/// the shape rule restricted to lowercase ASCII (see [`is_slug_shaped`]) and, on top of it, not a value the
/// repo's YAML parser reads back as something other than a string (the slug
/// lands in frontmatter and wikilinks), nor one of the extra over-rejections
/// below. So everything this accepts is slug-shaped, not the reverse.
/// Pure: whether the name is already taken is the domain's job.
pub fn validate_explicit_slug(slug: &str) -> Result<(), SlugRejection> {
    check_shape(slug, true)?;
    if is_yaml_typed(slug) {
        return Err(SlugRejection::YamlTyped);
    }
    Ok(())
}

/// Whether a charset-valid slug would not survive as text in frontmatter:
/// the repo's YAML parser reads it as a number, bool or null (asked of the
/// parser itself, [`yaml_scalar_is_string`]), or it is one of the YAML 1.1
/// bool words, an ISO date or an all-digit run (`007`, octal to a YAML 1.1
/// reader), which that parser reads as text but other tools do not. The
/// extras are deliberate over-rejection.
fn is_yaml_typed(slug: &str) -> bool {
    if !yaml_scalar_is_string(slug) {
        return true;
    }
    if matches!(slug, "yes" | "no" | "on" | "off") {
        return true;
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if digits(slug) {
        return true;
    }
    // `2026-11-05`: a year, a month and a day, all digits.
    let parts: Vec<&str> = slug.split('-').collect();
    parts.len() == 3 && parts[0].len() == 4 && parts.iter().all(|p| digits(p))
}

/// Whether `text` has the shape of a slug: non-empty, at most
/// [`SLUG_MAX_CHARS`] chars, every char alphanumeric or `-`, no leading or
/// trailing `-`, no `--`, no ASCII upper case. Everything `slugify` can
/// produce passes, including non-ASCII ones such as `über-model`, so an old
/// slug survives as an alias; the validator is this rule narrowed to ASCII
/// plus the YAML over-rejections, so `no`, `007` and `2026` are slug-shaped
/// though a person may not type them as a new slug.
pub fn is_slug_shaped(text: &str) -> bool {
    check_shape(text, false).is_ok()
}
