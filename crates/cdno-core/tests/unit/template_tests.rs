use cdno_core::config::VaultConfig;
use cdno_core::template::{
    Template, TemplateEngine, TemplateSource, VariableContext, placeholder_names,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn engine_with_defaults(vault_root: Option<&Path>) -> TemplateEngine {
    let mut defaults = HashMap::new();
    defaults.insert(
        "project".to_string(),
        "---\ntype: project\ncontext: {{context}}\nstatus: active\ncreated: {{date}}\n---\n\n# {{title}}\n",
    );
    defaults.insert(
        "daily".to_string(),
        "---\ntype: daily\ndate: {{date}}\n---\n\n# {{date}} {{day_name}}\n",
    );
    defaults.insert(
        "tracking-gym".to_string(),
        "---\ntype: tracking\nroutine: {{routine}}\ndate: {{date}}\n---\n\n# Gym — {{date}}\n",
    );
    TemplateEngine::new(vault_root, defaults)
}

fn write_custom_template(vault_root: &Path, filename: &str, content: &str) {
    let template_dir = vault_root.join(".cuaderno").join("templates");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join(filename), content).unwrap();
}

// --- Template loading ---

#[test]
fn load_builtin_default_template() {
    let engine = engine_with_defaults(None);
    let (template, source) = engine.load_template("project", None).unwrap();
    assert!(template.content.contains("type: project"));
    assert_eq!(source, TemplateSource::BuiltinDefault);
}

#[test]
fn load_builtin_variant_template() {
    let engine = engine_with_defaults(None);
    let (template, source) = engine.load_template("tracking", Some("gym")).unwrap();
    assert!(template.content.contains("routine: {{routine}}"));
    assert_eq!(source, TemplateSource::BuiltinVariant);
}

#[test]
fn load_falls_back_to_base_type_when_variant_default_missing() {
    let engine = engine_with_defaults(None);
    // "tracking" without variant doesn't exist in defaults,
    // but "tracking-gym" does. Asking for variant "body" should
    // fail since neither "tracking-body" nor "tracking" exists.
    let result = engine.load_template("tracking", Some("body"));
    assert!(result.is_err());
}

#[test]
fn load_error_for_unknown_type() {
    let engine = engine_with_defaults(None);
    let result = engine.load_template("nonexistent", None);
    assert!(result.is_err());
}

#[test]
fn custom_template_overrides_builtin() {
    let dir = TempDir::new().unwrap();
    write_custom_template(
        dir.path(),
        "project.md",
        "---\ntype: project\ncustom: true\n---\n\n# {{title}}\n",
    );

    let engine = engine_with_defaults(Some(dir.path()));
    let (template, source) = engine.load_template("project", None).unwrap();
    assert!(template.content.contains("custom: true"));
    assert_eq!(source, TemplateSource::CustomBase);
}

#[test]
fn custom_variant_template_overrides_builtin() {
    let dir = TempDir::new().unwrap();
    write_custom_template(
        dir.path(),
        "tracking-gym.md",
        "---\ntype: tracking\ncustom_gym: true\n---\n\n# Gym\n",
    );

    let engine = engine_with_defaults(Some(dir.path()));
    let (template, source) = engine.load_template("tracking", Some("gym")).unwrap();
    assert!(template.content.contains("custom_gym: true"));
    assert_eq!(source, TemplateSource::CustomVariant);
}

#[test]
fn custom_type_template_used_when_variant_custom_missing() {
    let dir = TempDir::new().unwrap();
    // Only a base "tracking.md" custom template, no "tracking-gym.md"
    write_custom_template(
        dir.path(),
        "tracking.md",
        "---\ntype: tracking\ngeneric_custom: true\n---\n",
    );

    let engine = engine_with_defaults(Some(dir.path()));
    // Asking for variant "gym": no custom variant → falls to custom base type
    let (template, source) = engine.load_template("tracking", Some("gym")).unwrap();
    assert!(template.content.contains("generic_custom: true"));
    assert_eq!(source, TemplateSource::CustomBase);
}

// --- Variable resolution ---

#[test]
fn resolve_follows_tier_precedence() {
    let mut ctx = VariableContext::new();
    ctx.set_vault_level("author", "Config Author");
    ctx.set_contextual("author", "Contextual Author");
    ctx.set_builtin("author", "Builtin Author");

    // Tier 1 (builtin) wins
    assert_eq!(ctx.resolve("author"), Some("Builtin Author"));
}

#[test]
fn resolve_falls_through_tiers() {
    let mut ctx = VariableContext::new();
    ctx.set_vault_level("institution", "University");

    // Not in builtins or contextual, found in vault-level
    assert_eq!(ctx.resolve("institution"), Some("University"));
    assert_eq!(ctx.resolve("nonexistent"), None);
}

#[test]
fn load_from_config_populates_vault_level() {
    let mut ctx = VariableContext::new();
    let config = toml::from_str::<VaultConfig>(
        r#"
[variables]
author = "A. Researcher"
orcid = "0000-0000-0000-0000"
"#,
    )
    .unwrap();

    ctx.load_from_config(&config);

    assert_eq!(ctx.resolve("author"), Some("A. Researcher"));
    assert_eq!(ctx.resolve("orcid"), Some("0000-0000-0000-0000"));
}

// --- Rendering ---

#[test]
fn render_substitutes_known_variables() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "# {{title}}\nCreated: {{date}}".to_string(),
    };

    let mut ctx = VariableContext::new();
    ctx.set_contextual("title", "My Note");
    ctx.set_builtin("date", "2026-04-16");

    let prompts = HashMap::new();
    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.content, "# My Note\nCreated: 2026-04-16");
    assert!(rendered.unresolved_prompts.is_empty());
}

#[test]
fn render_leaves_unknown_variables_as_is() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "Author: {{unknown_var}}".to_string(),
    };

    let ctx = VariableContext::new();
    let prompts = HashMap::new();
    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.content, "Author: {{unknown_var}}");
}

#[test]
fn render_reports_unresolved_prompted_variables() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "Collaborators: {{collaborators}}".to_string(),
    };

    let ctx = VariableContext::new();
    let mut prompts = HashMap::new();
    prompts.insert(
        "collaborators".to_string(),
        "Who are the collaborators?".to_string(),
    );

    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.unresolved_prompts.len(), 1);
    assert_eq!(rendered.unresolved_prompts[0].0, "collaborators");
    assert_eq!(
        rendered.unresolved_prompts[0].1,
        "Who are the collaborators?"
    );
    // Placeholder kept in output
    assert!(rendered.content.contains("{{collaborators}}"));
}

#[test]
fn render_resolves_prompted_variable_when_value_provided() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "Collaborators: {{collaborators}}".to_string(),
    };

    let mut ctx = VariableContext::new();
    ctx.set_prompted("collaborators", "Alice, Bob");

    let mut prompts = HashMap::new();
    prompts.insert(
        "collaborators".to_string(),
        "Who are the collaborators?".to_string(),
    );

    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.content, "Collaborators: Alice, Bob");
    assert!(rendered.unresolved_prompts.is_empty());
}

#[test]
fn render_handles_unclosed_braces_gracefully() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "Broken {{ template here".to_string(),
    };

    let ctx = VariableContext::new();
    let prompts = HashMap::new();
    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.content, "Broken {{ template here");
}

#[test]
fn render_handles_adjacent_variables() {
    let engine = engine_with_defaults(None);
    let template = Template {
        content: "{{year}}-{{month}}-{{day_name}}".to_string(),
    };

    let mut ctx = VariableContext::new();
    ctx.set_builtin("year", "2026");
    ctx.set_builtin("month", "04");
    ctx.set_builtin("day_name", "Thursday");

    let prompts = HashMap::new();
    let rendered = engine.render(&template, &ctx, &prompts);

    assert_eq!(rendered.content, "2026-04-Thursday");
}

#[test]
fn placeholder_names_lists_in_first_appearance_order_deduped() {
    let content =
        "---\ncontext: {{context}}\ncreated: {{created}}\n---\n# {{title}}\n#tag/{{title}}\n";
    // `title` appears twice but is listed once; order follows first sighting.
    assert_eq!(
        placeholder_names(content),
        vec![
            "context".to_owned(),
            "created".to_owned(),
            "title".to_owned()
        ]
    );
}

#[test]
fn placeholder_names_trims_and_ignores_unterminated_open() {
    // Names are trimmed; a `{{` with no closing `}}` is not a placeholder,
    // mirroring `render`'s tokenisation.
    assert_eq!(
        placeholder_names("{{  spaced  }} then {{ unterminated"),
        vec!["spaced".to_owned()]
    );
}

#[test]
fn placeholder_names_empty_for_plain_text() {
    assert!(placeholder_names("no placeholders here").is_empty());
}

#[test]
fn placeholder_names_matches_what_render_substitutes() {
    // The set a template references must equal what `render` fills — guard
    // that the two tokenisers stay in lockstep.
    let engine = engine_with_defaults(None);
    let content = "{{context}} {{title}} {{context}} {{ created }}";
    let names = placeholder_names(content);

    let mut ctx = VariableContext::new();
    for name in &names {
        ctx.set_builtin(name, "X");
    }
    let rendered = engine.render(
        &Template {
            content: content.to_owned(),
        },
        &ctx,
        &HashMap::new(),
    );
    assert!(
        !rendered.content.contains("{{"),
        "every name placeholder_names reported should have been substituted: {}",
        rendered.content
    );
}
