//! Unit tests for `Vault::monthly_context` — the composed monthly-review
//! snapshot (RFC 0006 §8.0). `MemoryVaultStore` / `MemoryIndex` keep it
//! fast.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, QuestionDomain};
use chrono::{Datelike, NaiveDate, NaiveDateTime};

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn at(date: NaiveDate) -> NaiveDateTime {
    date.and_hms_opt(9, 0, 0).unwrap()
}

#[test]
fn monthly_context_composes_every_field_from_its_source() {
    // A date well after the run: every map written now is older than
    // the 14-day stuck threshold, so stuck-detection has something to
    // find (the in-memory store stamps the real clock).
    // Four years ahead of the real clock, so the gap never closes.
    let y = chrono::Local::now().date_naive().year() + 4;
    let today = ymd(y, 6, 12);
    let since = ymd(y, 5, 13);
    let long_ago = ymd(y, 1, 10);

    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in [
        (
            "actions/_done/2030/win.md",
            "---\ntype: action\nstatus: completed\nproject: alpha\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2030-05-01\ncompleted: 2030-05-20\nblocker: null\ncriteria: null\ntags: []\n---\n\n# Win\n",
        ),
        (
            "projects/_done/2030/old.md",
            "---\ntype: project\ncontext: work\nstatus: dropped\ncreated: 2030-01-01\nclosed: 2030-05-25\n---\n\n# Old\n",
        ),
    ] {
        let path = path.replace("2030", &y.to_string());
        let body = body.replace("2030", &y.to_string());
        store
            .write_file(&VaultPath::new(&path).unwrap(), &body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");

    vault
        .create_project(at(long_ago), "Alpha", Context::Work, None)
        .unwrap();
    vault
        .create_project(at(long_ago), "Beta", Context::Work, None)
        .unwrap();
    vault.park_project(at(long_ago), "beta").unwrap();
    vault
        .create_question(
            at(long_ago),
            QuestionDomain::Research,
            "What truly matters?",
        )
        .unwrap();
    vault
        .create_portfolio(at(long_ago), "Does X beat Y?", None)
        .unwrap();
    vault
        .create_stewardship_flat(at(long_ago), "Health", Context::Personal)
        .unwrap();
    vault
        .create_commitment(
            at(long_ago),
            "Renew the licence",
            ymd(y, 7, 10),
            Context::Personal,
            None,
            None,
        )
        .unwrap();

    let ctx = vault.monthly_context(today).unwrap();

    assert_eq!(ctx.since, since);

    assert_eq!(
        ctx.completed_actions,
        vault.completed_actions_between(since, today).unwrap()
    );
    assert_eq!(ctx.completed_actions.len(), 1);

    assert_eq!(
        ctx.closed_projects,
        vault.closed_projects_between(since, today).unwrap()
    );
    assert_eq!(ctx.closed_projects.len(), 1);
    assert_eq!(ctx.closed_projects[0].slug, "old");

    let parked: Vec<_> = ctx
        .parked_projects
        .iter()
        .map(|p| (p.path.clone(), p.frontmatter.clone()))
        .collect();
    assert_eq!(parked, vault.parked_projects().unwrap());
    assert_eq!(ctx.parked_projects.len(), 1);

    assert_eq!(ctx.active_questions, vault.active_questions().unwrap());
    assert_eq!(ctx.active_questions.len(), 1);

    assert_eq!(ctx.portfolios, vault.list_portfolios(today).unwrap());
    assert_eq!(ctx.portfolios.len(), 1);

    assert_eq!(ctx.stuck_projects, vault.stuck_projects(today, 14).unwrap());
    assert_eq!(ctx.stuck_projects.len(), 1);
    assert_eq!(ctx.stuck_projects[0].slug, "alpha");

    assert_eq!(ctx.stewardships, vault.list_stewardships(today).unwrap());
    assert_eq!(ctx.stewardships.len(), 1);

    assert_eq!(ctx.commitments, vault.commitments(today, 42).unwrap());
    assert_eq!(ctx.commitments.len(), 1);
    assert_eq!(ctx.commitments[0].title, "Renew the licence");

    assert_eq!(ctx.slots, vault.slot_usage().unwrap());
    assert_eq!(ctx.slots.active, 1);
}

#[test]
fn parked_and_stuck_entries_carry_holder_counts_and_waiting_on_empty() {
    // The monthly review reads parked maps by title and tells "someday"
    // from "someone has it" (RFC 0006 §8.5); the stuck scan keeps held
    // maps, carrying the same fields.
    let map = |status: &str, holder: Option<&str>, title: &str, waiting: &str| {
        let held = holder
            .map(|h| format!("held_by: \"{h}\"\n"))
            .unwrap_or_default();
        format!(
            "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-01-01\n{held}---\n\n\
             # {title}\n\n## Current State\nN/A.\n\n## Next Actions\n\n## Waiting On\n{waiting}"
        )
    };
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in [
        (
            "projects/_parked/lighthouse.md",
            map(
                "parked",
                Some("the lighthouse trust"),
                "Lighthouse",
                "- Survey report from the trust\n",
            ),
        ),
        (
            "projects/_parked/orchard.md",
            map("parked", None, "Orchard", "(nothing yet)\n"),
        ),
        (
            "projects/ferry.md",
            map("active", Some("the ferry office"), "Ferry", ""),
        ),
        (
            "projects/mill.md",
            map("active", None, "Mill", "- Quote from the joiner\n"),
        ),
    ] {
        store
            .write_file(&VaultPath::new(path).unwrap(), &body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    // Well after the run, so every map written now is past the 14-day
    // stuck threshold.
    let today = chrono::Local::now().date_naive() + chrono::Duration::days(60);

    let ctx = vault.monthly_context(today).unwrap();

    let mut parked: Vec<_> = ctx.parked_projects.iter().collect();
    parked.sort_by_key(|p| p.path.as_path().to_path_buf());
    assert_eq!(parked.len(), 2);
    let lighthouse = parked[0];
    assert!(lighthouse.path.as_path().ends_with("lighthouse.md"));
    assert_eq!(lighthouse.held_by.as_deref(), Some("the lighthouse trust"));
    assert!(!lighthouse.counts, "a parked map holds no slot");
    assert!(!lighthouse.waiting_on_empty);
    let orchard = parked[1];
    assert_eq!(orchard.held_by, None);
    assert!(!orchard.counts);
    assert!(orchard.waiting_on_empty, "the placeholder says nothing");

    let stuck: Vec<&str> = ctx.stuck_projects.iter().map(|p| p.slug.as_str()).collect();
    assert_eq!(stuck, vec!["ferry", "mill"]);
    let ferry = &ctx.stuck_projects[0];
    assert_eq!(ferry.held_by.as_deref(), Some("the ferry office"));
    assert!(!ferry.counts);
    assert!(ferry.waiting_on_empty, "a name over an empty Waiting On");
    let mill = &ctx.stuck_projects[1];
    assert_eq!(mill.held_by, None);
    assert!(mill.counts);
    assert!(!mill.waiting_on_empty);
}
