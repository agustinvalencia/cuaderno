use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::ConfigError;

/// Top-level vault configuration, loaded from `.cuaderno/config.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub vault: VaultMeta,
    #[serde(default)]
    pub schemas: HashMap<String, SchemaExtension>,
    /// User-defined note types, declared under `[note_types.<name>]` — see
    /// [`CustomNoteType`]. Distinct from `[schemas.*]`, which *extends* a
    /// built-in type rather than *defining* a new one.
    #[serde(default)]
    pub note_types: HashMap<String, CustomNoteType>,
    #[serde(default)]
    pub variables: Variables,
    /// Glob patterns for files to exclude from the index (and therefore
    /// from reconciliation, search, and lint). Matched against each
    /// file's vault-relative path: `*` matches within one path segment,
    /// `**` matches across segments, and a bare name like `CLAUDE.md` is
    /// anchored to the vault root — use `**/CLAUDE.md` to match at any
    /// depth. Patterns are additive only; `!`-negation / re-inclusion is
    /// not supported, and a leading `/` does not anchor (paths are
    /// already root-relative).
    ///
    /// Empty by default: nothing is ignored unless explicitly listed,
    /// since markdown is the source of truth and silently dropping a note
    /// would be data loss to retrieval. Intended for fencing off repo
    /// scaffolding that lives in the vault dir but isn't a note —
    /// `CLAUDE.md`, `README.md`. Ignoring a *real* note is supported but
    /// discouraged: it disappears from search, lint, backlinks, and the
    /// active-project count, with no per-file warning. Exclusion never
    /// deletes anything — the file stays on disk and reappears the moment
    /// the pattern is removed and the vault is reindexed.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// The `[vault]` section — basic vault metadata.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VaultMeta {
    pub name: String,
    pub max_active_projects: u8,
    /// Maximum length, in Unicode scalar values, of a project's
    /// `## Current State` section body. `0` disables the check.
    ///
    /// Current State is a *snapshot*, not a running narrative: every
    /// update auto-logs the previous body to the daily note, so the
    /// long-form history is preserved there regardless. Agent-driven
    /// updates tend to sprawl past that intent, so the cap keeps the
    /// field terse. What happens on breach is governed by
    /// [`state_overflow`](Self::state_overflow).
    pub max_state_chars: u16,
    /// What [`update_project_state`] does when a new Current State
    /// exceeds [`max_state_chars`](Self::max_state_chars).
    ///
    /// [`update_project_state`]: ../../cdno_domain/vault/struct.Vault.html
    pub state_overflow: StateOverflow,
}

impl Default for VaultMeta {
    fn default() -> Self {
        Self {
            name: String::from("My Vault"),
            max_active_projects: 5,
            max_state_chars: 500,
            // Single source of truth for the default policy: the enum's
            // `#[default]`, so the two can't drift.
            state_overflow: StateOverflow::default(),
        }
    }
}

/// How a Current State update that exceeds [`VaultMeta::max_state_chars`]
/// is handled. Serialised as a lowercase string in `config.toml`
/// (`state_overflow = "reject"`).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StateOverflow {
    /// Reject the write with an actionable error; the caller must
    /// summarise before retrying. The default — and the only level that
    /// forces a verbose *agent* to re-condense in its own loop rather
    /// than leave the noise behind.
    #[default]
    Reject,
    /// Write it anyway, but return a non-fatal advisory the CLI, MCP,
    /// and desktop surface alongside the success. A nudge, not a wall.
    Warn,
    /// No length check at all — equivalent to `max_state_chars = 0`.
    Off,
}

/// The scalar type a `[schemas.<type>.fields.<name>]` declares. Deliberately
/// small (`#301`): a link-heavy list shape is reserved via [`FieldSpec::list`]
/// rather than by adding array variants here, and an "enum" is expressed as a
/// `string` constrained by [`FieldSpec::values`] rather than a distinct type.
///
/// An unknown `type = "…"` is a hard deserialize error (serde rejects any value
/// outside these variants) — so a future `float`/`datetime` fails loudly on an
/// older `cdno` rather than being silently misparsed.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Bool,
    Int,
    String,
    Date,
}

impl FieldType {
    /// The lowercase TOML spelling, for lint/validation messages.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::Bool => "bool",
            FieldType::Int => "int",
            FieldType::String => "string",
            FieldType::Date => "date",
        }
    }
}

/// A typed frontmatter field declared under `[schemas.<type>.fields.<name>]`
/// (`#301`).
///
/// `deny_unknown_fields` turns a mistyped key (`defualt = …`) into a hard
/// parse error rather than a silently-ignored no-op — a schema typo is a
/// footgun worth failing on.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// The field's scalar type (TOML key `type`).
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// A static default value, type-checked against `ty` at load. TOML has no
    /// null, so an absent `default` means "no default" (`None`). Populating a
    /// note with this value is PR-B; PR-A only validates it.
    // `toml::Value` has no ts-rs impl, so name the wire shape by hand: a
    // scalar default serialises as a string / number / boolean, and an
    // absent one as `null` (the `Option` None arm).
    #[cfg_attr(feature = "ts-bindings", ts(type = "string | number | boolean | null"))]
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// Whether the field must be present. Only an *explicit* `required = true`
    /// will opt a field into create-time erroring; the desugared
    /// `extra_required` view keeps this `false` (lint-only).
    ///
    /// INERT through PR-B — nothing reads this yet. PR-B populates declared
    /// *defaults* at create but deliberately leaves `required` toothless,
    /// because Phase 1 has no create surface that supplies a caller value for a
    /// built-in's schema field: erroring on absence would break every create.
    /// Create-time required-enforcement lands in **Phase 2** (the
    /// `set_frontmatter` setter / `--var` supply path), together with its
    /// load-time guard "`required` without a `default` is a load error on
    /// lazily-scaffolded types (daily/weekly/monthly/inbox)" — otherwise the
    /// first `append_to_log` of a day would scaffold a daily note missing a
    /// required-no-default field and fail (the checkpoint-logging cliff). Both
    /// arrive in the same phase so the cliff cannot open in PR-B by
    /// construction.
    #[serde(default)]
    pub required: bool,
    /// An allowed-value constraint on a `string` field — the "enum" shape
    /// without a dedicated type. Rejected on a non-string field by
    /// [`VaultConfig::validate_schemas`].
    #[serde(default)]
    pub values: Option<Vec<String>>,
    /// RESERVED (`#301`): a list/array field. Parsed so the grammar is fixed
    /// now, but `list = true` is a load error ("not yet implemented") in P1 so
    /// the shape can be added source-compatibly later.
    #[serde(default)]
    pub list: Option<bool>,
    /// RESERVED for the Phase-2 setter: whether the field may be changed by
    /// `set_frontmatter`. Parsed but unused in P1.
    #[serde(default)]
    pub settable: Option<bool>,
    /// RESERVED for the Phase-2 setter: whether a change should be auto-logged
    /// to the daily note. Parsed but unused in P1.
    #[serde(default)]
    pub log_on_change: Option<bool>,
}

impl FieldSpec {
    /// The desugared spec for a bare `extra_required` entry: an untyped
    /// `string` field that is **never** create-time `required`. Folding
    /// `extra_required` into the typed field view this way keeps it lint-only —
    /// so PR-B's create-time population can't turn an existing lint warning into
    /// a note-creation failure for a vault that already uses `extra_required`.
    fn lint_only_string() -> Self {
        Self {
            ty: FieldType::String,
            default: None,
            required: false,
            values: None,
            list: None,
            settable: None,
            log_on_change: None,
        }
    }

    /// The static `default` rendered as the scalar string a template
    /// substitutes for `{{field}}` at create time (`#301` PR-B): a bool
    /// `false` → `"false"`, an int → its digits, a `string`/`date` → its text.
    /// Returns `None` when the field declares no default — the caller then
    /// supplies the absent-value convention (the built-in templates render a
    /// literal `null`, e.g. `action`'s `completed`/`blocker`).
    ///
    /// `validate_schemas` has already rejected any float/array/table or
    /// mistyped default before a note is ever created, so the non-scalar arms
    /// are unreachable in practice; they stringify defensively rather than
    /// panic.
    pub fn default_template_value(&self) -> Option<String> {
        self.default.as_ref().map(|value| match value {
            toml::Value::String(s) => s.clone(),
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::Boolean(b) => b.to_string(),
            // A `date` default is authored as a quoted `YYYY-MM-DD` string
            // (String arm above); a bare TOML date would be a Datetime, which
            // `default_mismatch` already rejects — handled here for totality.
            toml::Value::Datetime(dt) => dt.to_string(),
            other => other.to_string(),
        })
    }

    /// Type-check a frontmatter value (as the index parsed it into JSON)
    /// against this field's declared type and `values` constraint. Returns
    /// `None` when the value is acceptable, or `Some(reason)` naming the
    /// mismatch for a lint message. Callers skip `null`/absent values first:
    /// presence is a separate concern (the `required` field / the deferred
    /// undeclared-key lint), not a type mismatch.
    pub fn check_value(&self, value: &serde_json::Value) -> Option<String> {
        let type_ok = match self.ty {
            FieldType::Bool => value.is_boolean(),
            // A YAML integer parses to a JSON i64/u64; a float (`is_f64`) is not
            // an int and is rejected.
            FieldType::Int => value.is_i64() || value.is_u64(),
            FieldType::String => value.is_string(),
            // A date is carried as a `YYYY-MM-DD` string; it must both be a
            // string and parse as a calendar date.
            FieldType::Date => value
                .as_str()
                .is_some_and(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()),
        };
        if !type_ok {
            return Some(format!("is not a valid {}", self.ty.as_str()));
        }
        // `values` is a string constraint (validate_schemas rejects it on a
        // non-string field), so only check membership once the value is a
        // string of the right type.
        if let Some(allowed) = &self.values
            && let Some(s) = value.as_str()
            && !allowed.iter().any(|v| v == s)
        {
            return Some(format!("is not one of the allowed values {allowed:?}"));
        }
        None
    }
}

/// Per-type schema extension: `[schemas.<type>]`.
///
/// Adds vault-specific required fields on top of the built-in ones — either as
/// a bare name list (`extra_required`, lint-only) or as typed field specs
/// (`[schemas.<type>.fields.<name>]`, `#301`).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SchemaExtension {
    /// Bare extra-required field names. Retained for backward compatibility:
    /// lint-only and built-in-only, never a create-time error. Desugared into
    /// the [`SchemaExtension::declared_fields`] view as untyped, non-required
    /// string fields.
    #[serde(default)]
    pub extra_required: Vec<String>,
    /// Typed field declarations, keyed by field name (`#301`).
    #[serde(default)]
    pub fields: HashMap<String, FieldSpec>,
}

impl SchemaExtension {
    /// The merged typed-field view: every `extra_required` name desugared into
    /// an untyped, non-required `string` field, overlaid with the explicit
    /// `[schemas.<type>.fields]` specs. On a name collision the **explicit**
    /// block wins (it carries a real type and may set `required`); the
    /// desugared `extra_required` entry stays lint-only.
    ///
    /// This is the single source both the editor's placeholder recognition and
    /// the value-type lint layer on top of.
    pub fn declared_fields(&self) -> HashMap<String, FieldSpec> {
        let mut out: HashMap<String, FieldSpec> = HashMap::new();
        for name in &self.extra_required {
            out.insert(name.clone(), FieldSpec::lint_only_string());
        }
        // Explicit specs overwrite any desugared collision.
        for (name, spec) in &self.fields {
            out.insert(name.clone(), spec.clone());
        }
        out
    }
}

/// A user-defined note type, declared under `[note_types.<name>]`.
///
/// Schema-only by design: the reconciler already stores `type` as an opaque
/// string, so a custom type indexes, searches, and accrues backlinks like any
/// note. What this declares is the *rules* — folder, field requirements, a
/// template, ordering — that lint and normalise enforce. It carries no
/// behaviour: caps, lifecycle, and cross-type aggregation remain exclusive to
/// the built-in types (they go through typed frontmatter structs a config type
/// never satisfies).
///
/// This struct holds only what `cdno-core` can validate *structurally* (no
/// knowledge of the built-in type names lives here); the reserved-name check
/// against the built-in `NoteType` set is `cdno-domain`'s job.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomNoteType {
    /// Vault-relative folder its notes live in, e.g. `"people"`.
    pub folder: String,
    /// Frontmatter fields that must be present and non-null — lint errors on a
    /// note of this type that omits one.
    #[serde(default)]
    pub required: Vec<String>,
    /// Frontmatter fields that may be present; included in the canonical
    /// frontmatter order after the required ones.
    #[serde(default)]
    pub optional: Vec<String>,
    /// Template filename under `.cuaderno/templates/`. Defaults to `<name>.md`
    /// when omitted (resolved at create time, not here).
    #[serde(default)]
    pub template: Option<String>,
    /// Whether notes of this type are append-only. Accepted and exposed now;
    /// lint enforcement is deferred (no archival-snapshot machinery exists
    /// outside actions yet).
    #[serde(default)]
    pub append_only: bool,
    /// Frontmatter field to draw the note's display title from. When omitted,
    /// the title comes from the body's first `# H1` (matching built-in notes).
    #[serde(default)]
    pub title_field: Option<String>,
    /// Frontmatter field carrying the note's date, used by date-filtered
    /// search. When omitted, the type has no logical date.
    #[serde(default)]
    pub date_field: Option<String>,
}

/// The `[variables]` and `[variables.prompt]` sections.
///
/// Static variables are available in all templates.
/// Prompted variables trigger interactive input when unresolved.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Variables {
    #[serde(flatten)]
    pub static_vars: HashMap<String, String>,
    #[serde(default)]
    pub prompt: HashMap<String, String>,
}

impl VaultConfig {
    /// Load configuration from `.cuaderno/config.toml` within the given vault root.
    ///
    /// Returns the default config if the file does not exist.
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(vault_root: &Path) -> Result<Self, ConfigError> {
        let config_path = vault_root.join(crate::paths::CONFIG_FILE);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents =
            std::fs::read_to_string(&config_path).map_err(|source| ConfigError::Read {
                path: config_path.clone(),
                source,
            })?;

        toml::from_str(&contents).map_err(|source| ConfigError::Parse {
            path: config_path,
            source,
        })
    }

    /// Returns the schema extension for a given note type, if any.
    pub fn schema_for(&self, note_type: &str) -> Option<&SchemaExtension> {
        self.schemas.get(note_type)
    }

    /// Returns all extra required fields for a given note type.
    /// Returns an empty slice if no schema extension is defined.
    pub fn extra_required_fields(&self, note_type: &str) -> &[String] {
        self.schemas
            .get(note_type)
            .map(|s| s.extra_required.as_slice())
            .unwrap_or_default()
    }

    /// The user-defined note type named `name`, if declared under
    /// `[note_types.<name>]`.
    pub fn custom_type(&self, name: &str) -> Option<&CustomNoteType> {
        self.note_types.get(name)
    }

    /// Structural validation of the `[note_types.*]` table — the checks that
    /// need no knowledge of the built-in *type* set (that reserved-name check
    /// lives in `cdno-domain`, which layers it on top of this). Surfaced at
    /// vault-open so a malformed declaration fails fast rather than silently
    /// mis-shaping notes. Rejects a custom type whose:
    /// - `folder` is empty, has leading/trailing whitespace, or escapes the
    ///   vault (absolute, contains `..`, or uses a `\` separator);
    /// - `folder` collides with another custom type's, or with a built-in
    ///   top-level folder ([`crate::paths::RESERVED_TOP_LEVEL_FOLDERS`]);
    /// - `template` filename contains a path separator;
    /// - `title_field`/`date_field` names a field not in `required`/`optional`.
    pub fn validate_note_types(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err::<(), ConfigError>(ConfigError::InvalidNoteType(msg));
        let mut folders: HashMap<&str, &str> = HashMap::new();
        for (name, def) in &self.note_types {
            let folder = def.folder.as_str();
            if folder.is_empty() {
                return invalid(format!("note type `{name}` has an empty `folder`"));
            }
            if folder != folder.trim() {
                return invalid(format!(
                    "note type `{name}` `folder` has leading/trailing whitespace: `{folder}`"
                ));
            }
            if folder.starts_with('/')
                || folder.contains('\\')
                || folder.split('/').any(|seg| seg == "..")
            {
                return invalid(format!(
                    "note type `{name}` has a `folder` that escapes the vault: `{folder}`"
                ));
            }
            let top = folder.split('/').next().unwrap_or(folder);
            if crate::paths::RESERVED_TOP_LEVEL_FOLDERS.contains(&top) {
                return invalid(format!(
                    "note type `{name}` `folder` `{folder}` collides with the built-in \
                     `{top}` folder — pick a different one"
                ));
            }
            if let Some(prev) = folders.insert(folder, name) {
                return invalid(format!(
                    "note types `{prev}` and `{name}` both declare folder `{folder}`"
                ));
            }
            if let Some(template) = &def.template
                && (template.contains('/') || template.contains('\\'))
            {
                return invalid(format!(
                    "note type `{name}` template `{template}` must be a bare filename \
                     under .cuaderno/templates/, not a path"
                ));
            }
            for (label, field) in [
                ("title_field", &def.title_field),
                ("date_field", &def.date_field),
            ] {
                if let Some(field) = field
                    && !def.required.contains(field)
                    && !def.optional.contains(field)
                {
                    return invalid(format!(
                        "note type `{name}` `{label}` names `{field}`, which is not in its \
                         `required` or `optional` fields"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Structural validation of the `[schemas.*.fields]` tables (`#301`) — the
    /// checks that need no knowledge of the built-in *type* set (the
    /// reserved-engine-field check lives in `cdno-domain`, which layers it on
    /// top of this). Mirrors [`VaultConfig::validate_note_types`]; surfaced at
    /// vault-open so a malformed field declaration fails fast. Rejects a field
    /// whose:
    /// - `list = true` — reserved but unimplemented in P1;
    /// - `values` is set on a non-`string` field (allowed-values is a string
    ///   constraint, not a type of its own);
    /// - `default` does not type-check against `type` (or, when `values` is
    ///   set, is not one of the allowed values).
    ///
    /// (Unknown `type` values and unknown keys are already rejected at
    /// deserialize time by the enum and `deny_unknown_fields`.)
    pub fn validate_schemas(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err::<(), ConfigError>(ConfigError::InvalidSchema(msg));
        for (type_name, schema) in &self.schemas {
            for (field_name, spec) in &schema.fields {
                let at = format!("`[schemas.{type_name}.fields.{field_name}]`");

                // Reserved list shape: parsed so the grammar is fixed, but not
                // yet implemented.
                if spec.list == Some(true) {
                    return invalid(format!(
                        "{at} uses `list = true`, which is not yet implemented"
                    ));
                }

                // `values` is only meaningful on a string field.
                if spec.values.is_some() && spec.ty != FieldType::String {
                    return invalid(format!(
                        "{at} sets `values` on a `{}` field — `values` is only valid on a `string`",
                        spec.ty.as_str()
                    ));
                }

                // A `default` must type-check against `type` (and `values`).
                if let Some(default) = &spec.default
                    && let Some(reason) = default_mismatch(spec, default)
                {
                    return invalid(format!("{at} has a `default` that {reason}"));
                }
            }
        }
        Ok(())
    }

    /// Resolve a variable by name. Checks static variables only.
    /// Prompted variables are not resolved here — the caller is
    /// responsible for interactive resolution.
    pub fn resolve_variable(&self, name: &str) -> Option<&str> {
        self.variables.static_vars.get(name).map(String::as_str)
    }

    /// Returns the prompt message for a prompted variable, if defined.
    pub fn prompt_for_variable(&self, name: &str) -> Option<&str> {
        self.variables.prompt.get(name).map(String::as_str)
    }

    /// Compile the `ignore` glob list into a matcher. Returns an error
    /// if any pattern is malformed — surfaced at vault-open time rather
    /// than silently ignoring an unparseable rule.
    pub fn ignore_set(&self) -> Result<IgnoreSet, ConfigError> {
        IgnoreSet::compile(&self.ignore)
    }
}

/// Whether a `default` (a raw `toml::Value`) fails to type-check against its
/// field spec. Returns `None` when the default is acceptable, or `Some(reason)`
/// describing the mismatch for [`VaultConfig::validate_schemas`]. The `default`
/// is a TOML value (from `config.toml`), distinct from the JSON note values
/// [`FieldSpec::check_value`] inspects — hence the parallel shape.
fn default_mismatch(spec: &FieldSpec, default: &toml::Value) -> Option<String> {
    let type_ok = match spec.ty {
        FieldType::Bool => default.as_bool().is_some(),
        FieldType::Int => default.as_integer().is_some(),
        FieldType::String => default.as_str().is_some(),
        // A date default is a quoted `YYYY-MM-DD` string that must parse as a
        // calendar date; static only (no "today").
        FieldType::Date => default
            .as_str()
            .is_some_and(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()),
    };
    if !type_ok {
        return Some(format!("is not a valid {}", spec.ty.as_str()));
    }
    if let Some(allowed) = &spec.values
        && let Some(s) = default.as_str()
        && !allowed.iter().any(|v| v == s)
    {
        return Some(format!("is not one of the allowed values {allowed:?}"));
    }
    None
}

/// A compiled set of `ignore` globs, matched against vault-relative
/// paths during reconciliation. Wraps `globset` so that dependency
/// stays an implementation detail of this crate — callers construct an
/// `IgnoreSet` and never name `GlobSet` themselves.
#[derive(Debug, Clone)]
pub struct IgnoreSet {
    set: GlobSet,
}

impl IgnoreSet {
    /// An ignore set that matches nothing — the default when a vault
    /// configures no `ignore` patterns, and what tests use to assert
    /// the unchanged-by-default behaviour.
    pub fn empty() -> Self {
        Self {
            set: GlobSet::empty(),
        }
    }

    /// Compile a list of glob patterns into a matcher over vault-relative
    /// paths. `literal_separator(true)` gives gitignore-ish semantics:
    /// `*` and `?` stay within a single path segment and `**` is the
    /// explicit recursive operator. globset's default lets `*` cross `/`,
    /// which would make `ignore = ["*.md"]` silently swallow every note
    /// in the vault — data loss to retrieval, the very thing the empty
    /// default guards against. A malformed pattern is rendered to a
    /// message so the foreign error type doesn't leak past this crate.
    pub fn compile(patterns: &[String]) -> Result<Self, ConfigError> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| ConfigError::InvalidGlob(e.to_string()))?;
            builder.add(glob);
        }
        let set = builder
            .build()
            .map_err(|e| ConfigError::InvalidGlob(e.to_string()))?;
        Ok(Self { set })
    }

    /// Whether `path` (vault-relative) matches any ignore glob.
    pub fn is_match(&self, path: &Path) -> bool {
        self.set.is_match(path)
    }
}
