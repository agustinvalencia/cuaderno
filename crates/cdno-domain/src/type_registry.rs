//! The note-type registry: the single place that answers "is this type
//! known, and what are its schema rules?" across built-in and user-defined
//! (config) types.
//!
//! Note types are a closed Rust enum ([`NoteType`]) that carries *behaviour*
//! (caps, lifecycle, aggregation) via typed frontmatter structs. Users can
//! also declare **schema-only** types under `[note_types.<name>]` in
//! `config.toml` — these get a folder, field rules, a template, and generic
//! linting/normalisation, but never bespoke behaviour (a runtime type can't
//! be pattern-matched by the domain).
//!
//! The registry unifies the two behind one lookup so the three domain choke
//! points that used to gate on the enum — lint, normalise, and template
//! placeholders — consult a single source of truth. It is a cheap borrowing
//! view over the vault's [`VaultConfig`]; validation runs once at vault-open
//! via [`TypeRegistry::validate`].

use std::str::FromStr;

use cdno_core::config::{CustomNoteType, VaultConfig};

use crate::error::DomainError;
use crate::note_type::NoteType;

/// A resolved note type — either one of the built-in [`NoteType`] variants or a
/// user-defined config type. Borrows from the [`VaultConfig`] it was resolved
/// against.
#[derive(Debug, Clone, Copy)]
pub enum NoteTypeDescriptor<'a> {
    /// A built-in type carrying real behaviour.
    Builtin(NoteType),
    /// A schema-only config type declared under `[note_types.<name>]`.
    Custom {
        name: &'a str,
        def: &'a CustomNoteType,
    },
}

impl<'a> NoteTypeDescriptor<'a> {
    /// Whether this is a config-defined (schema-only) type.
    pub fn is_custom(&self) -> bool {
        matches!(self, NoteTypeDescriptor::Custom { .. })
    }

    /// The vault-relative folder a custom type's notes live in. `None` for
    /// built-in types (whose folder placement is bespoke per create path).
    pub fn folder(&self) -> Option<&'a str> {
        match self {
            NoteTypeDescriptor::Builtin(_) => None,
            NoteTypeDescriptor::Custom { def, .. } => Some(&def.folder),
        }
    }

    /// Whether this type is append-only. Built-ins encode this per variant
    /// elsewhere; config types carry an explicit flag (enforcement deferred).
    pub fn append_only(&self) -> bool {
        match self {
            NoteTypeDescriptor::Builtin(_) => false,
            NoteTypeDescriptor::Custom { def, .. } => def.append_only,
        }
    }

    /// The frontmatter fields that must be present and non-null. For a built-in
    /// type these are the vault's `[schemas.<type>].extra_required` additions;
    /// for a config type they are its declared `required` fields. (Built-in
    /// types' *intrinsic* required fields are enforced by their typed
    /// `TryFrom<Frontmatter>` parse, not here.)
    pub fn required_fields(&self, config: &'a VaultConfig) -> &'a [String] {
        match self {
            NoteTypeDescriptor::Builtin(nt) => config.extra_required_fields(nt.as_str()),
            NoteTypeDescriptor::Custom { def, .. } => &def.required,
        }
    }

    /// The canonical frontmatter key order for a config type: `type` first,
    /// then the declared required fields, then the optional ones (dropping
    /// duplicates while preserving first-seen order). Built-in types derive
    /// their order from the effective template instead, so this returns `None`
    /// for them — the caller keeps the template-derived path.
    pub fn custom_frontmatter_order(&self) -> Option<Vec<String>> {
        match self {
            NoteTypeDescriptor::Builtin(_) => None,
            NoteTypeDescriptor::Custom { def, .. } => {
                let mut order: Vec<String> = Vec::new();
                let mut push = |k: &str| {
                    if !order.iter().any(|existing| existing == k) {
                        order.push(k.to_owned());
                    }
                };
                push("type");
                for f in &def.required {
                    push(f);
                }
                for f in &def.optional {
                    push(f);
                }
                Some(order)
            }
        }
    }
}

/// A borrowing view over the vault's note-type declarations. Built-in types
/// (from [`NoteType::ALL`]) take precedence; config types fill the rest.
#[derive(Debug, Clone, Copy)]
pub struct TypeRegistry<'a> {
    config: &'a VaultConfig,
}

impl<'a> TypeRegistry<'a> {
    /// Wrap a config as a registry. Cheap (borrows only); assumes the config
    /// has already passed [`TypeRegistry::validate`] at vault-open.
    pub fn new(config: &'a VaultConfig) -> Self {
        Self { config }
    }

    /// Validate a config's `[note_types.*]` table: the core structural checks
    /// plus the reserved-name check (a custom type may not shadow a built-in).
    /// Run once at [`Vault::new`](crate::vault::Vault::new) so a bad
    /// declaration fails at vault-open rather than mid-operation.
    pub fn validate(config: &VaultConfig) -> Result<(), DomainError> {
        config.validate_note_types()?;
        for name in config.note_types.keys() {
            // Case-insensitive: `from_str` is exact-match, so `[note_types.Project]`
            // would otherwise slip past and resolve as a *distinct* type from the
            // lowercase `project` every tool writes — a silent divergence. Reject
            // any case-variant of a built-in name.
            if NoteType::ALL
                .iter()
                .any(|t| t.as_str().eq_ignore_ascii_case(name))
            {
                return Err(DomainError::ReservedTypeName { name: name.clone() });
            }
        }
        Ok(())
    }

    /// Resolve a `type` string to its descriptor: built-in first, then config.
    /// `None` means the type is unknown — the caller decides whether that is an
    /// error (lint) or a skip (normalise).
    pub fn resolve(&self, note_type: &str) -> Option<NoteTypeDescriptor<'a>> {
        if let Ok(nt) = NoteType::from_str(note_type) {
            return Some(NoteTypeDescriptor::Builtin(nt));
        }
        // `get_key_value` borrows both the name and def from the map, so the
        // descriptor lives as long as the config (`'a`), not this call.
        self.config
            .note_types
            .get_key_value(note_type)
            .map(|(name, def)| NoteTypeDescriptor::Custom {
                name: name.as_str(),
                def,
            })
    }

    /// Whether `note_type` is a known type (built-in or config-defined).
    pub fn is_known(&self, note_type: &str) -> bool {
        self.resolve(note_type).is_some()
    }

    /// Every known type name — the 11 built-ins plus every config type — for
    /// shell completions and the `--type` filter. Built-ins first (in
    /// [`NoteType::ALL`] order), then config types sorted alphabetically so the
    /// completion list is stable across runs (the config map is unordered).
    pub fn all_names(&self) -> Vec<&'a str> {
        let mut names: Vec<&str> = NoteType::ALL.iter().map(|t| t.as_str()).collect();
        let mut custom: Vec<&str> = self.config.note_types.keys().map(String::as_str).collect();
        custom.sort_unstable();
        names.extend(custom);
        names
    }
}
