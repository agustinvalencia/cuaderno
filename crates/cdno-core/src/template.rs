use std::collections::HashMap;
use std::path::Path;

use crate::config::VaultConfig;
use crate::error::TemplateError;

/// A parsed template — raw content with `{{variable}}` placeholders.
#[derive(Debug, Clone)]
pub struct Template {
    pub content: String,
}

/// The output of rendering a template.
#[derive(Debug, Clone)]
pub struct RenderedNote {
    pub content: String,
    /// Prompted variables that still need values.
    /// Each entry is `(variable_name, prompt_message)`.
    pub unresolved_prompts: Vec<(String, String)>,
}

/// Layered variable resolution following four-tier precedence.
///
/// Resolution order (first match wins):
/// 1. Built-ins — date, time, year, etc.
/// 2. Contextual — title, slug, context, etc.
/// 3. Vault-level — from `config.toml` `[variables]`
/// 4. Prompted — from `config.toml` `[variables.prompt]` (value if provided)
#[derive(Debug, Default)]
pub struct VariableContext {
    builtins: HashMap<String, String>,
    contextual: HashMap<String, String>,
    vault_level: HashMap<String, String>,
    prompted: HashMap<String, String>,
}

impl VariableContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_builtin(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.builtins.insert(key.into(), value.into());
    }

    pub fn set_contextual(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.contextual.insert(key.into(), value.into());
    }

    /// Tier 3 (vault-level): populated from config `[variables]` via
    /// [`load_from_config`](Self::load_from_config), called on every
    /// creation path (#238).
    pub fn set_vault_level(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vault_level.insert(key.into(), value.into());
    }

    /// Tier 4 (prompted): from config `[variables.prompt]`. Resolved by
    /// `resolve` but not yet populated by any creation path (follow-up).
    pub fn set_prompted(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.prompted.insert(key.into(), value.into());
    }

    /// Populate tier 3 variables from a VaultConfig (wired in #238).
    pub fn load_from_config(&mut self, config: &VaultConfig) {
        for (key, value) in &config.variables.static_vars {
            self.set_vault_level(key.clone(), value.clone());
        }
    }

    /// Resolve a variable name through all four tiers.
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.builtins
            .get(name)
            .or_else(|| self.contextual.get(name))
            .or_else(|| self.vault_level.get(name))
            .or_else(|| self.prompted.get(name))
            .map(String::as_str)
    }
}

/// Loads the raw content of a custom template by filename (e.g.
/// `"project.md"` or `"tracking-gym.md"`), returning `None` when the
/// vault has no such custom template. Injected so the engine doesn't
/// hard-code an I/O backend: the fs constructor reads
/// `.cuaderno/templates/`, while the domain backs it with a `VaultStore`
/// (so custom templates resolve through the same abstraction as every
/// other vault file, and work under `MemoryVaultStore` in tests).
pub type CustomTemplateLoader = Box<dyn Fn(&str) -> Result<Option<String>, TemplateError>>;

/// Template engine: loads templates, resolves variables, renders output.
///
/// Template selection priority:
/// 1. Activity-specific custom template (e.g. `templates/tracking-gym.md`)
/// 2. Type-level custom template (e.g. `templates/project.md`)
/// 3. Built-in default from the fallback map
pub struct TemplateEngine {
    loader: CustomTemplateLoader,
    defaults: HashMap<String, &'static str>,
}

impl TemplateEngine {
    /// Create an engine that reads custom templates from
    /// `.cuaderno/templates/` on the filesystem under `vault_root`
    /// (`None` disables custom templates — built-in defaults only).
    ///
    /// `defaults` maps type name → built-in template content, provided
    /// by the domain layer via `include_str!`.
    ///
    /// In-tree this filesystem constructor is exercised only by this
    /// crate's tests; the domain resolves custom templates through a
    /// `VaultStore` via [`with_loader`](Self::with_loader).
    pub fn new(vault_root: Option<&Path>, defaults: HashMap<String, &'static str>) -> Self {
        let root = vault_root.map(|p| p.to_path_buf());
        let loader: CustomTemplateLoader = Box::new(move |filename: &str| {
            let Some(root) = &root else {
                return Ok(None);
            };
            let path = root.join(crate::paths::TEMPLATES_DIR).join(filename);
            if !path.exists() {
                return Ok(None);
            }
            std::fs::read_to_string(&path)
                .map(Some)
                .map_err(|source| TemplateError::Read {
                    path: path.clone(),
                    source,
                })
        });
        Self { loader, defaults }
    }

    /// Create an engine with a caller-supplied custom-template loader —
    /// used by the domain to resolve `.cuaderno/templates/` through a
    /// `VaultStore` rather than the filesystem.
    pub fn with_loader(
        loader: CustomTemplateLoader,
        defaults: HashMap<String, &'static str>,
    ) -> Self {
        Self { loader, defaults }
    }

    /// Load a template for the given type and optional variant.
    ///
    /// Checks custom templates first, then falls back to built-in defaults.
    /// A variant like `"gym"` for type `"tracking"` looks for `tracking-gym.md`.
    pub fn load_template(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<Template, TemplateError> {
        // 1. Custom variant-specific template (e.g. `tracking-gym.md`)
        if let Some(variant) = variant
            && let Some(content) = (self.loader)(&format!("{note_type}-{variant}.md"))?
        {
            return Ok(Template { content });
        }

        // 2. Custom type-level template (e.g. `project.md`)
        if let Some(content) = (self.loader)(&format!("{note_type}.md"))? {
            return Ok(Template { content });
        }

        // 3. Fall back to built-in default
        let key = match variant {
            Some(v) => format!("{note_type}-{v}"),
            None => note_type.to_string(),
        };

        // Try variant-specific default first, then plain type default
        if let Some(content) = self.defaults.get(&key) {
            return Ok(Template {
                content: (*content).to_string(),
            });
        }

        if variant.is_some()
            && let Some(content) = self.defaults.get(note_type)
        {
            return Ok(Template {
                content: (*content).to_string(),
            });
        }

        Err(TemplateError::NotFound {
            note_type: note_type.to_string(),
            variant: variant.map(String::from),
        })
    }

    /// Render a template by substituting variables from the context.
    ///
    /// Returns the rendered content plus any unresolved prompted variables.
    pub fn render(
        &self,
        template: &Template,
        variables: &VariableContext,
        prompt_definitions: &HashMap<String, String>,
    ) -> RenderedNote {
        let mut unresolved_prompts = Vec::new();
        let mut result = String::with_capacity(template.content.len());

        let mut rest = template.content.as_str();
        while let Some(start) = rest.find("{{") {
            result.push_str(&rest[..start]);

            let after_open = &rest[start + 2..];
            if let Some(end) = after_open.find("}}") {
                let var_name = after_open[..end].trim();

                if let Some(value) = variables.resolve(var_name) {
                    result.push_str(value);
                } else if let Some(prompt_msg) = prompt_definitions.get(var_name) {
                    unresolved_prompts.push((var_name.to_string(), prompt_msg.clone()));
                    // Leave placeholder for caller to fill after prompting
                    result.push_str(&rest[start..start + 4 + end]);
                } else {
                    // Unknown variable — leave as-is
                    result.push_str(&rest[start..start + 4 + end]);
                }

                rest = &after_open[end + 2..];
            } else {
                // No closing `}}` — not a variable, keep literal
                result.push_str("{{");
                rest = after_open;
            }
        }
        result.push_str(rest);

        RenderedNote {
            content: result,
            unresolved_prompts,
        }
    }
}

/// Extract the distinct `{{placeholder}}` names referenced in template
/// `content`, in first-appearance order.
///
/// Tokenisation mirrors [`TemplateEngine::render`] exactly — names are
/// trimmed, and a `{{` with no following `}}` is skipped rather than
/// treated as a placeholder — so "what a template references" always
/// matches "what render will substitute".
pub fn placeholder_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let name = after_open[..end].trim();
            if !name.is_empty() && !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
            rest = &after_open[end + 2..];
        } else {
            // No closing `}}` — mirror render and advance past the `{{`.
            rest = after_open;
        }
    }
    names
}
