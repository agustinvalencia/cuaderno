//! Life-domain classification for projects, stewardships, and
//! commitments. Canonical and closed — see `docs/design.md` §5.10.
//!
//! Lives at the frontmatter-module top level rather than under any
//! single note type because it's shared: project, commitment, and
//! (later) stewardship frontmatter all carry a `context:` field.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub enum Context {
    Work,
    SideProject,
    University,
    Family,
    Household,
    Legal,
    Personal,
}

impl Context {
    /// Every variant in declaration order — used by [`FromStr`] and
    /// any future "iterate every context" need.
    pub const ALL: [Context; 7] = [
        Context::Work,
        Context::SideProject,
        Context::University,
        Context::Family,
        Context::Household,
        Context::Legal,
        Context::Personal,
    ];

    /// Kebab-case YAML / CLI form. Mirrors the `#[serde(rename_all =
    /// "kebab-case")]` projection used for serialisation, but exposed
    /// directly so write paths don't have to round-trip through
    /// `serde_yaml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Context::Work => "work",
            Context::SideProject => "side-project",
            Context::University => "university",
            Context::Family => "family",
            Context::Household => "household",
            Context::Legal => "legal",
            Context::Personal => "personal",
        }
    }
}

/// Error returned when a string does not match any [`Context`] variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown context: {0}")]
pub struct ParseContextError(pub String);

impl FromStr for Context {
    type Err = ParseContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Context::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseContextError(s.to_owned()))
    }
}
