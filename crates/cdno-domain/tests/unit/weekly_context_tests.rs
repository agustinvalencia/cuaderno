//! Unit tests for `Vault::weekly_context` — the composed weekly-review
//! snapshot (RFC 0006 §8.0). `MemoryVaultStore` / `MemoryIndex` keep it
//! fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use chrono::NaiveDate;

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(String, String)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(path).unwrap(), body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

/// A daily note for `date` whose `## Logs` section carries `lines`.
fn daily(date: NaiveDate, lines: &str) -> (String, String) {
    (
        cdno_core::paths::daily_note_relpath(date),
        format!(
            "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{lines}",
            date = date.format("%Y-%m-%d"),
        ),
    )
}

/// `count` plain checkpoint lines.
fn checkpoints(count: usize) -> String {
    (0..count)
        .map(|i| format!("- **10:{:02}**: checkpoint {i}\n", i % 60))
        .collect()
}

#[test]
fn weekly_context_composes_every_field_from_its_source() {
    // 2030-06-12 is a Wednesday: the week runs Mon 06-10 to Sun 06-16.
    let today = ymd(2030, 6, 12);
    let monday = ymd(2030, 6, 10);
    let sunday = ymd(2030, 6, 16);

    // 120 log lines across the week, more than the 100 an interface may
    // keep: 59 checkpoints and a state change on Monday, 59 checkpoints
    // and a bullet completion today. The composition must not bound them.
    let monday_logs = format!(
        "- **08:00**: state on [[alpha]]\n  was: drafting\n  now: the parser is in\n{}",
        checkpoints(59)
    );
    let today_logs = format!(
        "{}- **17:00**: action done on [[alpha]] \u{2014} Write the boundary tests (deep)\n",
        checkpoints(59)
    );
    let vault = vault_with(&[
        daily(monday, &monday_logs),
        daily(today, &today_logs),
        // Outside the week: its line must not be counted.
        daily(ymd(2030, 6, 9), "- **09:00**: last Sunday\n"),
        (
            "actions/_done/2030/win.md".to_owned(),
            "---\ntype: action\nstatus: completed\nproject: alpha\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2030-06-01\ncompleted: 2030-06-11\nblocker: null\ncriteria: null\ntags: []\n---\n\n# Win\n".to_owned(),
        ),
        (
            "projects/_done/2030/shipped.md".to_owned(),
            "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2030-01-01\nclosed: 2030-06-11\n---\n\n# Shipped\n".to_owned(),
        ),
        (
            "commitments/send-the-draft.md".to_owned(),
            "---\ntype: commitment\nstatus: active\ndue: 2030-06-20\ncreated: 2030-06-01\ncompleted: null\ncontext: work\n---\n\n# Send the draft\n".to_owned(),
        ),
    ]);

    let ctx = vault.weekly_context(today).unwrap();

    assert_eq!(ctx.week_of, monday);

    assert_eq!(ctx.logs, vault.weekly_logs(today).unwrap());
    assert_eq!(ctx.logs.len(), 120, "every line of the week, unbounded");

    let completed = vault.completed_actions_between(monday, sunday).unwrap();
    assert_eq!(ctx.completed_actions, completed);
    assert_eq!(
        ctx.completed_actions.len(),
        2,
        "note and bullet: {completed:?}"
    );

    assert_eq!(
        ctx.closed_projects,
        vault.closed_projects_between(monday, sunday).unwrap()
    );
    assert_eq!(ctx.closed_projects.len(), 1);
    assert_eq!(ctx.closed_projects[0].slug, "shipped");

    assert_eq!(
        ctx.state_changes,
        vault.project_state_changes_between(monday, sunday).unwrap()
    );
    assert_eq!(ctx.state_changes.len(), 1);
    assert_eq!(ctx.state_changes[0].old_state, "drafting", "full bodies");

    assert_eq!(ctx.commitments, vault.commitments(today, 14).unwrap());
    assert_eq!(ctx.commitments.len(), 1);
    assert_eq!(ctx.commitments[0].title, "Send the draft");
}
