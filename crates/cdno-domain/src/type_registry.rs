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

    /// The underlying [`CustomNoteType`] if this is a config type, else `None`
    /// (built-in types have bespoke create paths, not a config declaration).
    pub fn as_custom(&self) -> Option<&'a CustomNoteType> {
        match self {
            NoteTypeDescriptor::Custom { def, .. } => Some(def),
            NoteTypeDescriptor::Builtin(_) => None,
        }
    }

    /// The `{{placeholders}}` this type's create path supplies — what a custom
    /// template for it may reference. For a built-in, its static registry list
    /// ([`NoteType::supplied_placeholders`]). For a config type, the create-path
    /// built-ins (`title`, `slug`, `created`, `date`) plus its declared
    /// `required`/`optional` fields, de-duplicated in a stable order.
    pub fn supplied_placeholders(&self) -> Vec<String> {
        match self {
            NoteTypeDescriptor::Builtin(nt) => nt
                .supplied_placeholders()
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            NoteTypeDescriptor::Custom { def, .. } => {
                let mut names: Vec<String> = ["title", "slug", "created", "date"]
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect();
                for f in def.required.iter().chain(def.optional.iter()) {
                    if !names.contains(f) {
                        names.push(f.clone());
                    }
                }
                names
            }
        }
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

    /// Validate a config's `[note_types.*]` and `[schemas.*]` tables: the core
    /// structural checks (which need no built-in-type knowledge) plus the two
    /// checks that do — the reserved-name check (a custom type may not shadow a
    /// built-in) and the reserved-engine-field check (a built-in's schema field
    /// may not redeclare an engine-owned key). Run once at
    /// [`Vault::new`](crate::vault::Vault::new) so a bad declaration fails at
    /// vault-open rather than mid-operation.
    pub fn validate(config: &VaultConfig) -> Result<(), DomainError> {
        config.validate_note_types()?;
        config.validate_schemas()?;
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
        Self::validate_reserved_schema_fields(config)?;
        Ok(())
    }

    /// Reject (or warn on) a `[schemas.<builtin>.fields.<name>]` that collides
    /// with a key the engine owns for that built-in type (`#301`).
    ///
    /// The reserved set is **derived** from each type's metadata, never a
    /// hardcoded global name list (which would drift and miss per-type keys):
    /// - **Hard block** `type` (every note carries it, engine-written) and the
    ///   type's own date/period identity key — `date` for daily, `week`/`month`
    ///   for weekly/monthly. That key is read from
    ///   [`NoteType::frontmatter_order`] (position 1 for the calendar types,
    ///   which are exactly the types the engine keys and scaffolds by period),
    ///   so the *name* is derived, not hardcoded.
    /// - **Warn** (don't block) on a field colliding with any other supplied
    ///   placeholder ([`NoteType::supplied_placeholders`]): the value tiering
    ///   already shadows engine-owned defaults, so this is footgun-prevention
    ///   with a good message, not a correctness backstop.
    ///
    /// Only the explicit typed `fields` block is checked — `extra_required` is
    /// legacy lint-only and unaffected.
    fn validate_reserved_schema_fields(config: &VaultConfig) -> Result<(), DomainError> {
        for (type_name, schema) in &config.schemas {
            // Only built-in types have engine-owned keys; a `[schemas.<x>]` for
            // an unknown/custom name has nothing to reserve here.
            let Ok(nt) = NoteType::from_str(type_name) else {
                continue;
            };
            let hard_reserved = hard_reserved_fields(nt);
            for field_name in schema.fields.keys() {
                if hard_reserved.contains(&field_name.as_str()) {
                    return Err(DomainError::ReservedSchemaField {
                        note_type: type_name.clone(),
                        field: field_name.clone(),
                    });
                }
                if nt.supplied_placeholders().contains(&field_name.as_str()) {
                    tracing::warn!(
                        note_type = %type_name,
                        field = %field_name,
                        "schema field shadows an engine-supplied placeholder; the \
                         engine-supplied value wins, so the declared default/value will \
                         not take effect"
                    );
                }
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

    /// Every known type name — the 12 built-ins plus every config type — for
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

/// The engine-owned keys a built-in type's schema field must never redeclare:
/// `type` (every note carries it) plus, for the calendar types, their own
/// date/period identity key. The identity key's *name* is read from
/// [`NoteType::frontmatter_order`] (position 1 for daily/weekly/monthly) rather
/// than hardcoded, so it can't drift from the real frontmatter shape.
pub(crate) fn hard_reserved_fields(nt: NoteType) -> Vec<&'static str> {
    let mut reserved = vec!["type"];
    // Daily/weekly/monthly are the types the engine keys and scaffolds by
    // period, so their period key (frontmatter_order[1]: `date`/`week`/`month`)
    // is engine-owned and must not be shadowed by a declared field.
    if matches!(nt, NoteType::Daily | NoteType::Weekly | NoteType::Monthly)
        && let Some(period_key) = nt.frontmatter_order().get(1)
    {
        reserved.push(period_key);
    }
    reserved
}
