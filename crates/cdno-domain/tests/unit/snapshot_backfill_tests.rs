//! The startup backfill of the masked snapshot hash (RFC 0007 T16): rows
//! archived before index migration 004 carry no masked hash, and
//! `Vault::new` stamps one on each whose plain hash still verifies.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{ArchivalSnapshot, MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::EnergyLevel;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(day: u32, hour: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 5, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
}

fn open(store: &Arc<dyn VaultStore>, index: &Arc<dyn VaultIndex>) -> Vault {
    Vault::new(Arc::clone(store), Arc::clone(index), VaultConfig::default())
        .expect("Vault::new succeeded")
        .0
}

/// What [`two_archived_actions`] hands back.
type Archived = (
    Arc<dyn VaultStore>,
    Arc<dyn VaultIndex>,
    [(VaultPath, ArchivalSnapshot); 2],
);

/// Two actions on `surrogate-model`, archived through the verb, so each
/// snapshot row is exactly what archival stamps today. Returns the store,
/// the index and the two archived paths with their stamped rows.
fn two_archived_actions() -> Archived {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store
        .write_file(&vp("projects/surrogate-model.md"), PROJECT)
        .unwrap();
    let vault = open(&store, &index);
    for (title, slug) in [("Characterise", "characterise"), ("Calibrate", "calibrate")] {
        vault
            .add_action_with_note(dt(1, 9), "surrogate-model", title, EnergyLevel::Deep)
            .expect("add action with note");
        vault
            .complete_action(dt(2, 9), "surrogate-model", slug)
            .expect("complete action");
    }
    let rows = ["characterise", "calibrate"].map(|slug| {
        let path = vp(&format!("actions/_done/2026/{slug}.md"));
        let snap = index
            .find_archival_snapshot(&path)
            .unwrap()
            .expect("archival stamps a snapshot");
        assert!(
            snap.masked_hash.is_some(),
            "archival stamps the masked hash"
        );
        (path, snap)
    });
    (store, index, rows)
}

/// Clear the masked fields, as a row archived before migration 004 holds.
fn unmask(index: &Arc<dyn VaultIndex>, path: &VaultPath, snap: &ArchivalSnapshot) {
    let pre_004 = ArchivalSnapshot {
        masked_hash: None,
        mask_version: None,
        masked_size: None,
        ..snap.clone()
    };
    index.record_archival_snapshot(path, &pre_004).unwrap();
}

fn row(index: &Arc<dyn VaultIndex>, path: &VaultPath) -> ArchivalSnapshot {
    index.find_archival_snapshot(path).unwrap().expect("row")
}

#[test]
fn backfill_fills_only_rows_that_verify() {
    let (store, index, [(intact, stamped), (tampered, tampered_snap)]) = two_archived_actions();
    unmask(&index, &intact, &stamped);
    unmask(&index, &tampered, &tampered_snap);

    // A same-length prose edit inside the tampered note's frozen prefix:
    // its plain hash no longer verifies.
    let raw = store.read_file(&tampered).unwrap();
    let edited = raw.replace("# Calibrate", "# Calibrats");
    assert_ne!(raw, edited);
    store.write_file(&tampered, &edited).unwrap();

    let _vault = open(&store, &index);

    // The intact row gets exactly what archival stamped, so a backfilled
    // row and a freshly archived one verify alike.
    assert_eq!(row(&index, &intact), stamped);
    // The tampered row is left NULL: masking an edited prefix would bless
    // the edit.
    let left = row(&index, &tampered);
    assert_eq!(left.masked_hash, None);
    assert_eq!(left.mask_version, None);
    assert_eq!(left.masked_size, None);
    assert_eq!(left.frozen_hash, tampered_snap.frozen_hash);
}

#[test]
fn backfill_is_idempotent() {
    let (store, index, [(first, first_snap), (second, second_snap)]) = two_archived_actions();
    unmask(&index, &first, &first_snap);

    let _vault = open(&store, &index);
    let after_one = row(&index, &first);
    assert_eq!(after_one, first_snap);

    // A later open finds nothing to do and changes nothing.
    let _vault = open(&store, &index);
    assert_eq!(row(&index, &first), after_one);

    // A row that already carries a masked hash is never re-stamped, even
    // when this binary would compute something else (a row a newer
    // binary stamped at a newer mask version, say), and even beside a row
    // that does need filling.
    let foreign = ArchivalSnapshot {
        masked_hash: Some("ffffffffffffffff".to_owned()),
        mask_version: Some(99),
        masked_size: Some(1),
        ..second_snap.clone()
    };
    index.record_archival_snapshot(&second, &foreign).unwrap();
    unmask(&index, &first, &first_snap);
    let _vault = open(&store, &index);
    assert_eq!(row(&index, &second), foreign);
    assert_eq!(row(&index, &first), first_snap);
}
