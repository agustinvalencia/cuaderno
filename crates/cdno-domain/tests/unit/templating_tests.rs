//! Tests for template-engine wiring (#212): note creation resolves a
//! custom `.cuaderno/templates/` override through the `VaultStore`, and
//! falls back to the built-in default when none exists.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::frontmatter::Context;
use cdno_domain::{PlaceholderSource, TemplateSource, Vault};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 26).unwrap()
}

fn vault_with(custom: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    vault_with_config(custom, VaultConfig::default())
}

fn vault_with_config(custom: &[(&str, &str)], config: VaultConfig) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in custom {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    (vault, store)
}

/// A `VaultConfig` whose `[variables]` static map has `pairs`.
fn config_with_static_vars(pairs: &[(&str, &str)]) -> VaultConfig {
    let mut config = VaultConfig::default();
    for (k, v) in pairs {
        config
            .variables
            .static_vars
            .insert((*k).to_owned(), (*v).to_owned());
    }
    config
}

/// A `VaultConfig` whose `[variables.prompt]` map has `pairs` (name → prompt
/// message).
fn config_with_prompt_vars(pairs: &[(&str, &str)]) -> VaultConfig {
    let mut config = VaultConfig::default();
    for (k, v) in pairs {
        config
            .variables
            .prompt
            .insert((*k).to_owned(), (*v).to_owned());
    }
    config
}

/// A `HashMap` of caller-supplied prompted values from `pairs`.
fn prompted(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn create_uses_a_custom_type_template_override_from_the_store() {
    // A custom project template in the vault (read via the store, not
    // the filesystem) takes precedence over the built-in default.
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\n---\n# {{title}}\n\nCUSTOM PROJECT BODY\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/project.md", custom)]);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM PROJECT BODY"),
        "the custom template should be used:\n{content}"
    );
    // Built-in template variables still resolve inside the custom one.
    assert!(content.contains("# My Proj"), "{content}");
    assert!(content.contains("context: work"), "{content}");
    assert!(content.contains("status: active"), "{content}");
    // The built-in body must NOT appear.
    assert!(
        !content.contains("No work done yet"),
        "built-in body leaked:\n{content}"
    );
}

#[test]
fn create_uses_a_custom_variant_template_override_from_the_store() {
    // The store-backed loader must also honour the variant tier:
    // a custom `tracking-gym.md` wins over the built-in `tracking-gym`
    // default for a gym entry. (The core engine tests cover variant
    // precedence with the *filesystem* loader; this exercises the
    // *store* loader wired in by this PR — the new code path.)
    let custom = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: {{activity}}\ndate: {{date}}\n---\n# {{title}}\n\nCUSTOM GYM BODY\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/tracking-gym.md", custom)]);
    vault
        .create_stewardship_expanded(
            today().and_hms_opt(9, 0, 0).unwrap(),
            "Health",
            Context::Personal,
        )
        .expect("create stewardship");

    let path = vault
        .add_tracking_entry(
            today().and_hms_opt(19, 0, 0).unwrap(),
            "health",
            "gym",
            None,
            "Energy was good.",
        )
        .expect("add tracking entry");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM GYM BODY"),
        "the custom variant template should be used:\n{content}"
    );
    assert!(content.contains("activity: gym"), "{content}");
    // The built-in gym body (its exercise table) must NOT leak through.
    assert!(
        !content.contains("| Exercise | Sets | Reps"),
        "built-in gym body leaked:\n{content}"
    );
}

#[test]
fn create_falls_back_to_the_builtin_template_when_no_custom_exists() {
    // No custom template → the built-in default is used (its body text).
    let (vault, store) = vault_with(&[]);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("No work done yet"),
        "built-in template expected:\n{content}"
    );
}

#[test]
fn created_notes_have_no_unsubstituted_placeholders() {
    // The project/action/commitment creation tests assert *parsed*
    // frontmatter (order-insensitive) rather than raw output, so a ctx
    // that missed a `{{placeholder}}` would slip past them. Guard the
    // wiring directly, exercising the optional/link paths (core_question,
    // milestone/due nulls, commitment project link) most likely to drift.
    use cdno_domain::frontmatter::EnergyLevel;

    let (vault, store) = vault_with(&[]);
    let at = today().and_hms_opt(9, 0, 0).unwrap();

    let project = vault
        .create_project(today(), "Proj", Context::Work, Some("questions/q"))
        .expect("project");
    let action = vault
        .add_action_with_note(at, "proj", "Do the thing", EnergyLevel::Deep)
        .expect("action");
    let commitment = vault
        .create_commitment(at, "Promise", today(), Context::Work, Some("proj"), None)
        .expect("commitment");

    for path in [project, action, commitment] {
        let content = store.read_file(&path).unwrap();
        assert!(
            !content.contains("{{") && !content.contains("}}"),
            "unsubstituted placeholder in {path}:\n{content}"
        );
    }
}

#[test]
fn daily_creation_uses_a_custom_template_override() {
    // daily/weekly/inbox are now template-driven too (#212 PR A2): a
    // custom `.cuaderno/templates/daily.md` is honoured when the daily
    // note is first created. It must keep a `## Logs` section — the log
    // line is appended there.
    let custom = "---\ntype: daily\ndate: {{date}}\n---\n\n# {{heading}}\n\nCUSTOM DAILY PREAMBLE\n\n## Logs\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/daily.md", custom)]);

    let path = vault
        .log_to_daily_note(today().and_hms_opt(9, 0, 0).unwrap(), "first entry")
        .expect("log");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM DAILY PREAMBLE"),
        "custom daily used:\n{content}"
    );
    assert!(
        content.contains("first entry"),
        "log line appended into ## Logs:\n{content}"
    );
}

#[test]
fn daily_template_supplies_the_weekday_variable() {
    // Regression: a custom daily template titled `# {{weekday}}` must
    // render the weekday name, not the literal `{{weekday}}`. The daily
    // scaffold supplies `date`, `heading`, and `weekday`.
    let custom = "---\ntype: daily\ndate: {{date}}\n---\n\n# {{weekday}}\n\n## Logs\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/daily.md", custom)]);

    let path = vault
        .log_to_daily_note(today().and_hms_opt(9, 0, 0).unwrap(), "first entry")
        .expect("log");
    let content = store.read_file(&path).unwrap();

    // today() is 2026-04-26, a Sunday — assert the literal rather than
    // re-deriving via %A, so a coordinated format change in both source
    // and test can't pass silently.
    assert!(
        content.contains("# Sunday"),
        "weekday should render as `# Sunday`:\n{content}"
    );
    assert!(
        !content.contains("{{weekday}}"),
        "no unrendered placeholder should remain:\n{content}"
    );
}

#[test]
fn weekly_creation_uses_a_custom_template_override() {
    use cdno_domain::WeeklySection;
    let custom = "---\ntype: weekly\nweek: {{week}}\ndate_start: {{date_start}}\ndate_end: {{date_end}}\n---\n\n# Week {{week_num}}, {{year}}\n\nCUSTOM WEEKLY PREAMBLE\n\n## Wins\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/weekly.md", custom)]);

    let path = vault
        .upsert_weekly_section(today(), WeeklySection::Wins, "shipped the engine", false)
        .expect("weekly");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("CUSTOM WEEKLY PREAMBLE"),
        "custom weekly used:\n{content}"
    );
    assert!(
        content.contains("shipped the engine"),
        "section written:\n{content}"
    );
}

#[test]
fn inbox_creation_uses_a_custom_template_override() {
    // A custom inbox template adds a frontmatter field the built-in lacks.
    let custom = "---\ntype: inbox\ncreated: {{created}}\nsource: quick-capture\n---\n\n{{body}}\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/inbox.md", custom)]);

    let path = vault
        .capture_to_inbox(today().and_hms_opt(9, 0, 0).unwrap(), "a fleeting thought")
        .expect("capture");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("source: quick-capture"),
        "custom inbox used:\n{content}"
    );
    assert!(content.contains("a fleeting thought"), "body:\n{content}");
}

#[test]
fn daily_anchor_follows_a_custom_templates_last_section() {
    // PR B: the "keep last" anchor is the daily template's last section,
    // not a hardcoded `## Logs`. A custom daily template ending in
    // `## Reflection` keeps Reflection last even after a planning section
    // is added later.
    use cdno_domain::DailySection;
    let custom =
        "---\ntype: daily\ndate: {{date}}\n---\n\n# {{heading}}\n\n## Logs\n\n## Reflection\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/daily.md", custom)]);
    let at = today().and_hms_opt(9, 0, 0).unwrap();

    let path = vault.log_to_daily_note(at, "did stuff").expect("log");
    vault
        .upsert_daily_section(today(), DailySection::Meeting, "sync notes", false)
        .expect("upsert");

    let content = store.read_file(&path).unwrap();
    let reflection = content.find("## Reflection").expect("Reflection present");
    let meeting = content.find("## Meeting").expect("Meeting present");
    let logs = content.find("## Logs").expect("Logs present");
    assert!(
        reflection > meeting && reflection > logs,
        "the custom template's last section (Reflection) should stay last:\n{content}"
    );
}

// ---------------------------------------------------------------------
// Tier 3: static config `[variables]` (#238)
// ---------------------------------------------------------------------

#[test]
fn custom_template_renders_a_static_config_variable() {
    // A custom template references {{owner}}, supplied by config
    // `[variables] owner = "..."` — exercises load_from_config / vault_level.
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\nowner: {{owner}}\n---\n# {{title}}\n";
    let config = config_with_static_vars(&[("owner", "A. Researcher")]);
    let (vault, store) = vault_with_config(&[(".cuaderno/templates/project.md", custom)], config);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("owner: A. Researcher"),
        "static config variable should render:\n{content}"
    );
    // And no placeholder survives.
    assert!(!content.contains("{{owner}}"), "{content}");
}

#[test]
fn contextual_variable_beats_a_config_variable_of_the_same_name() {
    // Precedence guard: a config `[variables] context` must NOT override the
    // project's contextual `context` (tier 2 wins over tier 3).
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\n---\n# {{title}}\n";
    let config = config_with_static_vars(&[("context", "personal")]);
    let (vault, store) = vault_with_config(&[(".cuaderno/templates/project.md", custom)], config);

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("context: work"),
        "contextual value must win over the config variable:\n{content}"
    );
    assert!(!content.contains("context: personal"), "{content}");
}

#[test]
fn config_variables_do_not_leak_into_templates_that_do_not_use_them() {
    // A template with no config-referenced placeholders renders identically
    // whether or not `[variables]` is set — no accidental injection.
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\n---\n# {{title}}\n\nBODY\n";
    let (plain, plain_store) = vault_with(&[(".cuaderno/templates/project.md", custom)]);
    let (withcfg, withcfg_store) = vault_with_config(
        &[(".cuaderno/templates/project.md", custom)],
        config_with_static_vars(&[("unused", "value")]),
    );

    let p1 = plain
        .create_project(today(), "Proj", Context::Work, None)
        .unwrap();
    let p2 = withcfg
        .create_project(today(), "Proj", Context::Work, None)
        .unwrap();

    assert_eq!(
        plain_store.read_file(&p1).unwrap(),
        withcfg_store.read_file(&p2).unwrap(),
        "an unused config variable must not change rendered output"
    );
}

// ---------------------------------------------------------------------
// Tier 4: prompted config `[variables.prompt]` + caller-supplied values (#238)
// ---------------------------------------------------------------------

const PROJECT_WITH_TICKET: &str = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\nticket: {{ticket}}\n---\n# {{title}}\n";

#[test]
fn create_with_vars_renders_a_prompted_variable() {
    // A `[variables.prompt]` placeholder resolves from the caller-supplied
    // prompted map (the value the CLI gathered up front).
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) = vault_with_config(
        &[(".cuaderno/templates/project.md", PROJECT_WITH_TICKET)],
        config,
    );

    let path = vault
        .create_project_with_vars(
            today(),
            "My Proj",
            Context::Work,
            None,
            &prompted(&[("ticket", "ABC-1")]),
        )
        .expect("create project");
    let content = store.read_file(&path).unwrap();

    assert!(
        content.contains("ticket: ABC-1"),
        "prompted value should render:\n{content}"
    );
    assert!(!content.contains("{{ticket}}"), "{content}");
}

#[test]
fn create_without_a_prompted_value_errors_with_unresolved_prompts() {
    // The no-vars wrapper supplies nothing, so a prompt-defined placeholder
    // the template uses is unresolved → a clear error, not a literal
    // `{{ticket}}` left in the note.
    use cdno_domain::error::DomainError;
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, _store) = vault_with_config(
        &[(".cuaderno/templates/project.md", PROJECT_WITH_TICKET)],
        config,
    );

    let err = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect_err("should error on the unresolved prompt");
    match err {
        DomainError::UnresolvedPrompts { note_type, names } => {
            assert_eq!(note_type, "project");
            assert_eq!(names, vec!["ticket".to_owned()]);
        }
        other => panic!("expected UnresolvedPrompts, got {other:?}"),
    }
}

#[test]
fn a_static_default_satisfies_a_prompted_variable_without_a_value() {
    // A name defined in BOTH `[variables.prompt]` and `[variables]` is
    // satisfied by the static default, so the no-vars path doesn't error.
    let mut config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    config
        .variables
        .static_vars
        .insert("ticket".to_owned(), "DEFAULT-0".to_owned());
    let (vault, store) = vault_with_config(
        &[(".cuaderno/templates/project.md", PROJECT_WITH_TICKET)],
        config,
    );

    let path = vault
        .create_project(today(), "My Proj", Context::Work, None)
        .expect("static default should resolve the prompt");
    let content = store.read_file(&path).unwrap();
    assert!(content.contains("ticket: DEFAULT-0"), "{content}");
}

#[test]
fn template_prompts_reports_only_unsatisfied_prompted_names() {
    // `template_prompts` is the "what to ask" query the CLI uses. It lists a
    // prompt-defined name the template uses...
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, _store) = vault_with_config(
        &[(".cuaderno/templates/project.md", PROJECT_WITH_TICKET)],
        config,
    );
    let needed = vault.template_prompts("project", None).expect("prompts");
    assert_eq!(needed, vec![("ticket".to_owned(), "Ticket?".to_owned())]);

    // ...but excludes one a static `[variables]` already satisfies.
    let mut config2 = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    config2
        .variables
        .static_vars
        .insert("ticket".to_owned(), "DEFAULT-0".to_owned());
    let (vault2, _s2) = vault_with_config(
        &[(".cuaderno/templates/project.md", PROJECT_WITH_TICKET)],
        config2,
    );
    assert!(
        vault2.template_prompts("project", None).unwrap().is_empty(),
        "a statically-defaulted prompt name should not be asked for"
    );
}

#[test]
fn template_prompts_ignores_prompt_names_the_template_does_not_use() {
    // A `[variables.prompt]` entry whose `{{name}}` isn't in the template is
    // not reported (and won't block creation).
    let custom = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\n---\n# {{title}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, _store) = vault_with_config(&[(".cuaderno/templates/project.md", custom)], config);
    assert!(
        vault.template_prompts("project", None).unwrap().is_empty(),
        "an unused prompt name should not be reported"
    );
}

// The prompt machinery lives in the shared `scaffold`, so `project` proves
// the core path. These cover the `*_with_vars` entry points that reach
// `scaffold` through their own render helper or a non-obvious path — commitment,
// evidence, the tracking variant, promotion, and the action-note branch — so a
// missing `set_prompted` in one of those can't ship silently. The remaining
// creators (portfolio, question, stewardship) are plain single-scaffold calls
// structurally identical to `project`, so `project` already covers them.

#[test]
fn create_commitment_with_vars_renders_a_prompted_variable() {
    let custom = "---\ntype: commitment\ntitle: {{title}}\nstatus: {{status}}\ndue: {{due}}\ncontext: {{context}}\nproject: {{project}}\nstewardship: {{stewardship}}\ncreated: {{created}}\ncompleted: {{completed}}\nticket: {{ticket}}\n---\n# {{title}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) =
        vault_with_config(&[(".cuaderno/templates/commitment.md", custom)], config);
    let at = today().and_hms_opt(9, 0, 0).unwrap();

    let path = vault
        .create_commitment_with_vars(
            at,
            "Promise",
            today(),
            Context::Work,
            None,
            None,
            &prompted(&[("ticket", "ABC-9")]),
        )
        .expect("commitment");
    assert!(
        store.read_file(&path).unwrap().contains("ticket: ABC-9"),
        "commitment prompted value should render"
    );
}

#[test]
fn file_evidence_with_vars_renders_a_prompted_variable() {
    let custom = "---\ntype: evidence\nsource: {{source}}\norigin: {{origin}}\nportfolio: {{portfolio}}\ncreated: {{created}}\nticket: {{ticket}}\n---\n{{content}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) = vault_with_config(&[(".cuaderno/templates/evidence.md", custom)], config);
    let at = today().and_hms_opt(9, 0, 0).unwrap();
    vault
        .create_portfolio(at, "Sparse vs dense", None)
        .expect("portfolio");

    let path = vault
        .file_evidence_with_vars(
            at,
            "sparse-vs-dense",
            "Chen 2025",
            "projects/foo",
            "Body.",
            &prompted(&[("ticket", "EV-3")]),
        )
        .expect("evidence");
    assert!(
        store.read_file(&path).unwrap().contains("ticket: EV-3"),
        "evidence prompted value should render"
    );
}

#[test]
fn add_tracking_entry_with_vars_renders_a_prompted_variable_from_a_variant_template() {
    // Covers both the tracking `*_with_vars` path AND variant resolution:
    // the prompt lives in `tracking-gym.md`, the variant the gym entry uses.
    let custom = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: {{activity}}\ndate: {{date}}\nticket: {{ticket}}\n---\n# {{activity_title}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) =
        vault_with_config(&[(".cuaderno/templates/tracking-gym.md", custom)], config);
    vault
        .create_stewardship_expanded(
            today().and_hms_opt(9, 0, 0).unwrap(),
            "Health",
            Context::Personal,
        )
        .expect("stewardship");

    // `template_prompts` for the variant reports the prompt...
    assert_eq!(
        vault.template_prompts("tracking", Some("gym")).unwrap(),
        vec![("ticket".to_owned(), "Ticket?".to_owned())]
    );
    // ...and the create path renders the supplied value.
    let (path, source) = vault
        .add_tracking_entry_with_vars(
            today().and_hms_opt(19, 0, 0).unwrap(),
            "health",
            "gym",
            None,
            "Good session.",
            &prompted(&[("ticket", "GYM-1")]),
        )
        .expect("tracking");
    assert!(
        store.read_file(&path).unwrap().contains("ticket: GYM-1"),
        "tracking-variant prompted value should render"
    );
    // A custom `tracking-gym.md` override resolves as a custom variant (#287).
    assert_eq!(source, TemplateSource::CustomVariant);
}

#[test]
fn add_tracking_entry_reports_the_resolved_template_source() {
    // #287 — the create path reports which template rung it resolved, so the
    // CLI hint keys off the real outcome instead of re-deriving it. Three rungs
    // reachable today: no custom template → the generic built-in
    // (`BuiltinDefault`, the only case the newcomer hint fires); a custom base
    // `tracking.md` → `CustomBase`; a custom `tracking-<activity>.md` →
    // `CustomVariant`. (`BuiltinVariant` needs a bundled variant default, which
    // none ship — covered at the engine level in `template_tests.rs`.)
    let at = today().and_hms_opt(19, 0, 0).unwrap();
    let steward = |vault: &Vault| {
        vault
            .create_stewardship_expanded(
                today().and_hms_opt(9, 0, 0).unwrap(),
                "Health",
                Context::Personal,
            )
            .expect("stewardship");
    };

    // No custom template → the generic built-in (the only case the hint fires).
    let (vault, _store) = vault_with(&[]);
    steward(&vault);
    let (_p, source) = vault
        .add_tracking_entry_with_vars(at, "health", "gym", None, "Session.", &prompted(&[]))
        .expect("tracking");
    assert_eq!(source, TemplateSource::BuiltinDefault);

    // A custom base `tracking.md` (no variant-specific file) → CustomBase.
    let base = "---\ntype: tracking\nstewardship: {{stewardship}}\nactivity: {{activity}}\ndate: {{date}}\n---\n# {{activity_title}}\n";
    let (vault, _store) = vault_with(&[(".cuaderno/templates/tracking.md", base)]);
    steward(&vault);
    let (_p, source) = vault
        .add_tracking_entry_with_vars(at, "health", "gym", None, "Session.", &prompted(&[]))
        .expect("tracking");
    assert_eq!(source, TemplateSource::CustomBase);
}

#[test]
fn promote_action_with_vars_renders_a_prompted_variable() {
    // Promotion scaffolds an action note, so it must thread prompted vars
    // too (regression guard for the promote path).
    use cdno_domain::frontmatter::EnergyLevel;
    let custom = "---\ntype: action\nstatus: {{status}}\nproject: {{project}}\nenergy: {{energy}}\nmilestone: {{milestone}}\ndue: {{due}}\ncreated: {{created}}\ncompleted: {{completed}}\nblocker: {{blocker}}\ncriteria: {{criteria}}\ntags: {{tags}}\nticket: {{ticket}}\n---\n# {{title}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) = vault_with_config(&[(".cuaderno/templates/action.md", custom)], config);
    let at = today().and_hms_opt(9, 0, 0).unwrap();
    vault
        .create_project(today(), "Proj", Context::Work, None)
        .expect("project");
    vault
        .add_action(at, "proj", "Profile the assembly", EnergyLevel::Deep)
        .expect("bullet");

    let path = vault
        .promote_action_with_vars(at, "proj", "profile", &prompted(&[("ticket", "PR-1")]))
        .expect("promote");
    assert!(
        store.read_file(&path).unwrap().contains("ticket: PR-1"),
        "promoted action note should carry the prompted value"
    );
}

#[test]
fn add_action_with_note_and_vars_renders_a_prompted_variable() {
    // The `add_action --note` / MCP `with_note: true` branch scaffolds an
    // action note from the same template as promotion, so it must thread
    // prompted vars too (regression guard for the note-creating branch).
    use cdno_domain::frontmatter::EnergyLevel;
    let custom = "---\ntype: action\nstatus: {{status}}\nproject: {{project}}\nenergy: {{energy}}\nmilestone: {{milestone}}\ndue: {{due}}\ncreated: {{created}}\ncompleted: {{completed}}\nblocker: {{blocker}}\ncriteria: {{criteria}}\ntags: {{tags}}\nticket: {{ticket}}\n---\n# {{title}}\n";
    let config = config_with_prompt_vars(&[("ticket", "Ticket?")]);
    let (vault, store) = vault_with_config(&[(".cuaderno/templates/action.md", custom)], config);
    let at = today().and_hms_opt(9, 0, 0).unwrap();
    vault
        .create_project(today(), "Proj", Context::Work, None)
        .expect("project");

    let path = vault
        .add_action_with_note_and_vars(
            at,
            "proj",
            "Profile the assembly",
            EnergyLevel::Deep,
            &prompted(&[("ticket", "AN-1")]),
        )
        .expect("add action with note");
    assert!(
        store.read_file(&path).unwrap().contains("ticket: AN-1"),
        "action note should carry the prompted value"
    );
}

// ---------------------------------------------------------------------
// template_placeholders — discovery of a type's supported {{placeholders}} (#271)
// ---------------------------------------------------------------------

/// Convenience: the placeholder names of a given `source`, in order.
fn names_with_source(
    placeholders: &[cdno_domain::TemplatePlaceholder],
    want: &PlaceholderSource,
) -> Vec<String> {
    placeholders
        .iter()
        .filter(|p| &p.source == want)
        .map(|p| p.name.clone())
        .collect()
}

#[test]
fn template_placeholders_lists_the_project_supplied_set() {
    // The type's complete create-path key set (#279), in registry order, all
    // classified `Supplied`.
    let (vault, _store) = vault_with(&[]);
    let placeholders = vault.template_placeholders("project").unwrap();

    let supplied = names_with_source(&placeholders, &PlaceholderSource::Supplied);
    assert_eq!(
        supplied,
        vec![
            "title".to_owned(),
            "context".to_owned(),
            "status".to_owned(),
            "created".to_owned(),
            "core_question".to_owned()
        ],
        "project supplies its complete create-path key set"
    );
    // A default vault has no config vars, so nothing else is listed.
    assert_eq!(placeholders.len(), supplied.len());
}

#[test]
fn template_placeholders_classifies_config_and_prompt_vars() {
    // `[variables]` -> Config, `[variables.prompt]` -> Prompt (with message),
    // appended after the supplied set.
    let mut config = config_with_static_vars(&[("author", "A. Researcher")]);
    config
        .variables
        .prompt
        .insert("ticket".to_owned(), "Ticket ID?".to_owned());
    let (vault, _store) = vault_with_config(&[], config);

    let placeholders = vault.template_placeholders("project").unwrap();
    assert_eq!(
        names_with_source(&placeholders, &PlaceholderSource::Config),
        vec!["author".to_owned()]
    );
    assert_eq!(
        placeholders
            .iter()
            .find(|p| p.name == "ticket")
            .map(|p| &p.source),
        Some(&PlaceholderSource::Prompt {
            message: "Ticket ID?".to_owned()
        })
    );
}

#[test]
fn template_placeholders_omits_a_config_name_shadowed_by_a_supplied_key() {
    // A config/prompt var named like a supplied key never takes effect
    // (contextual value shadows it), so it isn't double-listed.
    let mut config = VaultConfig::default();
    config
        .variables
        .prompt
        .insert("title".to_owned(), "won't fire".to_owned());
    config
        .variables
        .static_vars
        .insert("context".to_owned(), "won't fire".to_owned());
    let (vault, _store) = vault_with_config(&[], config);

    let placeholders = vault.template_placeholders("project").unwrap();
    // `title` and `context` appear once each, and as Supplied.
    assert_eq!(placeholders.iter().filter(|p| p.name == "title").count(), 1);
    assert!(
        placeholders
            .iter()
            .all(|p| p.source == PlaceholderSource::Supplied),
        "shadowed config/prompt names are omitted, leaving only supplied keys"
    );
}

#[test]
fn template_placeholders_tracking_lists_the_complete_supplied_set() {
    // #279: the supplied set is the type's full create-path key set, so it
    // includes `routine` (which the generic built-in template doesn't
    // reference). Exact ordered vec — also guards against the registry
    // *over*-reporting a key the create path doesn't fill.
    let (vault, _store) = vault_with(&[]);
    let names: Vec<String> = vault
        .template_placeholders("tracking")
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(
        names,
        vec![
            "stewardship",
            "activity",
            "activity_title",
            "routine",
            "content",
            "date",
            "date_long"
        ]
    );
}

#[test]
fn template_placeholders_errors_on_unknown_type() {
    use cdno_domain::error::DomainError;
    let (vault, _store) = vault_with(&[]);
    match vault.template_placeholders("bogus") {
        Err(DomainError::UnknownNoteType { note_type }) => assert_eq!(note_type, "bogus"),
        other => panic!("expected UnknownNoteType, got {other:?}"),
    }
}

#[test]
fn template_placeholders_classifies_a_name_in_both_config_sources_as_config() {
    // A name under BOTH `[variables]` and `[variables.prompt]` has a static
    // default that suppresses the prompt at creation, so it's `Config`, not
    // `Prompt` — matching scaffold's resolve precedence.
    let mut config = config_with_static_vars(&[("author", "Default Author")]);
    config
        .variables
        .prompt
        .insert("author".to_owned(), "Author?".to_owned());
    let (vault, _store) = vault_with_config(&[], config);

    let placeholders = vault.template_placeholders("project").unwrap();
    let author = placeholders
        .iter()
        .find(|p| p.name == "author")
        .expect("author listed");
    assert_eq!(author.source, PlaceholderSource::Config);
    assert_eq!(
        placeholders.iter().filter(|p| p.name == "author").count(),
        1,
        "listed once, not once per source"
    );
}

// ---------------------------------------------------------------------
// eject_template — materialise a built-in for customisation (#270)
// ---------------------------------------------------------------------

#[test]
fn eject_template_writes_the_builtin_into_the_vault() {
    let (vault, store) = vault_with(&[]);
    let path = vault.eject_template("project", None, false).expect("eject");
    assert_eq!(path.to_string(), ".cuaderno/templates/project.md");

    let content = store.read_file(&path).unwrap();
    // The built-in project template's distinctive markers.
    assert!(content.contains("# {{title}}"), "content:\n{content}");
    assert!(content.contains("## Current State"), "content:\n{content}");
    assert!(content.contains("No work done yet"), "content:\n{content}");
}

#[test]
fn eject_template_writes_the_generic_tracking_template() {
    // Only the generic tracking template ships built-in (no variants), so the
    // base `tracking` type ejects; a `--variant` would error (covered below).
    let (vault, store) = vault_with(&[]);
    let path = vault
        .eject_template("tracking", None, false)
        .expect("eject tracking");
    assert_eq!(path.to_string(), ".cuaderno/templates/tracking.md");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.contains("# {{activity_title}}"),
        "content:\n{content}"
    );
}

#[test]
fn eject_template_refuses_to_clobber_without_force() {
    use cdno_domain::error::DomainError;
    let custom = "---\ntype: project\n---\n# mine\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/project.md", custom)]);

    match vault.eject_template("project", None, false) {
        Err(DomainError::TemplateAlreadyExists { path }) => {
            assert_eq!(path, ".cuaderno/templates/project.md")
        }
        other => panic!("expected TemplateAlreadyExists, got {other:?}"),
    }
    // The user's template is untouched.
    assert_eq!(
        store
            .read_file(&vp(".cuaderno/templates/project.md"))
            .unwrap(),
        custom
    );
}

#[test]
fn eject_template_force_overwrites_an_existing_custom() {
    let custom = "---\ntype: project\n---\n# mine\n";
    let (vault, store) = vault_with(&[(".cuaderno/templates/project.md", custom)]);

    vault
        .eject_template("project", None, true)
        .expect("force eject");
    let content = store
        .read_file(&vp(".cuaderno/templates/project.md"))
        .unwrap();
    assert!(
        content.contains("## Current State"),
        "overwritten with built-in"
    );
    assert!(!content.contains("# mine"), "custom content replaced");
}

#[test]
fn eject_template_unknown_variant_errors_without_falling_back() {
    use cdno_domain::error::DomainError;
    let (vault, store) = vault_with(&[]);
    match vault.eject_template("tracking", Some("deadlift"), false) {
        Err(DomainError::UnknownTemplateVariant { note_type, variant }) => {
            assert_eq!(note_type, "tracking");
            assert_eq!(variant, "deadlift");
        }
        other => panic!("expected UnknownTemplateVariant, got {other:?}"),
    }
    // Nothing written on the error path.
    assert!(
        !store
            .exists(&vp(".cuaderno/templates/tracking-deadlift.md"))
            .unwrap()
    );
}

#[test]
fn template_placeholders_reports_keys_the_default_template_omits() {
    // #279: the supplied set is the complete create-path key set, not just what
    // the built-in template references. `daily`'s create path sets `weekday`
    // even though the default `daily.md` doesn't use it — it's still reported.
    let (vault, _store) = vault_with(&[]);
    let names: Vec<String> = vault
        .template_placeholders("daily")
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    // Exact vec: `weekday` is present (create-path-supplied though daily.md
    // omits it), and no key is over-reported.
    assert_eq!(names, vec!["date", "heading", "weekday"]);
}

#[test]
fn built_in_templates_only_reference_supplied_placeholders() {
    // Drift guard (#279): every `{{placeholder}}` a built-in template uses must
    // be in that type's supplied set, so `supplied_placeholders` can never fall
    // behind the templates and a template can never reference a key the create
    // path doesn't fill (which would render literally). Reads the template
    // files the same way the frontmatter-order sync test does.
    use cdno_domain::note_type::NoteType;
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");
    let cases = [
        (NoteType::Project, "project.md"),
        (NoteType::Action, "action.md"),
        (NoteType::Question, "question.md"),
        (NoteType::Stewardship, "stewardship.md"),
        (NoteType::Portfolio, "portfolio.md"),
        (NoteType::Evidence, "evidence.md"),
        (NoteType::Commitment, "commitment.md"),
        (NoteType::Tracking, "tracking/generic.md"),
        (NoteType::Daily, "daily.md"),
        (NoteType::Weekly, "weekly.md"),
        (NoteType::Monthly, "monthly.md"),
        (NoteType::Inbox, "inbox.md"),
    ];
    for (nt, file) in cases {
        let raw = std::fs::read_to_string(format!("{dir}/{file}"))
            .unwrap_or_else(|e| panic!("read template {file}: {e}"));
        let supplied = nt.supplied_placeholders();
        for name in cdno_core::template::placeholder_names(&raw) {
            assert!(
                supplied.contains(&name.as_str()),
                "template {file} references {{{{{name}}}}} which is not in {nt:?}'s supplied set {supplied:?}"
            );
        }
    }
}

#[test]
fn every_supplied_placeholder_is_filled_by_the_create_path() {
    // #285 — the complement of the drift guard: create a note of every type
    // from a custom template that references *every* `supplied_placeholders()`
    // key, and assert nothing renders as a literal `{{...}}`. This proves the
    // registry never over-advertises — every key it lists is genuinely
    // `set_contextual`'d by the create path (a future registry key with no
    // matching `set_contextual` would survive as a literal and fail here).
    use cdno_domain::frontmatter::{EnergyLevel, QuestionDomain};
    use cdno_domain::note_type::NoteType;
    use cdno_domain::{MonthlySection, WeeklySection};

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");
    // A custom template = the built-in with an HTML comment inserted right after
    // the frontmatter that references every supplied key. The comment sits
    // before the H1/sections, so it never disturbs the section-append logic
    // (daily `## Logs`, weekly `## Wins`); its `{{key}}`s still substitute.
    let custom = |nt: NoteType, file: &str| -> String {
        let builtin = std::fs::read_to_string(format!("{dir}/{file}"))
            .unwrap_or_else(|e| panic!("read template {file}: {e}"));
        let keys = nt
            .supplied_placeholders()
            .iter()
            .map(|k| format!("{{{{{k}}}}}"))
            .collect::<Vec<_>>()
            .join(" ");
        let (front, body) = builtin
            .split_once("\n---\n")
            .expect("template has a frontmatter block");
        format!("{front}\n---\n<!-- every supplied key: {keys} -->\n{body}")
    };

    // (seed filename under `.cuaderno/templates/`, built-in source file). An
    // exhaustive match so adding a note type is a compile error here — the
    // guard can't silently skip a new type. `tracking` seeds its `gym` variant
    // because the create call below tracks activity "gym"; every other type
    // seeds its base `<key>.md`.
    let seed_spec = |nt: NoteType| -> (String, &'static str) {
        match nt {
            NoteType::Project => ("project.md".into(), "project.md"),
            NoteType::Action => ("action.md".into(), "action.md"),
            NoteType::Question => ("question.md".into(), "question.md"),
            NoteType::Portfolio => ("portfolio.md".into(), "portfolio.md"),
            NoteType::Evidence => ("evidence.md".into(), "evidence.md"),
            NoteType::Stewardship => ("stewardship.md".into(), "stewardship.md"),
            NoteType::Tracking => ("tracking-gym.md".into(), "tracking/generic.md"),
            NoteType::Commitment => ("commitment.md".into(), "commitment.md"),
            NoteType::Daily => ("daily.md".into(), "daily.md"),
            NoteType::Weekly => ("weekly.md".into(), "weekly.md"),
            NoteType::Monthly => ("monthly.md".into(), "monthly.md"),
            NoteType::Inbox => ("inbox.md".into(), "inbox.md"),
        }
    };
    let customs: Vec<(String, String)> = NoteType::ALL
        .iter()
        .map(|&nt| {
            let (seed, src) = seed_spec(nt);
            (format!(".cuaderno/templates/{seed}"), custom(nt, src))
        })
        .collect();
    let custom_refs: Vec<(&str, &str)> = customs
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    // `vault_with` seeds an empty config: no `[variables]`, so a supplied key
    // can never be masked by a static config var of the same name filling the
    // placeholder in its stead — the only paths that resolve a key here are the
    // create paths' `set_contextual` calls, which is exactly what we're guarding.
    let (vault, store) = vault_with(&custom_refs);

    let at = today().and_hms_opt(9, 0, 0).unwrap();
    // Created in dependency order (a `vec!` evaluates left-to-right): project
    // before action/commitment; expanded stewardship before tracking; portfolio
    // before evidence. Each `.expect` reaches the real create path — a missing
    // prerequisite panics loudly rather than silently skipping a type.
    let paths = vec![
        vault
            .create_project(today(), "Proj", Context::Work, Some("questions/q"))
            .expect("project"),
        vault
            .add_action_with_note(at, "proj", "Do the thing", EnergyLevel::Deep)
            .expect("action"),
        vault
            .create_question(at, QuestionDomain::Research, "Does it hold?")
            .expect("question"),
        vault
            .create_portfolio(at, "Sparse vs dense", None)
            .expect("portfolio"),
        vault
            .file_evidence(at, "sparse-vs-dense", "Chen 2025", "projects/proj", "Body.")
            .expect("evidence"),
        vault
            .create_stewardship_expanded(at, "Health", Context::Personal)
            .expect("stewardship"),
        vault
            .add_tracking_entry(
                today().and_hms_opt(19, 0, 0).unwrap(),
                "health",
                "gym",
                Some("upper-body-a"),
                "Session.",
            )
            .expect("tracking"),
        vault
            .create_commitment(
                at,
                "Promise",
                today(),
                Context::Work,
                Some("proj"),
                Some("health"),
            )
            .expect("commitment"),
        vault.log_to_daily_note(at, "entry").expect("daily"),
        vault
            .upsert_weekly_section(today(), WeeklySection::Wins, "shipped", false)
            .expect("weekly"),
        vault
            .upsert_monthly_section(today(), MonthlySection::Wins, "shipped", false)
            .expect("monthly"),
        vault.capture_to_inbox(at, "thought").expect("inbox"),
    ];

    // Pair the exhaustive seed match: every type must also be created and
    // asserted. If a new type is added and seeded but its create call is
    // forgotten, this trips instead of the type going silently unguarded.
    assert_eq!(
        paths.len(),
        NoteType::ALL.len(),
        "every note type must be created and asserted — add the new type's create call above"
    );

    for path in paths {
        let content = store.read_file(&path).unwrap();
        assert!(
            !content.contains("{{") && !content.contains("}}"),
            "unsubstituted placeholder in {path} — a supplied key isn't filled by the create path:\n{content}"
        );
    }
}
