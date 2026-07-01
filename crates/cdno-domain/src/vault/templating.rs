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
use cdno_core::template::{CustomTemplateLoader, TemplateEngine, VariableContext};

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
const TRACKING_GYM_TEMPLATE: &str = include_str!("../../templates/tracking/gym.md");
const TRACKING_BODY_TEMPLATE: &str = include_str!("../../templates/tracking/body.md");
const TRACKING_SWIM_TEMPLATE: &str = include_str!("../../templates/tracking/swim.md");
const DAILY_TEMPLATE: &str = include_str!("../../templates/daily.md");
const WEEKLY_TEMPLATE: &str = include_str!("../../templates/weekly.md");
const INBOX_TEMPLATE: &str = include_str!("../../templates/inbox.md");

/// Built-in default templates, keyed as the engine expects: `<type>` for
/// the type-level default and `<type>-<variant>` for a variant default
/// (so `tracking-gym` overrides `tracking` for the gym activity).
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
        ("tracking-gym".to_owned(), TRACKING_GYM_TEMPLATE),
        ("tracking-body".to_owned(), TRACKING_BODY_TEMPLATE),
        ("tracking-swim".to_owned(), TRACKING_SWIM_TEMPLATE),
        ("daily".to_owned(), DAILY_TEMPLATE),
        ("weekly".to_owned(), WEEKLY_TEMPLATE),
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
        let engine = self.template_engine();
        ctx.load_from_config(self.config());
        let template = engine.load_template(note_type, variant)?;
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
        Ok(rendered.content)
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
        let template = engine.load_template(note_type, variant)?;
        let mut ctx = VariableContext::new();
        ctx.load_from_config(self.config());
        let rendered = engine.render(&template, &ctx, &self.config().variables.prompt);
        Ok(rendered.unresolved_prompts)
    }

    /// The `{{placeholders}}` a note type's template supports (#271) — for
    /// discovery when writing a custom template, so the supported set isn't
    /// buried in source or docs.
    ///
    /// The "supplied" set is the placeholders the **built-in** template for
    /// `(note_type, variant)` references (the built-in, not any custom
    /// override, because the create path supplies the same keys regardless of
    /// how the template is customised). Every one is genuinely filled by the
    /// create path, so the list never advertises a placeholder that would
    /// render literally. It is *not* guaranteed exhaustive, though: a few
    /// types' create paths set an extra contextual key their default template
    /// doesn't reference (e.g. `daily` also provides `weekday`; `tracking`
    /// provides `routine` / `activity_title`), and those aren't derivable
    /// from the template text. For the complete fillable set see the
    /// "Customising templates" tutorial. Deriving from the template keeps the
    /// common case in lock-step with the scaffold and needs no hand-maintained
    /// registry.
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
        variant: Option<&str>,
    ) -> Result<Vec<TemplatePlaceholder>, DomainError> {
        let builtins = builtin_defaults();
        let variant_key = variant.map(|v| format!("{note_type}-{v}"));
        let content = variant_key
            .as_ref()
            .and_then(|k| builtins.get(k))
            .or_else(|| builtins.get(note_type))
            .ok_or_else(|| DomainError::UnknownNoteType {
                note_type: note_type.to_owned(),
            })?;

        let supplied = cdno_core::template::placeholder_names(content);
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
            .filter(|name| !supplied.contains(name))
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
                !supplied.contains(*name) && !variables.static_vars.contains_key(*name)
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
            .content)
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
