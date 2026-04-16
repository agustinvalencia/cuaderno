use std::collections::HashMap;
use std::path::Path;

use crate::config::VaultConfig;
use crate::error::CoreError;

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

    pub fn set_vault_level(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vault_level.insert(key.into(), value.into());
    }

    pub fn set_prompted(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.prompted.insert(key.into(), value.into());
    }

    /// Populate tier 3 variables from a VaultConfig.
    pub fn load_from_config(&mut self, config: &VaultConfig) {
        for (key, value) in &config.variables.static_vars {
            self.vault_level.insert(key.clone(), value.clone());
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

/// Template engine: loads templates, resolves variables, renders output.
///
/// Template selection priority:
/// 1. Activity-specific custom template (e.g. `templates/tracking-gym.md`)
/// 2. Type-level custom template (e.g. `templates/project.md`)
/// 3. Built-in default from the fallback map
pub struct TemplateEngine {
    vault_root: Option<Box<Path>>,
    defaults: HashMap<String, &'static str>,
}

impl TemplateEngine {
    /// Create a new engine.
    ///
    /// - `vault_root`: path to the vault, used to find custom templates
    ///   in `.cuaderno/templates/`. Pass `None` for test contexts.
    /// - `defaults`: map of type name → built-in template content,
    ///   provided by the domain layer via `include_str!`.
    pub fn new(vault_root: Option<&Path>, defaults: HashMap<String, &'static str>) -> Self {
        Self {
            vault_root: vault_root.map(|p| p.into()),
            defaults,
        }
    }

    /// Load a template for the given type and optional variant.
    ///
    /// Checks custom templates first, then falls back to built-in defaults.
    /// A variant like `"gym"` for type `"tracking"` looks for `tracking-gym.md`.
    pub fn load_template(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<Template, CoreError> {
        // 1. Try activity-specific custom template
        if let Some(variant) = variant
            && let Some(template) = self.load_custom(note_type, Some(variant))?
        {
            return Ok(template);
        }

        // 2. Try type-level custom template
        if let Some(template) = self.load_custom(note_type, None)? {
            return Ok(template);
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

        Err(CoreError::TemplateNotFound {
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

    /// Try to load a custom template from `.cuaderno/templates/`.
    fn load_custom(
        &self,
        note_type: &str,
        variant: Option<&str>,
    ) -> Result<Option<Template>, CoreError> {
        let vault_root = match &self.vault_root {
            Some(root) => root,
            None => return Ok(None),
        };

        let filename = match variant {
            Some(v) => format!("{note_type}-{v}.md"),
            None => format!("{note_type}.md"),
        };

        let path = vault_root
            .join(".cuaderno")
            .join("templates")
            .join(&filename);

        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path).map_err(|source| CoreError::TemplateRead {
            path: path.clone(),
            source,
        })?;

        Ok(Some(Template { content }))
    }
}
