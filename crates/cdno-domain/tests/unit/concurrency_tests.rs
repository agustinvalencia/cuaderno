//! Concurrency regression test for the vault write lock (#196).
//!
//! Multiple processes (here: threads, each with its own `FsVaultStore`
//! fd + `SqliteIndex` connection over one vault dir) hammering the same
//! daily note must not lose log lines. `log_to_daily_note` is a
//! read-modify-write full rewrite; without the cross-process lock,
//! concurrent writers clobber each other. With it, every line survives.

use std::path::Path;
use std::sync::Arc;
use std::thread;

use cdno_core::config::VaultConfig;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tempfile::tempdir;

/// Build a vault backed by the real filesystem + SQLite at `root`. Each
/// call is an independent `FsVaultStore`/`SqliteIndex` pair, so several
/// of them over one dir model several processes.
fn build_fs_vault(root: &Path) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> =
        Arc::new(SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index"));
    let (vault, _report) =
        Vault::new(store, index, VaultConfig::default()).expect("construct vault");
    vault
}

#[test]
fn concurrent_log_appends_do_not_lose_lines() {
    const WRITERS: usize = 8;
    const PER_WRITER: usize = 5;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();

    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());

    // Build each writer's vault up front so only the logging races, not
    // vault construction (which reconciles).
    let vaults: Vec<Vault> = (0..WRITERS).map(|_| build_fs_vault(root)).collect();

    let handles: Vec<_> = vaults
        .into_iter()
        .enumerate()
        .map(|(w, vault)| {
            thread::spawn(move || {
                for n in 0..PER_WRITER {
                    vault
                        .log_to_daily_note(at, &format!("writer {w} line {n}"))
                        .expect("log_to_daily_note");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // Every line must be present — none clobbered by a concurrent rewrite.
    let daily =
        std::fs::read_to_string(root.join("journal/2026/daily/2026-06-13.md")).expect("daily note");
    let got = daily.matches("writer ").count();
    assert_eq!(
        got,
        WRITERS * PER_WRITER,
        "lost log lines under concurrent writers (got {got}, want {}):\n{daily}",
        WRITERS * PER_WRITER
    );
}

#[test]
fn concurrent_add_action_keeps_every_bullet() {
    // A project-body read-modify-write (read the project map, append a
    // bullet to `## Next Actions`, rewrite it) — the surface the 16+
    // reorders protect. Without the lock covering the read, concurrent
    // `add_action`s clobber each other's bullets.
    const WRITERS: usize = 8;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());

    // One project to hammer.
    build_fs_vault(root)
        .create_project(at.date(), "Concurrency", Context::Work, None)
        .expect("create project");

    let vaults: Vec<Vault> = (0..WRITERS).map(|_| build_fs_vault(root)).collect();
    let handles: Vec<_> = vaults
        .into_iter()
        .enumerate()
        .map(|(w, vault)| {
            thread::spawn(move || {
                vault
                    .add_action(
                        at,
                        "concurrency",
                        &format!("action {w}"),
                        EnergyLevel::Medium,
                    )
                    .expect("add_action");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let project =
        std::fs::read_to_string(root.join("projects/concurrency.md")).expect("project map");
    let got = (0..WRITERS)
        .filter(|w| project.contains(&format!("action {w}")))
        .count();
    assert_eq!(
        got, WRITERS,
        "lost action bullets under concurrent add_action (got {got}, want {WRITERS}):\n{project}"
    );
}
