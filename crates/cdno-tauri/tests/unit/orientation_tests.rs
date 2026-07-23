//! `get_orientation_impl` against the Memory doubles — the command
//! seam, no Tauri runtime involved.

use std::sync::Arc;

use cdno_core::config::{VaultConfig, VaultMeta};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::commands::orientation::get_orientation_impl;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    vault_with_config(notes, VaultConfig::default())
}

fn vault_with_config(notes: &[(&str, &str)], config: VaultConfig) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, config).expect("Vault::new");
    vault
}

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nCore loop underway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] File receipts (light)\n\n## Milestones\n- [ ] Submit abstract — hard: 2026-05-27\n";

const HEALTH: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Active Habits\n- Swimming 1x/week \u{2014} lapsed since March\n";

#[test]
fn orientation_view_composes_context_actions_and_lapses() {
    let vault = vault_with(&[
        ("projects/alpha.md", ALPHA),
        ("stewardships/health.md", HEALTH),
    ]);

    let view = get_orientation_impl(&vault, ymd(2026, 5, 26)).unwrap();

    assert_eq!(view.today, ymd(2026, 5, 26));
    // The hard milestone one day out shows in the strip.
    assert_eq!(view.commitments.len(), 1);
    assert_eq!(view.commitments[0].title, "Submit abstract");

    // One project, carrying its life context and EVERY open bullet —
    // the energy selector filters client-side.
    assert_eq!(view.projects.len(), 1);
    let project = &view.projects[0];
    assert_eq!(project.summary.slug, "alpha");
    assert_eq!(project.summary.context, cdno_domain::Context::Work);
    assert_eq!(project.actions.len(), 2);

    assert_eq!(view.lapsed_habits.len(), 1);
    assert_eq!(view.lapsed_habits[0].stewardship, "health");
}

/// The sidebar states "n of N slots" on every page, and N is the vault's
/// own cap — a vault that lowered it must not be told it has five (#444).
#[test]
fn max_active_reflects_the_configured_cap() {
    let config = VaultConfig {
        vault: VaultMeta {
            name: "test-vault".to_owned(),
            max_active_projects: 3,
            ..VaultMeta::default()
        },
        ..VaultConfig::default()
    };
    let vault = vault_with_config(&[("projects/alpha.md", ALPHA)], config);
    let view = get_orientation_impl(&vault, ymd(2026, 5, 26)).unwrap();
    assert_eq!(view.max_active, 3);
}

#[test]
fn orientation_view_empty_vault_is_calmly_empty() {
    let vault = vault_with(&[]);
    let view = get_orientation_impl(&vault, ymd(2026, 5, 26)).unwrap();
    assert!(view.commitments.is_empty());
    assert!(view.projects.is_empty());
    assert!(view.lapsed_habits.is_empty());
}

// ---------------------------------------------------------------------
// get_now (#442) — the Today page's Now band. Reconstructed from the
// day's log, so it needs no state of its own and sees a start made from
// anywhere: the app, the CLI, or an agent over MCP.
// ---------------------------------------------------------------------

use cdno_tauri::commands::orientation::get_now_impl;

const PROJECT: &str = "---\ntype: project\ncontext: university\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Current State\nGoing.\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n";

fn daily(lines: &[&str]) -> String {
    let body = lines
        .iter()
        .map(|l| format!("- {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("---\ndate: 2026-07-13\ntype: daily\n---\n\n# 2026-07-13\n\n## Logs\n{body}\n")
}

#[test]
fn get_now_is_none_when_nothing_was_started() {
    let vault = vault_with(&[("projects/alpha.md", PROJECT)]);

    let now = get_now_impl(&vault, ymd(2026, 7, 13)).unwrap();

    assert!(now.is_none());
}

#[test]
fn get_now_reports_the_open_action_with_its_project_context() {
    let log = daily(&["**09:30**: started [[alpha]] \u{2014} Draft the methods section (deep)"]);
    let vault = vault_with(&[
        ("projects/alpha.md", PROJECT),
        ("journal/2026/daily/2026-07-13.md", &log),
    ]);

    let now = get_now_impl(&vault, ymd(2026, 7, 13))
        .unwrap()
        .expect("a focus");

    assert_eq!(now.project, "alpha");
    assert_eq!(now.action, "Draft the methods section (deep)");
    assert_eq!(now.started, "09:30");
    assert_eq!(now.context, Some(cdno_domain::Context::University));
}

#[test]
fn get_now_still_reports_a_focus_whose_project_is_gone() {
    // The log is the record. A project renamed or archived since the line
    // was written must not make what you are on disappear — only its
    // colour dot is unknown.
    let log = daily(&["**09:30**: started [[vanished]] \u{2014} Something in flight"]);
    let vault = vault_with(&[("journal/2026/daily/2026-07-13.md", &log)]);

    let now = get_now_impl(&vault, ymd(2026, 7, 13))
        .unwrap()
        .expect("a focus");

    assert_eq!(now.project, "vanished");
    assert_eq!(now.context, None);
}
