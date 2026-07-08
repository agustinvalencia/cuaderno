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

use cdno_core::error::TemplateError;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;
use cdno_core::template::{
    CustomTemplateLoader, Template, TemplateEngine, TemplateSource, VariableContext,
};

use super::Vault;
use crate::error::DomainError;

/// Where a template placeholder's value comes from — the classification
/// [`Vault::template_placeholders`] attaches to each name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceholderSource {
    /// A contextual key the note type's create path fills automatically
    /// (e.g. `title`, `created`, `status`). Derived from the built-in
    /// template, which references exactly what the scaffold supplies.
    Supplied,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplatePlaceholder {
    pub name: String,
    pub source: PlaceholderSource,
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
        // config-defined custom type yields its create-path built-ins plus its
        // declared fields. A truly unknown type errors.
        let supplied = self
            .type_registry()
            .resolve(note_type)
            .ok_or_else(|| DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            })?
            .supplied_placeholders();

        let mut out: Vec<TemplatePlaceholder> = supplied
            .iter()
            .map(|name| TemplatePlaceholder {
                name: name.clone(),
                source: PlaceholderSource::Supplied,
            })
            .collect();

        // Config-level names available to any template, sorted for
        // deterministic output (HashMap iteration order is not). A name that
        // is already supplied contextually is skipped — it can't take effect.
        let variables = &self.config().variables;
        let mut static_names: Vec<&String> = variables
            .static_vars
            .keys()
            .filter(|name| !supplied.iter().any(|s| s.as_str() == name.as_str()))
            .collect();
        static_names.sort();
        for name in static_names {
            out.push(TemplatePlaceholder {
                name: name.clone(),
                source: PlaceholderSource::Config,
            });
        }

        // Prompted names, minus any already supplied or satisfied by a static
        // default (a static default suppresses the prompt — it's effectively
        // config).
        let mut prompt_names: Vec<(&String, &String)> = variables
            .prompt
            .iter()
            .filter(|(name, _)| {
                !supplied.iter().any(|s| s.as_str() == name.as_str())
                    && !variables.static_vars.contains_key(*name)
            })
            .collect();
        prompt_names.sort_by(|a, b| a.0.cmp(b.0));
        for (name, message) in prompt_names {
            out.push(TemplatePlaceholder {
                name: name.clone(),
                source: PlaceholderSource::Prompt {
                    message: message.clone(),
                },
            });
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
