//! `get_commitments_impl` against the Memory doubles — the command
//! seam, no Tauri runtime involved.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::CommitmentSource;
use cdno_tauri::commands::commitments::get_commitments_impl;
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Milestones\n- [ ] Submit paper \u{2014} hard: 2026-06-01\n\n## Next Actions\n";
const COMMITMENT: &str = "---\ntype: commitment\nstatus: active\ndue: 2026-05-30\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\n---\n\n# Renew passport\n";

#[test]
fn view_stamps_today_and_carries_both_sources_sorted() {
    let vault = vault_with(&[
        ("projects/alpha.md", PROJECT),
        ("commitments/renew-passport.md", COMMITMENT),
    ]);

    let view = get_commitments_impl(&vault, ymd(2026, 5, 26), 90).unwrap();

    // `today` is stamped for the frontend, not computed there.
    assert_eq!(view.today, ymd(2026, 5, 26));
    assert_eq!(view.entries.len(), 2);
    // Sorted chronologically: the standalone (05-30) precedes the
    // milestone (06-01).
    assert_eq!(view.entries[0].title, "Renew passport");
    assert_eq!(
        view.entries[0].source,
        CommitmentSource::StandaloneCommitment("renew-passport".to_owned()),
    );
    assert_eq!(view.entries[1].title, "Submit paper");
    assert_eq!(
        view.entries[1].source,
        CommitmentSource::ProjectMilestone("alpha".to_owned()),
    );
}

#[test]
fn empty_window_is_calmly_empty() {
    let vault = vault_with(&[]);
    let view = get_commitments_impl(&vault, ymd(2026, 5, 26), 90).unwrap();
    assert!(view.entries.is_empty());
}
