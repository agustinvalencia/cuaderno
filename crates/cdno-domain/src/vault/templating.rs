//! Note scaffolding through the template engine (#212).
//!
//! Every `create_*` builds a [`VariableContext`] and calls
//! [`Vault::scaffold`], which resolves the effective template — a custom
//! one in `.cuaderno/templates/` if present, else the built-in default —
//! and substitutes the variables. Resolution reads custom templates
//! through the [`VaultStore`](cdno_core::store::VaultStore), so they
//! participate in the same I/O abstraction as every other vault file
//! (and work under `MemoryVaultStore` in tests) rather than the engine's
//! filesystem path.
//!
//! The built-in defaults are the `include_str!`'d templates, centralised
//! here so there's one map from note type → default content.

use std::collections::HashMap;
use std::sync::Arc;

use cdno_core::config::CustomNoteType;
use cdno_core::error::TemplateError;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;
use cdno_core::template::{
    CustomTemplateLoader, Template, TemplateEngine, TemplateSource, VariableContext,
};

use super::Vault;
use crate::error::DomainError;
use crate::note_type::NoteType;
use crate::type_registry::NoteTypeDescriptor;

/// Where a template placeholder's value comes from — the classification
/// [`Vault::template_placeholders`] attaches to each name.
///
/// Serialised adjacently-tagged (`{ "kind": "supplied" }`,
/// `{ "kind": "prompt", "data": { "message": … } }`) for the desktop
/// Templates view's placeholder-reference panel, which groups the set by
/// `kind`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum PlaceholderSource {
    /// A contextual key the note type's create path fills automatically
    /// (e.g. `title`, `created`, `status`). Derived from the built-in
    /// template, which references exactly what the scaffold supplies.
    Supplied,
    /// A field declared under `[note_types.<name>]` (`required`/`optional`)
    /// for a config-defined custom type. The create path fills it from the
    /// note's frontmatter, so a custom template may reference it — kept
    /// distinct from `Supplied` so the reference panel can label it as the
    /// type's own schema field rather than a universal built-in.
    Schema,
    /// A static config variable (`[variables]` in `config.toml`), available
    /// to any template.
    Config,
    /// A prompted config variable (`[variables.prompt]`): a value must be
    /// provided at creation — CLI `--var name=value`, MCP `vars`, or the
    /// interactive prompt. Carries the prompt message.
    Prompt { message: String },
}

/// A `{{placeholder}}` a note type's template supports, plus where its
/// value comes from. Returned by [`Vault::template_placeholders`].
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TemplatePlaceholder {
    pub name: String,
    pub source: PlaceholderSource,
}

/// Which rung a template's effective content came from — the
/// serialisable, wire-facing mirror of [`TemplateSource`] (which lives in
/// `cdno-core` and carries no serde/ts-rs derives). Reported by
/// [`Vault::read_template`] and [`Vault::list_templates`] so the desktop
/// Templates view can say "custom override" vs "built-in default".
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSourceKind {
    /// A custom `.cuaderno/templates/<type>-<variant>.md`.
    CustomVariant,
    /// A custom `.cuaderno/templates/<type>.md` (or a custom type's
    /// configured template file).
    CustomBase,
    /// A built-in `<type>-<variant>` default.
    BuiltinVariant,
    /// The built-in plain `<type>` default.
    BuiltinDefault,
}

impl From<TemplateSource> for TemplateSourceKind {
    fn from(source: TemplateSource) -> Self {
        match source {
            TemplateSource::CustomVariant => TemplateSourceKind::CustomVariant,
            TemplateSource::CustomBase => TemplateSourceKind::CustomBase,
            TemplateSource::BuiltinVariant => TemplateSourceKind::BuiltinVariant,
            TemplateSource::BuiltinDefault => TemplateSourceKind::BuiltinDefault,
        }
    }
}

/// One row of [`Vault::list_templates`]: a note type and the status of its
/// template. Built-in types always have an effective template (their
/// built-in default, unless overridden); a config-defined custom type may
/// have none yet, which the view offers to scaffold via
/// [`Vault::create_template`].
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TemplateSummary {
    /// The type key — `project`, `daily`, or a config custom type's name.
    pub note_type: String,
    /// A capitalised label for the list (`project` → `Project`).
    pub display_name: String,
    /// Whether this is a config-defined custom type (vs a built-in). The
    /// view offers `Create` only for a custom type with no template.
    pub is_custom_type: bool,
    /// The effective template's source rung, or `None` for a custom type
    /// that has no template file yet (nothing on disk backs it).
    pub source: Option<TemplateSourceKind>,
    /// Whether a custom override file exists under `.cuaderno/templates/`.
    pub has_custom_file: bool,
    /// The vault-relative path the custom override lives (or would live)
    /// at — always resolved, so "Open in editor" works even before a file
    /// exists.
    pub path: String,
}

/// The effective (resolved) content of a template plus its source rung,
/// returned by [`Vault::read_template`]. `source` is `None` when the
/// content is a synthesised starter for a custom type with no template
/// file — nothing on disk backs it yet.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TemplateContent {
    pub content: String,
    pub source: Option<TemplateSourceKind>,
}

const PROJECT_TEMPLATE: &str = include_str!("../../templates/project.md");
const ACTION_TEMPLATE: &str = include_str!("../../templates/action.md");
const QUESTION_TEMPLATE: &str = include_str!("../../templates/question.md");
const STEWARDSHIP_TEMPLATE: &str = include_str!("../../templates/stewardship.md");
const PORTFOLIO_TEMPLATE: &str = include_str!("../../templates/portfolio.md");
const EVIDENCE_TEMPLATE: &str = include_str!("../../templates/evidence.md");
const COMMITMENT_TEMPLATE: &str = include_str!("../../templates/commitment.md");
const TRACKING_GENERIC_TEMPLATE: &str = include_str!("../../templates/tracking/generic.md");
const DAILY_TEMPLATE: &str = include_str!("../../templates/daily.md");
const WEEKLY_TEMPLATE: &str = include_str!("../../templates/weekly.md");
const MONTHLY_TEMPLATE: &str = include_str!("../../templates/monthly.md");
const INBOX_TEMPLATE: &str = include_str!("../../templates/inbox.md");

/// Built-in default templates, keyed as the engine expects: `<type>` for
/// the type-level default and `<type>-<variant>` for a variant default.
///
/// Only the neutral `tracking` (generic) template ships built in — no
/// activity-specific variants. A vault supplies its own via
/// `.cuaderno/templates/tracking-<activity>.md`, which the resolver picks up
/// (slugify the activity → look up `tracking-<slug>` → fall back to generic).
/// See `examples/templates/tracking/` for ready-made gym/body/swim variants.
fn builtin_defaults() -> HashMap<String, &'static str> {
    HashMap::from([
        ("project".to_owned(), PROJECT_TEMPLATE),
        ("action".to_owned(), ACTION_TEMPLATE),
        ("question".to_owned(), QUESTION_TEMPLATE),
        ("stewardship".to_owned(), STEWARDSHIP_TEMPLATE),
        ("portfolio".to_owned(), PORTFOLIO_TEMPLATE),
        ("evidence".to_owned(), EVIDENCE_TEMPLATE),
        ("commitment".to_owned(), COMMITMENT_TEMPLATE),
        ("tracking".to_owned(), TRACKING_GENERIC_TEMPLATE),
        ("daily".to_owned(), DAILY_TEMPLATE),
        ("weekly".to_owned(), WEEKLY_TEMPLATE),
        ("monthly".to_owned(), MONTHLY_TEMPLATE),
        ("inbox".to_owned(), INBOX_TEMPLATE),
    ])
}

impl Vault {
    /// Resolve the effective template for `note_type` (+ optional
    /// `variant`) and render it with `ctx`. Custom `.cuaderno/templates/`
    /// overrides win over the built-in default; unknown `{{placeholders}}`
    /// are left verbatim (the caller is expected to supply every one).
    ///
    /// Static config variables (`[variables]` in `config.toml`) are layered
    /// into `ctx` here (#238 tier 3), so every create path gets them for
    /// free. Precedence is builtins → contextual → vault-level → prompted
    /// ([`VariableContext::resolve`]), so config vars only fill names a
    /// caller hasn't already set contextually — they can't override
    /// `title`/`context`/etc.
    ///
    /// Prompted variables (`[variables.prompt]`, #238 tier 4) are rendered
    /// against the config's prompt definitions; a placeholder that is
    /// prompt-defined, present in the template, and still unresolved (no
    /// `set_prompted` value and no static default) is an error rather than
    /// a literal `{{name}}` left in the note. Callers that gather prompted
    /// values (the CLI) call `set_prompted` on `ctx` first.
    pub(in crate::vault) fn scaffold(
        &self,
        note_type: &str,
        variant: Option<&str>,
        ctx: &mut VariableContext,
    ) -> Result<String, DomainError> {
        self.scaffold_with_source(note_type, variant, ctx)
            .map(|(content, _source)| content)
    }

    /// As [`scaffold`](Self::scaffold), but also reports which
    /// [`TemplateSource`] rung the effective template came from. Only paths
    /// that surface the resolution to the user (the `cdno track` hint) need
    /// this; `scaffold` is the discard-the-source wrapper the other create
    /// paths call.
    pub(in crate::vault) fn scaffold_with_source(
        &self,
        note_type: &str,
        variant: Option<&str>,
        ctx: &mut VariableContext,
    ) -> Result<(String, TemplateSource), DomainError> {
        let engine = self.template_engine();
        // Schema-field defaults first (tier 3), then `[variables]` (also tier
        // 3): a same-named static var overwrites the collision below, so an
        // explicit `[variables]` entry wins over a declared default.
        self.load_schema_defaults(note_type, ctx);
        ctx.load_from_config(self.config());
        let (template, source) = engine.load_template(note_type, variant)?;
        let rendered = engine.render(&template, ctx, &self.config().variables.prompt);
        if !rendered.unresolved_prompts.is_empty() {
            return Err(DomainError::UnresolvedPrompts {
                note_type: note_type.to_owned(),
                names: rendered
                    .unresolved_prompts
                    .into_iter()
                    .map(|(name, _msg)| name)
                    .collect(),
            });
        }
        Ok((rendered.content, source))
    }

    /// Inject `note_type`'s declared schema-field values (`#301` PR-B) into
    /// `ctx` at **tier 3** (`set_vault_level`) — the same rung as `[variables]`.
    ///
    /// Tier 3, *not* tier 2 (`set_contextual`), is the correctness lynchpin.
    /// Every create path sets its engine/caller values contextually (tier 2)
    /// *before* calling `scaffold`, and tier 2 shadows tier 3 in
    /// [`VariableContext::resolve`]. So a declared value only fills a
    /// `{{field}}` the create path did not already supply: an engine value
    /// always wins, and no create-path signature has to change. Injecting at
    /// tier 2 would instead CLOBBER those engine values — never do that.
    ///
    /// Value source per declared field: a static `default` contributes that
    /// value (`FieldSpec::default_template_value`); a field *without* a default
    /// (optional or the still-inert `required`) contributes the literal `null`
    /// — the built-in templates' convention for an absent optional (see
    /// `action`'s `completed`/`blocker`). Either way the token resolves, so a
    /// custom template referencing `{{field}}` never renders a literal
    /// `{{field}}`.
    ///
    /// A built-in's declared field only lands in frontmatter if a *custom*
    /// `.cuaderno/templates/<type>.md` override references `{{field}}` — render
    /// substitutes referenced tokens, it never adds a frontmatter line, and the
    /// shipped built-in templates can't reference vault-specific fields. A
    /// vault with no `[schemas.*.fields]` block for this type is a no-op.
    ///
    /// A name that is BOTH a declared schema field and a `[variables.prompt]`
    /// prompt var is SKIPPED here: the prompt owns the name. The `_with_vars`
    /// create paths set the caller's prompted answer at tier 4 *before*
    /// scaffold, and tier 3 shadows tier 4 in [`VariableContext::resolve`], so
    /// injecting a tier-3 default (or `null`) would silently discard that
    /// answer. Interactive supply is the more specific intent, so the prompt
    /// path collects the value and the schema default is intentionally unused.
    fn load_schema_defaults(&self, note_type: &str, ctx: &mut VariableContext) {
        let Some(schema) = self.config().schema_for(note_type) else {
            return;
        };
        let prompt_vars = &self.config().variables.prompt;
        for (name, spec) in schema.declared_fields() {
            // The prompt path owns any name it defines — see the doc comment.
            if prompt_vars.contains_key(&name) {
                continue;
            }
            let value = spec
                .default_template_value()
                .unwrap_or_else(|| "null".to_owned());
            ctx.set_vault_level(name, value);
        }
    }

    /// The prompt-defined variables (`[variables.prompt]`) a note's effective
    /// template actually references and that static config doesn't already
    /// satisfy — `(name, prompt-message)` pairs. The CLI calls this to know
    /// what to ask for before creating the note; it shares `render`'s
    /// `unresolved_prompts` logic so "what to ask" matches what `scaffold`
    /// will later enforce.
    pub fn template_prompts(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<Vec<(String, String)>, DomainError> {
        let engine = self.template_engine();
        let (template, _source) = engine.load_template(note_type, variant)?;
        let mut ctx = VariableContext::new();
        ctx.load_from_config(self.config());
        let rendered = engine.render(&template, &ctx, &self.config().variables.prompt);
        Ok(rendered.unresolved_prompts)
    }

    /// The complete set of `{{placeholders}}` a note type supports (#271,
    /// #279) — for discovery when writing a custom template, so the supported
    /// set isn't buried in source or docs.
    ///
    /// The "supplied" set is the type's full create-path key set
    /// ([`NoteType::supplied_placeholders`]) — every key the scaffold fills,
    /// including body placeholders and keys the *default* template happens not
    /// to reference (e.g. `daily`'s `weekday`, `tracking`'s `routine`), so the
    /// list is exhaustive. It mirrors the create path's `set_contextual` calls,
    /// so a custom template using any of these names renders rather than leaving
    /// a literal `{{…}}` (a drift test guards the converse — no built-in
    /// template references a name outside this set). Variant is irrelevant: a
    /// type's create path supplies the same keys regardless of which template
    /// resolves.
    ///
    /// Config-level variables available to every template are appended:
    /// `[variables]` as `Config`, `[variables.prompt]` as `Prompt`. A config
    /// name that collides with a supplied key is omitted — the contextual
    /// value shadows it (see [`Vault::scaffold`] precedence), so it would
    /// never take effect. Errors with [`DomainError::UnknownNoteType`] for an
    /// unrecognised type.
    pub fn template_placeholders(
        &self,
        note_type: &str,
    ) -> Result<Vec<TemplatePlaceholder>, DomainError> {
        // Registry-aware: a built-in yields its static supplied set; a
        // config-defined custom type yields its create-path built-ins
        // (`Supplied`) plus its declared `[note_types.<name>]` fields
        // (`Schema`). A truly unknown type errors.
        let descriptor = self.type_registry().resolve(note_type).ok_or_else(|| {
            DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            }
        })?;

        // Accumulate `(name, source)` pairs, de-duplicating by name: a config
        // or prompt var that collides with an already-present name is shadowed
        // by the contextual value (see [`Vault::scaffold`] precedence), so it
        // would never take effect and is dropped.
        let mut out: Vec<TemplatePlaceholder> = Vec::new();
        let add = |out: &mut Vec<TemplatePlaceholder>, name: &str, source: PlaceholderSource| {
            if !out.iter().any(|p| p.name == name) {
                out.push(TemplatePlaceholder {
                    name: name.to_owned(),
                    source,
                });
            }
        };

        match descriptor {
            NoteTypeDescriptor::Builtin(nt) => {
                // A built-in's full create-path key set, all supplied.
                for name in nt.supplied_placeholders() {
                    add(&mut out, name, PlaceholderSource::Supplied);
                }
                // Config-declared schema fields for this built-in type (#301):
                // emitted as `Schema` so the desktop Templates editor recognises
                // `{{field}}` in a custom override instead of warning "renders
                // literally". Sorted for deterministic output (the config map is
                // unordered). A field colliding with a supplied contextual name
                // is dropped by `add` — the supplied value shadows it, so the
                // name stays in the set as `Supplied` and never false-warns.
                if let Some(schema) = self.config().schema_for(nt.as_str()) {
                    let mut field_names: Vec<String> =
                        schema.declared_fields().into_keys().collect();
                    field_names.sort();
                    for name in field_names {
                        add(&mut out, &name, PlaceholderSource::Schema);
                    }
                }
            }
            NoteTypeDescriptor::Custom { def, .. } => {
                // The create-path built-ins every custom note gets, then the
                // type's own declared schema fields — the latter are what let
                // a custom template reference `{{field}}` without a false
                // "unknown token" warning in the editor.
                for name in ["title", "slug", "created", "date"] {
                    add(&mut out, name, PlaceholderSource::Supplied);
                }
                for field in def.required.iter().chain(def.optional.iter()) {
                    add(&mut out, field, PlaceholderSource::Schema);
                }
            }
        }

        // Config-level names available to any template, sorted for
        // deterministic output (HashMap iteration order is not).
        let variables = &self.config().variables;
        let mut static_names: Vec<&String> = variables
            .static_vars
            .keys()
            .filter(|name| !out.iter().any(|p| p.name.as_str() == name.as_str()))
            .collect();
        static_names.sort();
        for name in static_names {
            add(&mut out, name, PlaceholderSource::Config);
        }

        // Prompted names, minus any already present or satisfied by a static
        // default (a static default suppresses the prompt — it's effectively
        // config).
        let mut prompt_names: Vec<(&String, &String)> = variables
            .prompt
            .iter()
            .filter(|(name, _)| {
                !out.iter().any(|p| p.name.as_str() == name.as_str())
                    && !variables.static_vars.contains_key(*name)
            })
            .collect();
        prompt_names.sort_by(|a, b| a.0.cmp(b.0));
        for (name, message) in prompt_names {
            add(
                &mut out,
                name,
                PlaceholderSource::Prompt {
                    message: message.clone(),
                },
            );
        }

        Ok(out)
    }

    /// Materialise a built-in template into `.cuaderno/templates/<key>.md` as
    /// an editable starting point for customisation (#270). `<key>` is
    /// `note_type` or `note_type-variant`, matching the engine's resolution
    /// names (`builtin_defaults`) — so the ejected file is exactly what the
    /// custom-template loader will later pick up.
    ///
    /// Unlike scaffolding, a `variant` is *not* resolved with a fallback: it
    /// must have its own built-in, otherwise there's nothing distinct to eject
    /// — `UnknownTemplateVariant`. (No variant templates ship built-in today,
    /// so `variant` currently always errors here; the arg is kept for when a
    /// future type ships one. Activity variants for `tracking` are authored in
    /// the vault, not ejected — see `examples/templates/tracking/`.)
    /// Refuses to overwrite an existing custom template unless `force`
    /// (`TemplateAlreadyExists`). Returns the written path.
    pub fn eject_template(
        &self,
        note_type: &str,
        variant: Option<&str>,
        force: bool,
    ) -> Result<VaultPath, DomainError> {
        let builtins = builtin_defaults();
        let key = match variant {
            Some(v) => format!("{note_type}-{v}"),
            None => note_type.to_owned(),
        };
        let content = builtins.get(&key).ok_or_else(|| match variant {
            Some(v) => DomainError::UnknownTemplateVariant {
                note_type: note_type.to_owned(),
                variant: v.to_owned(),
            },
            None => DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            },
        })?;

        let path = VaultPath::new(format!("{}/{key}.md", cdno_core::paths::TEMPLATES_DIR))?;
        if !force && self.store.exists(&path)? {
            return Err(DomainError::TemplateAlreadyExists {
                path: path.to_string(),
            });
        }
        self.store.write_file(&path, content)?;
        Ok(path)
    }

    /// The resolved (effective) template content for `note_type` (+
    /// optional `variant`) — the custom `.cuaderno/templates/` override
    /// if present, else the built-in default. Used by the normaliser to
    /// derive the canonical frontmatter order from whatever template a
    /// note is actually created from.
    ///
    /// This is the built-in resolution path via the engine; the public
    /// [`Vault::read_template`] wraps it and additionally handles a config
    /// custom type's own template file.
    pub(in crate::vault) fn resolve_template_content(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<String, DomainError> {
        Ok(self
            .template_engine()
            .load_template(note_type, variant)?
            .0
            .content)
    }

    /// Every note type and the status of its template — the desktop
    /// Templates view's list (#357). Built-ins first (in [`NoteType::ALL`]
    /// order), then config-defined custom types sorted by name so the list
    /// is stable across runs.
    ///
    /// A built-in always has an effective template (its built-in default,
    /// reported as [`TemplateSourceKind::BuiltinDefault`] unless a custom
    /// override file exists, in which case [`TemplateSourceKind::CustomBase`]).
    /// A config custom type has an effective template only once its file
    /// exists; until then `source` is `None` and the view offers `Create`.
    pub fn list_templates(&self) -> Result<Vec<TemplateSummary>, DomainError> {
        let mut out: Vec<TemplateSummary> = Vec::new();

        for nt in NoteType::ALL {
            let key = nt.as_str();
            let path = template_path(&format!("{key}.md"))?;
            let has_custom_file = self.store.exists(&path)?;
            // Built-ins always ship a default, so the effective source is the
            // override when present, else the built-in default.
            let source = Some(if has_custom_file {
                TemplateSourceKind::CustomBase
            } else {
                TemplateSourceKind::BuiltinDefault
            });
            out.push(TemplateSummary {
                note_type: key.to_owned(),
                display_name: title_case(key),
                is_custom_type: false,
                source,
                has_custom_file,
                path: path.to_string(),
            });
        }

        // Config custom types, sorted by name (the config map is unordered).
        let mut custom: Vec<(&String, &CustomNoteType)> = self.config().note_types.iter().collect();
        custom.sort_by(|a, b| a.0.cmp(b.0));
        for (name, def) in custom {
            let path = template_path(&custom_template_filename(name, def))?;
            let has_custom_file = self.store.exists(&path)?;
            // No built-in backs a custom type, so its only effective template
            // is the file — absent it, `None` (the view shows `Create`).
            let source = has_custom_file.then_some(TemplateSourceKind::CustomBase);
            out.push(TemplateSummary {
                note_type: name.clone(),
                display_name: title_case(name),
                is_custom_type: true,
                source,
                has_custom_file,
                path: path.to_string(),
            });
        }

        Ok(out)
    }

    /// The effective content of `note_type`'s template (+ optional `variant`)
    /// plus its source rung, for the Templates editor (#357). Rejects an
    /// unknown type with [`DomainError::UnknownNoteType`].
    ///
    /// For a built-in this is the engine's full precedence resolution (custom
    /// override wins over built-in default). For a config custom type it reads
    /// the type's configured template file directly — the engine's generic
    /// step keys on `<type>.md`, which misses a custom type whose `template`
    /// names a different file. When a custom type has no file yet, this returns
    /// the synthesised starter with `source: None` so the editor can preview
    /// what `Create` would write.
    pub fn read_template(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<TemplateContent, DomainError> {
        let descriptor = self.type_registry().resolve(note_type).ok_or_else(|| {
            DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            }
        })?;

        match descriptor.as_custom() {
            None => {
                let (template, source) =
                    self.template_engine().load_template(note_type, variant)?;
                Ok(TemplateContent {
                    content: template.content,
                    source: Some(source.into()),
                })
            }
            Some(def) => {
                let path = template_path(&custom_template_filename(note_type, def))?;
                if self.store.exists(&path)? {
                    Ok(TemplateContent {
                        content: self.store.read_file(&path)?,
                        source: Some(TemplateSourceKind::CustomBase),
                    })
                } else {
                    Ok(TemplateContent {
                        content: custom_starter_template(note_type, def),
                        source: None,
                    })
                }
            }
        }
    }

    /// Write `content` verbatim as the custom template for `note_type` (+
    /// optional `variant`), returning the written path (#357). Templates are
    /// config, not append-only notes, so this is a plain confined
    /// `store.write_file` — no [`VaultTransaction`](crate::VaultTransaction).
    ///
    /// On a built-in-backed type this transparently CREATES the custom
    /// override (the direct edit-and-save model — no separate `eject` step)
    /// and on a re-save overwrites it. The path is confined to
    /// `.cuaderno/templates/` via [`VaultPath`]. Rejects an unknown type with
    /// [`DomainError::UnknownNoteType`].
    pub fn save_template(
        &self,
        note_type: &str,
        variant: Option<&str>,
        content: &str,
    ) -> Result<VaultPath, DomainError> {
        let descriptor = self.type_registry().resolve(note_type).ok_or_else(|| {
            DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            }
        })?;
        let filename = match descriptor.as_custom() {
            Some(def) => custom_template_filename(note_type, def),
            None => match variant {
                Some(v) => format!("{note_type}-{v}.md"),
                None => format!("{note_type}.md"),
            },
        };
        let path = template_path(&filename)?;
        self.store.write_file(&path, content)?;
        Ok(path)
    }

    /// Scaffold a starter template for a config-defined custom type that has
    /// none yet, returning the written path (#357). The starter is a
    /// frontmatter block of `type` plus each declared `required` field as a
    /// `{{field}}` placeholder, followed by a `# {{title}}` heading — an
    /// editable starting point the author refines.
    ///
    /// Genuinely distinct from [`Vault::eject_template`], which copies a
    /// *built-in* and explicitly refuses a custom type (there's no built-in to
    /// copy). Errors with [`DomainError::BuiltinTypeNotCustom`] for a built-in
    /// type (edit-and-save its override via [`Vault::save_template`] instead),
    /// and [`DomainError::TemplateAlreadyExists`] if a file is already there.
    pub fn create_template(&self, note_type: &str) -> Result<VaultPath, DomainError> {
        let descriptor = self.type_registry().resolve(note_type).ok_or_else(|| {
            DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            }
        })?;
        let def = descriptor
            .as_custom()
            .ok_or_else(|| DomainError::BuiltinTypeNotCustom {
                note_type: note_type.to_owned(),
            })?;
        let path = template_path(&custom_template_filename(note_type, def))?;
        if self.store.exists(&path)? {
            return Err(DomainError::TemplateAlreadyExists {
                path: path.to_string(),
            });
        }
        self.store
            .write_file(&path, &custom_starter_template(note_type, def))?;
        Ok(path)
    }

    /// Render a config-defined custom type's note.
    ///
    /// Unlike [`scaffold`](Self::scaffold) (which errors if a built-in type has
    /// no template), a custom type commonly has none — you declare the type
    /// before authoring `.cuaderno/templates/<type>.md`. So this loads the
    /// type's template file (`template_name`, the configured filename or the
    /// `<type>.md` default) and renders it with `ctx`; when the file is absent
    /// it **synthesises** a minimal note from `field_order` (a frontmatter block
    /// of the declared fields that have values, plus a `# {{title}}` H1).
    ///
    /// Errors on an unresolved `[variables.prompt]` reference in a real
    /// template, matching `scaffold`.
    pub(in crate::vault) fn scaffold_custom(
        &self,
        type_name: &str,
        template_name: &str,
        field_order: &[String],
        ctx: &mut VariableContext,
    ) -> Result<String, DomainError> {
        ctx.load_from_config(self.config());
        let template_path = VaultPath::new(format!(
            "{}/{template_name}",
            cdno_core::paths::TEMPLATES_DIR
        ))?;
        if self.store.exists(&template_path)? {
            let raw = self.store.read_file(&template_path)?;
            let engine = self.template_engine();
            let rendered = engine.render(
                &Template { content: raw },
                ctx,
                &self.config().variables.prompt,
            );
            if !rendered.unresolved_prompts.is_empty() {
                return Err(DomainError::UnresolvedPrompts {
                    note_type: type_name.to_owned(),
                    names: rendered
                        .unresolved_prompts
                        .into_iter()
                        .map(|(name, _msg)| name)
                        .collect(),
                });
            }
            Ok(rendered.content)
        } else {
            Ok(synthesise_custom_note(type_name, field_order, ctx))
        }
    }

    /// A template engine whose custom-template loader reads
    /// `.cuaderno/templates/` through this vault's store.
    fn template_engine(&self) -> TemplateEngine {
        let store: Arc<dyn VaultStore> = Arc::clone(&self.store);
        let loader: CustomTemplateLoader = Box::new(move |filename: &str| {
            let rel = format!("{}/{filename}", cdno_core::paths::TEMPLATES_DIR);
            let path = VaultPath::new(&rel).map_err(|e| TemplateError::Load {
                name: filename.to_owned(),
                message: e.to_string(),
            })?;
            let exists = store.exists(&path).map_err(|e| TemplateError::Load {
                name: filename.to_owned(),
                message: e.to_string(),
            })?;
            if !exists {
                return Ok(None);
            }
            store
                .read_file(&path)
                .map(Some)
                .map_err(|e| TemplateError::Load {
                    name: filename.to_owned(),
                    message: e.to_string(),
                })
        });
        TemplateEngine::with_loader(loader, builtin_defaults())
    }
}

/// A [`VaultPath`] for `filename` under `.cuaderno/templates/`. Centralises
/// the confinement so every template read/write goes through the same
/// [`VaultPath`] guard (absolute paths and `..` escapes rejected).
fn template_path(filename: &str) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(format!(
        "{}/{filename}",
        cdno_core::paths::TEMPLATES_DIR
    ))?)
}

/// The template filename a config custom type resolves to — its configured
/// `template`, or `<name>.md` by default. Matches `scaffold_custom`'s
/// resolution so the Templates view reads/writes the same file the create
/// path renders from.
fn custom_template_filename(name: &str, def: &CustomNoteType) -> String {
    def.template.clone().unwrap_or_else(|| format!("{name}.md"))
}

/// Capitalise the first character for a list label (`project` → `Project`).
/// ASCII type keys, so a char-boundary split is safe.
fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The starter template `create_template` writes for a custom type: `type`
/// plus each declared `required` field as a `{{field}}` placeholder, then a
/// `# {{title}}` heading. Authored as literal `{{…}}` tokens (not
/// serde_yaml) because these are template placeholders the create path
/// fills, not final values.
fn custom_starter_template(type_name: &str, def: &CustomNoteType) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("type: {type_name}\n"));
    for field in &def.required {
        out.push_str(&format!("{field}: {{{{{field}}}}}\n"));
    }
    out.push_str("---\n\n# {{title}}\n");
    out
}

/// Build a minimal note for a custom type that ships no template file: a
/// frontmatter block of `type` plus each field in `field_order` that has a
/// value in `ctx`, followed by a `# {{title}}` H1 (falling back to the type
/// name).
///
/// The frontmatter is serialised through `serde_yaml` (not `format!`), so every
/// field value is emitted as a properly-quoted **string** — a value with a
/// colon, `#`, newline, or one that looks like a bool/number/list round-trips
/// verbatim rather than crashing the parse, being coerced to another YAML type,
/// or injecting a second document via an embedded `---`.
fn synthesise_custom_note(
    type_name: &str,
    field_order: &[String],
    ctx: &VariableContext,
) -> String {
    let mut map = serde_yaml::Mapping::new();
    map.insert("type".into(), type_name.into());
    for key in field_order {
        if key == "type" {
            continue;
        }
        if let Some(value) = ctx.resolve(key) {
            map.insert(key.as_str().into(), value.into());
        }
    }
    // Infallible for a string→string mapping; the empty fallback still yields a
    // well-formed (if bare) frontmatter block rather than an injection vector.
    let frontmatter = serde_yaml::to_string(&map).unwrap_or_default();
    let title = ctx.resolve("title").unwrap_or(type_name);
    format!("---\n{frontmatter}---\n\n# {title}\n")
}
