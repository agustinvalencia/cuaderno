use std::fmt;

/// Contract for note types. Implemented in the domain layer.
///
/// Core uses this to load templates and validate type names
/// without knowing the actual set of note types.
pub trait NoteType: fmt::Display + Copy {
    fn as_str(&self) -> &str;
    fn all_variants() -> &'static [Self];
}
