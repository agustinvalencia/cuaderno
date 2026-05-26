use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use cdno_domain::frontmatter::{
    ActionFrontmatter, ActionStatus, CommitmentFrontmatter, CommitmentStatus, Context, EnergyLevel,
    ProjectFrontmatter, ProjectStatus,
};

fn parse_fm(yaml_body: &str) -> Frontmatter {
    let raw = format!("---\n{yaml_body}---\n");
    let (fm, _body) = Frontmatter::parse(&raw).expect("frontmatter parses");
    fm
}

// ---------------------------------------------------------------------
// Context — kebab-case YAML round-trip for every variant
// ---------------------------------------------------------------------

#[test]
fn context_serialises_as_kebab_case() {
    assert_eq!(
        serde_yaml::to_string(&Context::Work).unwrap().trim(),
        "work"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::SideProject).unwrap().trim(),
        "side-project"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::University).unwrap().trim(),
        "university"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::Family).unwrap().trim(),
        "family"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::Household).unwrap().trim(),
        "household"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::Legal).unwrap().trim(),
        "legal"
    );
    assert_eq!(
        serde_yaml::to_string(&Context::Personal).unwrap().trim(),
        "personal"
    );
}

#[test]
fn context_deserialises_from_kebab_case() {
    let cases = [
        ("work", Context::Work),
        ("side-project", Context::SideProject),
        ("university", Context::University),
        ("family", Context::Family),
        ("household", Context::Household),
        ("legal", Context::Legal),
        ("personal", Context::Personal),
    ];
    for (input, expected) in cases {
        let got: Context = serde_yaml::from_str(input).expect("parses");
        assert_eq!(got, expected, "input={input}");
    }
}

#[test]
fn context_as_str_returns_kebab_case() {
    let cases = [
        (Context::Work, "work"),
        (Context::SideProject, "side-project"),
        (Context::University, "university"),
        (Context::Family, "family"),
        (Context::Household, "household"),
        (Context::Legal, "legal"),
        (Context::Personal, "personal"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.as_str(), expected, "variant={variant:?}");
    }
}

#[test]
fn context_rejects_unknown_value() {
    let err = serde_yaml::from_str::<Context>("studies");
    assert!(err.is_err(), "expected error for unknown context");
}

#[test]
fn context_rejects_legacy_home_family_value() {
    // Pre-§5.10 vocabulary. Make sure it doesn't sneak back in.
    let err = serde_yaml::from_str::<Context>("home-family");
    assert!(err.is_err(), "expected error for legacy 'home-family'");
}

// ---------------------------------------------------------------------
// ProjectStatus — kebab-case YAML round-trip for every variant
// ---------------------------------------------------------------------

#[test]
fn project_status_serialises_as_kebab_case() {
    assert_eq!(
        serde_yaml::to_string(&ProjectStatus::Active)
            .unwrap()
            .trim(),
        "active"
    );
    assert_eq!(
        serde_yaml::to_string(&ProjectStatus::Parked)
            .unwrap()
            .trim(),
        "parked"
    );
    assert_eq!(
        serde_yaml::to_string(&ProjectStatus::Completed)
            .unwrap()
            .trim(),
        "completed"
    );
}

#[test]
fn project_status_deserialises_from_kebab_case() {
    assert_eq!(
        serde_yaml::from_str::<ProjectStatus>("active").unwrap(),
        ProjectStatus::Active
    );
    assert_eq!(
        serde_yaml::from_str::<ProjectStatus>("parked").unwrap(),
        ProjectStatus::Parked
    );
    assert_eq!(
        serde_yaml::from_str::<ProjectStatus>("completed").unwrap(),
        ProjectStatus::Completed
    );
}

#[test]
fn project_status_rejects_unknown_value() {
    assert!(serde_yaml::from_str::<ProjectStatus>("archived").is_err());
}

#[test]
fn project_status_as_str_returns_kebab_case() {
    let cases = [
        (ProjectStatus::Active, "active"),
        (ProjectStatus::Parked, "parked"),
        (ProjectStatus::Completed, "completed"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.as_str(), expected, "variant={variant:?}");
    }
}

// ---------------------------------------------------------------------
// EnergyLevel — kebab-case YAML round-trip + as_str
// ---------------------------------------------------------------------

#[test]
fn energy_level_serialises_as_kebab_case() {
    assert_eq!(
        serde_yaml::to_string(&EnergyLevel::Deep).unwrap().trim(),
        "deep"
    );
    assert_eq!(
        serde_yaml::to_string(&EnergyLevel::Medium).unwrap().trim(),
        "medium"
    );
    assert_eq!(
        serde_yaml::to_string(&EnergyLevel::Light).unwrap().trim(),
        "light"
    );
}

#[test]
fn energy_level_deserialises_from_kebab_case() {
    assert_eq!(
        serde_yaml::from_str::<EnergyLevel>("deep").unwrap(),
        EnergyLevel::Deep
    );
    assert_eq!(
        serde_yaml::from_str::<EnergyLevel>("medium").unwrap(),
        EnergyLevel::Medium
    );
    assert_eq!(
        serde_yaml::from_str::<EnergyLevel>("light").unwrap(),
        EnergyLevel::Light
    );
}

#[test]
fn energy_level_rejects_unknown_value() {
    assert!(serde_yaml::from_str::<EnergyLevel>("intense").is_err());
}

#[test]
fn energy_level_as_str_returns_kebab_case() {
    let cases = [
        (EnergyLevel::Deep, "deep"),
        (EnergyLevel::Medium, "medium"),
        (EnergyLevel::Light, "light"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.as_str(), expected, "variant={variant:?}");
    }
}

// ---------------------------------------------------------------------
// FromStr impls — used by clap to parse CLI args. Subprocess tests
// don't reach these on Linux tarpaulin (subprocess code isn't
// instrumented), so we cover them directly here.
// ---------------------------------------------------------------------

#[test]
fn context_from_str_parses_every_kebab_case_variant() {
    for variant in cdno_domain::frontmatter::Context::ALL {
        let parsed: Context = variant
            .as_str()
            .parse()
            .expect("kebab-case round-trips through FromStr");
        assert_eq!(parsed, variant);
    }
}

#[test]
fn context_from_str_rejects_unknown_value_with_helpful_error() {
    let err = "studies"
        .parse::<Context>()
        .expect_err("unknown context must reject");
    let msg = format!("{err}");
    assert!(msg.contains("studies"), "error names input: {msg}");
}

#[test]
fn energy_level_from_str_parses_every_kebab_case_variant() {
    for variant in cdno_domain::frontmatter::EnergyLevel::ALL {
        let parsed: EnergyLevel = variant
            .as_str()
            .parse()
            .expect("kebab-case round-trips through FromStr");
        assert_eq!(parsed, variant);
    }
}

#[test]
fn energy_level_from_str_rejects_unknown_value_with_helpful_error() {
    let err = "intense"
        .parse::<EnergyLevel>()
        .expect_err("unknown energy must reject");
    let msg = format!("{err}");
    assert!(msg.contains("intense"), "error names input: {msg}");
}

// ---------------------------------------------------------------------
// ProjectFrontmatter::try_from
// ---------------------------------------------------------------------

#[test]
fn try_from_parses_a_complete_frontmatter() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: work\n",
        "status: active\n",
        "created: 2026-01-15\n",
        "core_question: \"[[questions/research/foo]]\"\n",
    ));

    let parsed = ProjectFrontmatter::try_from(fm).expect("valid frontmatter");

    assert_eq!(parsed.context, Context::Work);
    assert_eq!(parsed.status, ProjectStatus::Active);
    assert_eq!(
        parsed.created,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    );
    assert_eq!(
        parsed.core_question.as_deref(),
        Some("[[questions/research/foo]]")
    );
}

#[test]
fn try_from_accepts_missing_optional_core_question() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: personal\n",
        "status: parked\n",
        "created: 2026-04-01\n",
    ));

    let parsed = ProjectFrontmatter::try_from(fm).expect("valid frontmatter");

    assert_eq!(parsed.context, Context::Personal);
    assert_eq!(parsed.status, ProjectStatus::Parked);
    assert!(parsed.core_question.is_none());
}

#[test]
fn try_from_rejects_missing_context() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "status: active\n",
        "created: 2026-01-15\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "context"),
        "got {err:?}"
    );
}

#[test]
fn try_from_rejects_missing_status() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: work\n",
        "created: 2026-01-15\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "status"),
        "got {err:?}"
    );
}

#[test]
fn try_from_rejects_missing_created() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: work\n",
        "status: active\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "created"),
        "got {err:?}"
    );
}

#[test]
fn try_from_rejects_invalid_status_value() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: work\n",
        "status: archived\n",
        "created: 2026-01-15\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidField { ref field, .. } if field == "status"),
        "got {err:?}"
    );
}

#[test]
fn try_from_rejects_invalid_context_value() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: home-family\n",
        "status: active\n",
        "created: 2026-01-15\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidField { ref field, .. } if field == "context"),
        "got {err:?}"
    );
}

#[test]
fn try_from_rejects_malformed_created_date() {
    let fm = parse_fm(concat!(
        "type: project\n",
        "context: work\n",
        "status: active\n",
        "created: not-a-date\n",
    ));

    let err = ProjectFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidField { ref field, .. } if field == "created"),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------
// CommitmentStatus — kebab-case YAML round-trip + as_str + FromStr
// ---------------------------------------------------------------------

#[test]
fn commitment_status_serialises_as_kebab_case() {
    assert_eq!(
        serde_yaml::to_string(&CommitmentStatus::Active)
            .unwrap()
            .trim(),
        "active"
    );
    assert_eq!(
        serde_yaml::to_string(&CommitmentStatus::Completed)
            .unwrap()
            .trim(),
        "completed"
    );
}

#[test]
fn commitment_status_deserialises_from_kebab_case() {
    assert_eq!(
        serde_yaml::from_str::<CommitmentStatus>("active").unwrap(),
        CommitmentStatus::Active
    );
    assert_eq!(
        serde_yaml::from_str::<CommitmentStatus>("completed").unwrap(),
        CommitmentStatus::Completed
    );
}

#[test]
fn commitment_status_as_str_returns_kebab_case() {
    let cases = [
        (CommitmentStatus::Active, "active"),
        (CommitmentStatus::Completed, "completed"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.as_str(), expected, "variant={variant:?}");
    }
}

#[test]
fn commitment_status_from_str_parses_every_variant() {
    for variant in CommitmentStatus::ALL {
        let parsed: CommitmentStatus = variant.as_str().parse().expect("round-trip");
        assert_eq!(parsed, variant);
    }
}

#[test]
fn commitment_status_from_str_rejects_unknown_value() {
    let err = "archived"
        .parse::<CommitmentStatus>()
        .expect_err("unknown status must reject");
    assert!(format!("{err}").contains("archived"));
}

// ---------------------------------------------------------------------
// CommitmentFrontmatter::try_from
// ---------------------------------------------------------------------

#[test]
fn commitment_try_from_parses_active_state() {
    let fm = parse_fm(concat!(
        "type: commitment\n",
        "status: active\n",
        "due: 2026-06-30\n",
        "created: 2026-05-01\n",
        "completed: null\n",
        "context: personal\n",
        "project: null\n",
        "stewardship: null\n",
    ));

    let parsed = CommitmentFrontmatter::try_from(fm).expect("valid active commitment");

    assert_eq!(parsed.status, CommitmentStatus::Active);
    assert_eq!(
        parsed.due,
        chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()
    );
    assert_eq!(
        parsed.created,
        chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()
    );
    assert!(parsed.completed.is_none());
    assert_eq!(parsed.context, Context::Personal);
    assert!(parsed.project.is_none());
    assert!(parsed.stewardship.is_none());
}

#[test]
fn commitment_try_from_parses_completed_state() {
    let fm = parse_fm(concat!(
        "type: commitment\n",
        "status: completed\n",
        "due: 2026-06-30\n",
        "created: 2026-05-01\n",
        "completed: 2026-05-15\n",
        "context: personal\n",
        "project: null\n",
        "stewardship: null\n",
    ));

    let parsed = CommitmentFrontmatter::try_from(fm).expect("valid completed commitment");

    assert_eq!(parsed.status, CommitmentStatus::Completed);
    assert_eq!(
        parsed.completed,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap())
    );
}

#[test]
fn commitment_try_from_rejects_missing_required_field() {
    // No `due:` — should error.
    let fm = parse_fm(concat!(
        "type: commitment\n",
        "status: active\n",
        "created: 2026-05-01\n",
        "context: personal\n",
    ));

    let err = CommitmentFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "due"),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------
// ActionStatus — kebab-case YAML round-trip + as_str + FromStr
// ---------------------------------------------------------------------

#[test]
fn action_status_serialises_as_kebab_case() {
    assert_eq!(
        serde_yaml::to_string(&ActionStatus::Active).unwrap().trim(),
        "active"
    );
    assert_eq!(
        serde_yaml::to_string(&ActionStatus::Completed)
            .unwrap()
            .trim(),
        "completed"
    );
    assert_eq!(
        serde_yaml::to_string(&ActionStatus::Blocked)
            .unwrap()
            .trim(),
        "blocked"
    );
}

#[test]
fn action_status_deserialises_from_kebab_case() {
    assert_eq!(
        serde_yaml::from_str::<ActionStatus>("active").unwrap(),
        ActionStatus::Active
    );
    assert_eq!(
        serde_yaml::from_str::<ActionStatus>("completed").unwrap(),
        ActionStatus::Completed
    );
    assert_eq!(
        serde_yaml::from_str::<ActionStatus>("blocked").unwrap(),
        ActionStatus::Blocked
    );
}

#[test]
fn action_status_as_str_returns_kebab_case() {
    let cases = [
        (ActionStatus::Active, "active"),
        (ActionStatus::Completed, "completed"),
        (ActionStatus::Blocked, "blocked"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.as_str(), expected, "variant={variant:?}");
    }
}

#[test]
fn action_status_from_str_parses_every_variant() {
    for variant in ActionStatus::ALL {
        let parsed: ActionStatus = variant.as_str().parse().expect("round-trip");
        assert_eq!(parsed, variant);
    }
}

#[test]
fn action_status_from_str_rejects_unknown_value() {
    let err = "archived"
        .parse::<ActionStatus>()
        .expect_err("unknown status must reject");
    assert!(format!("{err}").contains("archived"));
}

// ---------------------------------------------------------------------
// ActionFrontmatter::try_from
// ---------------------------------------------------------------------

#[test]
fn action_try_from_parses_active_milestone_pinned() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: surrogate-model\n",
        "energy: deep\n",
        "milestone: \"[[projects/surrogate-model#full-geometry]]\"\n",
        "due: null\n",
        "created: 2026-04-15\n",
        "completed: null\n",
        "blocker: null\n",
        "criteria: |\n",
        "  Sample efficiency curve generated for KAN-PPO across 5 seeds.\n",
        "tags: [kan, ppo]\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("valid milestone-pinned action");

    assert_eq!(parsed.status, ActionStatus::Active);
    assert_eq!(parsed.project, "surrogate-model");
    assert_eq!(parsed.energy, EnergyLevel::Deep);
    assert_eq!(
        parsed.milestone.as_deref(),
        Some("[[projects/surrogate-model#full-geometry]]")
    );
    assert!(parsed.due.is_none());
    assert_eq!(
        parsed.created,
        chrono::NaiveDate::from_ymd_opt(2026, 4, 15).unwrap()
    );
    assert!(parsed.completed.is_none());
    assert!(parsed.blocker.is_none());
    assert!(
        parsed
            .criteria
            .as_deref()
            .unwrap_or("")
            .contains("Sample efficiency curve")
    );
    assert_eq!(parsed.tags, vec!["kan".to_owned(), "ppo".to_owned()]);
}

#[test]
fn action_try_from_parses_standalone_with_due() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: icml-paper\n",
        "energy: medium\n",
        "milestone: null\n",
        "due: 2026-05-22\n",
        "created: 2026-05-01\n",
        "completed: null\n",
        "blocker: null\n",
        "criteria: null\n",
        "tags: []\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("valid standalone-due action");

    assert!(parsed.milestone.is_none());
    assert_eq!(
        parsed.due,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 22).unwrap())
    );
    assert!(parsed.tags.is_empty());
}

#[test]
fn action_try_from_parses_completed_state() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: completed\n",
        "project: surrogate-model\n",
        "energy: deep\n",
        "milestone: null\n",
        "due: null\n",
        "created: 2026-04-15\n",
        "completed: 2026-04-30\n",
        "blocker: null\n",
        "criteria: null\n",
        "tags: []\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("valid completed action");

    assert_eq!(parsed.status, ActionStatus::Completed);
    assert_eq!(
        parsed.completed,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 30).unwrap())
    );
}

#[test]
fn action_try_from_parses_blocked_state_with_blocker() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: blocked\n",
        "project: icml-paper\n",
        "energy: light\n",
        "milestone: null\n",
        "due: null\n",
        "created: 2026-05-01\n",
        "completed: null\n",
        "blocker: \"waiting on supervisor draft feedback\"\n",
        "criteria: null\n",
        "tags: []\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("valid blocked action");

    assert_eq!(parsed.status, ActionStatus::Blocked);
    assert_eq!(
        parsed.blocker.as_deref(),
        Some("waiting on supervisor draft feedback")
    );
}

#[test]
fn action_try_from_accepts_milestone_and_due_together() {
    // Both optional fields present — schema allows it. Aggregation
    // dedup is the query's job, not the frontmatter's.
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: icml-paper\n",
        "energy: deep\n",
        "milestone: \"[[projects/icml-paper#submission]]\"\n",
        "due: 2026-05-15\n",
        "created: 2026-05-01\n",
        "completed: null\n",
        "blocker: null\n",
        "criteria: null\n",
        "tags: []\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("milestone + due together is valid");
    assert!(parsed.milestone.is_some());
    assert!(parsed.due.is_some());
}

#[test]
fn action_try_from_defaults_missing_tags_to_empty() {
    // `tags:` field omitted entirely — should default to [].
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: surrogate-model\n",
        "energy: deep\n",
        "created: 2026-04-15\n",
    ));

    let parsed = ActionFrontmatter::try_from(fm).expect("missing tags is fine");
    assert!(parsed.tags.is_empty());
}

#[test]
fn action_try_from_rejects_missing_status() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "project: surrogate-model\n",
        "energy: deep\n",
        "created: 2026-04-15\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "status"),
        "got {err:?}"
    );
}

#[test]
fn action_try_from_rejects_missing_project() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "energy: deep\n",
        "created: 2026-04-15\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "project"),
        "got {err:?}"
    );
}

#[test]
fn action_try_from_rejects_missing_energy() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: surrogate-model\n",
        "created: 2026-04-15\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "energy"),
        "got {err:?}"
    );
}

#[test]
fn action_try_from_rejects_missing_created() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: surrogate-model\n",
        "energy: deep\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingField { ref field } if field == "created"),
        "got {err:?}"
    );
}

#[test]
fn action_try_from_rejects_invalid_status_value() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: archived\n",
        "project: surrogate-model\n",
        "energy: deep\n",
        "created: 2026-04-15\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidField { ref field, .. } if field == "status"),
        "got {err:?}"
    );
}

#[test]
fn action_try_from_rejects_invalid_energy_value() {
    let fm = parse_fm(concat!(
        "type: action\n",
        "status: active\n",
        "project: surrogate-model\n",
        "energy: intense\n",
        "created: 2026-04-15\n",
    ));

    let err = ActionFrontmatter::try_from(fm).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidField { ref field, .. } if field == "energy"),
        "got {err:?}"
    );
}
