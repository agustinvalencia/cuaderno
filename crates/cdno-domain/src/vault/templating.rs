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
    pub(in crate::vault) fn scaffold(
        &self,
        note_type: &str,
        variant: Option<&str>,
        ctx: &VariableContext,
    ) -> Result<String, DomainError> {
        let engine = self.template_engine();
        let template = engine.load_template(note_type, variant)?;
        // Prompted variables (config `[variables.prompt]`) are a deferred
        // follow-up; pass none, so every placeholder resolves from `ctx`.
        Ok(engine.render(&template, ctx, &HashMap::new()).content)
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
