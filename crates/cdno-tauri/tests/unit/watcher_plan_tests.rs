//! The batch decision table (`plan_batch`) and the #372 config-error
//! classifier (`is_invalid_config_error`) — both pure, no AppHandle.

use std::time::{Duration, Instant};

use cdno_core::error::{ConfigError, IndexError, StoreError, ValidationError};
use cdno_core::path::VaultPath;
use cdno_core::watcher::FileEvent;
use cdno_domain::error::DomainError;
use cdno_tauri::events::VaultArea;
use cdno_tauri::state::{ECHO_WINDOW, WriteJournal};
use cdno_tauri::watcher::{
    BatchPlan, RebuildAttempt, classify_rebuild, is_invalid_config_error, plan_batch,
};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

#[test]
fn external_edits_classify_sort_and_dedup() {
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("stewardships/health.md")),
            FileEvent::Changed(vp("projects/alpha.md")),
            FileEvent::Removed(vp("projects/beta.md")),
        ],
    );
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Projects, VaultArea::Stewardships],
            paths: vec![
                "stewardships/health.md".into(),
                "projects/alpha.md".into(),
                "projects/beta.md".into(),
            ],
            config_changed: false,
        }
    );
}

#[test]
fn rescan_wins_over_everything_in_the_batch() {
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("projects/alpha.md")),
            FileEvent::Rescan,
        ],
    );
    assert_eq!(plan, BatchPlan::Rescan);
}

#[test]
fn self_echoes_and_noise_are_quiet() {
    let journal = WriteJournal::default();
    journal.record([vp("projects/alpha.md")]);
    let plan = plan_batch(
        &journal,
        vec![
            // Our own write echoing back.
            FileEvent::Changed(vp("projects/alpha.md")),
            // Non-note noise: attachments, index db, temp staging.
            FileEvent::Changed(vp("assets/photo.png")),
            FileEvent::Changed(vp(".cuaderno/index.db")),
        ],
    );
    assert_eq!(plan, BatchPlan::Quiet);
}

#[test]
fn mixed_batch_emits_only_the_external_subset() {
    let journal = WriteJournal::default();
    journal.record([vp("journal/2026/daily/2026-07-07.md")]);
    let plan = plan_batch(
        &journal,
        vec![
            FileEvent::Changed(vp("journal/2026/daily/2026-07-07.md")),
            FileEvent::Changed(vp("projects/alpha.md")),
        ],
    );
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Projects],
            paths: vec!["projects/alpha.md".into()],
            config_changed: false,
        }
    );
}

#[test]
fn external_config_edit_sets_config_changed() {
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![FileEvent::Changed(vp(".cuaderno/config.toml"))],
    );
    match plan {
        BatchPlan::External {
            areas,
            config_changed,
            ..
        } => {
            assert!(config_changed, "a config.toml edit must trigger a rebuild");
            assert_eq!(areas, vec![VaultArea::Config]);
        }
        other => panic!("expected External, got {other:?}"),
    }
}

#[test]
fn note_only_batch_leaves_config_changed_false() {
    let journal = WriteJournal::default();
    let plan = plan_batch(&journal, vec![FileEvent::Changed(vp("projects/alpha.md"))]);
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Projects],
            paths: vec!["projects/alpha.md".into()],
            config_changed: false,
        }
    );
}

#[test]
fn template_edit_is_config_area_but_not_a_rebuild_trigger() {
    // A `.cuaderno/templates/*.md` edit classifies as Config (it changes the
    // log form's fields) but does NOT change the note-type registry, so it
    // must not trigger a live vault rebuild.
    let journal = WriteJournal::default();
    let plan = plan_batch(
        &journal,
        vec![FileEvent::Changed(vp(".cuaderno/templates/demo.md"))],
    );
    assert_eq!(
        plan,
        BatchPlan::External {
            areas: vec![VaultArea::Config],
            paths: vec![".cuaderno/templates/demo.md".into()],
            config_changed: false,
        }
    );
}

#[test]
fn self_write_config_edit_is_suppressed_to_quiet() {
    // A journalled (self-written) config edit is our own echo — the save
    // command already emitted precisely and drove the live reload — so the
    // batch is Quiet and config_changed never comes into play.
    let journal = WriteJournal::default();
    journal.record([vp(".cuaderno/config.toml")]);
    let plan = plan_batch(
        &journal,
        vec![FileEvent::Changed(vp(".cuaderno/config.toml"))],
    );
    assert_eq!(plan, BatchPlan::Quiet);
}

#[test]
fn journal_entries_expire_after_the_echo_window() {
    let journal = WriteJournal::default();
    let wrote_at = Instant::now();
    journal.record_at(wrote_at, [vp("projects/alpha.md")]);

    let just_inside = wrote_at + ECHO_WINDOW - Duration::from_millis(1);
    assert!(journal.is_recent_self_write_at(just_inside, &vp("projects/alpha.md")));

    let just_past = wrote_at + ECHO_WINDOW;
    assert!(
        !journal.is_recent_self_write_at(just_past, &vp("projects/alpha.md")),
        "an echo past the window must be treated as external (safer failure direction)"
    );
}

// -------------------------------------------------------------------
// is_invalid_config_error — the #372 classifier that keeps a transient
// reload failure (a held write lock, an IO/index hiccup) from being
// mislabelled as an invalid config in the banner, while still surfacing a
// genuinely invalid config.
// -------------------------------------------------------------------

#[test]
fn genuinely_invalid_config_errors_are_classified_invalid() {
    // A custom note type shadowing a built-in, and a schema field redeclaring
    // an engine-owned key, come straight from `TypeRegistry::validate` as
    // top-level DomainError variants — NOT Validation or Config — so the
    // classifier must recognise them or a real invalid config gets silently
    // swallowed as contention.
    assert!(is_invalid_config_error(&DomainError::ReservedTypeName {
        name: "Project".to_owned(),
    }));
    assert!(is_invalid_config_error(&DomainError::ReservedSchemaField {
        note_type: "daily".to_owned(),
        field: "date".to_owned(),
    }));
    // A bad ignore glob and other config-content problems are Config errors.
    assert!(is_invalid_config_error(&DomainError::Config(
        ConfigError::InvalidGlob("**[".to_owned()),
    )));
    assert!(is_invalid_config_error(&DomainError::Config(
        ConfigError::InvalidNoteType("empty folder".to_owned()),
    )));
    assert!(is_invalid_config_error(&DomainError::Config(
        ConfigError::InvalidSchema("bad default".to_owned()),
    )));
    // A Validation error, wherever it might arise, is content-invalid too.
    assert!(is_invalid_config_error(&DomainError::Validation(
        ValidationError::MissingField {
            field: "collaborators".to_owned(),
        },
    )));
}

#[test]
fn transient_reload_failures_are_not_classified_invalid() {
    // The write-lock timeout during reconcile is wrapped as IndexError::Update
    // — this is the exact shape #372 must not mislabel as "invalid config".
    assert!(!is_invalid_config_error(&DomainError::Index(
        IndexError::Update("acquiring write lock during reconcile: timed out".to_owned()),
    )));
    // A raw lock-timeout store error, wherever it surfaces, is also transient.
    assert!(!is_invalid_config_error(&DomainError::Store(
        StoreError::LockTimeout(std::time::Duration::from_secs(5)),
    )));
    // A read hiccup (e.g. the file caught mid-rename) is IO, not bad content.
    assert!(!is_invalid_config_error(&DomainError::Config(
        ConfigError::Read {
            path: std::path::PathBuf::from(".cuaderno/config.toml"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        },
    )));
}

// -------------------------------------------------------------------
// classify_rebuild / RebuildAttempt::needs_standalone_reconcile — the #371
// decision. Only an APPLIED rebuild reconciled the batch itself (via
// `Vault::new`, against the new ignore set); every FAILURE outcome keeps the
// old vault live, so the watcher must still run its standalone reconcile to
// fold the batch's note edits into the index. `classify_rebuild` splits a
// rebuild result three ways; `needs_standalone_reconcile` pins which of them
// still reconcile.
// -------------------------------------------------------------------

#[test]
fn classify_rebuild_maps_a_successful_rebuild_to_applied() {
    assert_eq!(classify_rebuild(&Ok(())), RebuildAttempt::Applied);
}

#[test]
fn classify_rebuild_maps_an_invalid_config_to_invalid() {
    // A bad ignore glob is a Config error — invalid content, not contention.
    let result = Err(DomainError::Config(ConfigError::InvalidGlob(
        "**[".to_owned(),
    )));
    assert_eq!(classify_rebuild(&result), RebuildAttempt::Invalid);
}

#[test]
fn classify_rebuild_maps_a_transient_failure_to_transient() {
    // A write-lock timeout during the rebuild's reconcile is wrapped as
    // IndexError::Update — a transient outcome worth one retry, not invalid.
    let result = Err(DomainError::Index(IndexError::Update(
        "acquiring write lock during reconcile: timed out".to_owned(),
    )));
    assert_eq!(classify_rebuild(&result), RebuildAttempt::Transient);
}

#[test]
fn only_an_applied_rebuild_skips_the_standalone_reconcile() {
    // The load-bearing #371 invariant: applied => no standalone reconcile (the
    // rebuild's own pass already folded the batch); every failure => standalone
    // reconcile (the old vault stayed live, note edits still need folding).
    assert!(!RebuildAttempt::Applied.needs_standalone_reconcile());
    assert!(RebuildAttempt::Invalid.needs_standalone_reconcile());
    assert!(RebuildAttempt::Transient.needs_standalone_reconcile());
}

// ---------------------------------------------------------------------
// The watcher's reconcile updates the #440 exclusion counts (not just the
// config-reload path). A bulk move of notes under a folder an existing
// glob already matches changes what is in the index with no config edit
// to trigger a rebuild — so if only rebuilds wrote the counts, the notice
// would stay silent through exactly the eviction it exists to report.
// ---------------------------------------------------------------------

use std::sync::Arc;

use arc_swap::ArcSwap;
use cdno_core::config::IgnoreSet;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_tauri::events::IndexExclusions;
use cdno_tauri::watcher::{WatcherDeps, run_reconcile};

const NOTE: &str = "---\ntype: zettel\ntitle: A note\n---\n# A note\n";

fn note_path(dir: &str, n: usize) -> VaultPath {
    VaultPath::new(format!("{dir}/note-{n:02}.md")).unwrap()
}

#[test]
fn a_watcher_reconcile_records_what_it_excluded() {
    // `archive/**` is already in the config and matches nothing at first;
    // then the notes move under it, out of band. Only a watcher pass runs.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for n in 0..12 {
        store.write_file(&note_path("notes", n), NOTE).unwrap();
    }
    let deps = WatcherDeps {
        store: store.clone(),
        index: index.clone(),
        ignore: Arc::new(ArcSwap::from_pointee(
            IgnoreSet::compile(&["archive/**".to_string()]).unwrap(),
        )),
        exclusions: Arc::new(ArcSwap::from_pointee(IndexExclusions::default())),
    };

    assert!(run_reconcile(&deps).ok);
    let before = **deps.exclusions.load();
    assert_eq!(before.ignored, 0);
    assert_eq!(before.indexed, 12);
    assert!(!before.ignore_looks_over_broad);

    // The out-of-band move the watcher exists to notice.
    for n in 0..12 {
        let raw = store.read_file(&note_path("notes", n)).unwrap();
        store.write_file(&note_path("archive", n), &raw).unwrap();
        store.delete_file(&note_path("notes", n)).unwrap();
    }

    let second = run_reconcile(&deps);
    assert!(second.ok);
    assert!(
        second.exclusions_changed,
        "a pass that evicts notes must report the change, so a quiet batch still emits"
    );
    let after = **deps.exclusions.load();
    assert_eq!(after.ignored, 12, "every moved note is now excluded");
    assert_eq!(after.indexed, 0);
    assert!(
        after.ignore_looks_over_broad,
        "an eviction this size must raise the notice, config edit or not"
    );
}

/// A store that swaps the ignore set the moment reconciliation starts
/// walking, modelling a config rebuild landing mid-pass.
///
/// This is the deterministic seam a review pointed out after I claimed the
/// guard could only be tested by racing threads: `reconcile` calls
/// `walk_dir` first, so a decorator here lands strictly between the
/// matcher's load at entry and the check at exit — no threads, no timing.
struct SwapOnWalk {
    inner: Arc<dyn VaultStore>,
    ignore: Arc<ArcSwap<IgnoreSet>>,
    swap_to: IgnoreSet,
}

impl VaultStore for SwapOnWalk {
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, cdno_core::error::StoreError> {
        self.ignore.store(Arc::new(self.swap_to.clone()));
        self.inner.walk_dir(path)
    }
    fn read_file(&self, p: &VaultPath) -> Result<String, cdno_core::error::StoreError> {
        self.inner.read_file(p)
    }
    fn read_bytes(&self, p: &VaultPath) -> Result<Vec<u8>, cdno_core::error::StoreError> {
        self.inner.read_bytes(p)
    }
    fn write_file(&self, p: &VaultPath, c: &str) -> Result<(), cdno_core::error::StoreError> {
        self.inner.write_file(p, c)
    }
    fn append_to_file(&self, p: &VaultPath, c: &str) -> Result<(), cdno_core::error::StoreError> {
        self.inner.append_to_file(p, c)
    }
    fn move_file(&self, s: &VaultPath, d: &VaultPath) -> Result<(), cdno_core::error::StoreError> {
        self.inner.move_file(s, d)
    }
    fn delete_file(&self, p: &VaultPath) -> Result<(), cdno_core::error::StoreError> {
        self.inner.delete_file(p)
    }
    fn exists(&self, p: &VaultPath) -> Result<bool, cdno_core::error::StoreError> {
        self.inner.exists(p)
    }
    fn list_dir(&self, p: &VaultPath) -> Result<Vec<VaultPath>, cdno_core::error::StoreError> {
        self.inner.list_dir(p)
    }
    fn metadata(
        &self,
        p: &VaultPath,
    ) -> Result<cdno_core::file_meta::FileMeta, cdno_core::error::StoreError> {
        self.inner.metadata(p)
    }
    fn import_external(
        &self,
        s: &std::path::Path,
        d: &VaultPath,
    ) -> Result<(), cdno_core::error::StoreError> {
        self.inner.import_external(s, d)
    }
}

#[test]
fn a_pass_whose_globs_were_swapped_underneath_it_does_not_publish() {
    // A config rebuild lands while this pass is walking. Its counts describe
    // globs no longer in force, so publishing them would overwrite the
    // rebuild's correct numbers — and the quiet-batch emit would then push
    // that stale value at the user, making a notice they just fixed return.
    let inner: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    for n in 0..12 {
        inner.write_file(&note_path("archive", n), NOTE).unwrap();
    }
    let ignore = Arc::new(ArcSwap::from_pointee(IgnoreSet::empty()));
    let store: Arc<dyn VaultStore> = Arc::new(SwapOnWalk {
        inner,
        ignore: ignore.clone(),
        swap_to: IgnoreSet::compile(&["archive/**".to_string()]).unwrap(),
    });

    // Stand in for the rebuild having already published its own counts.
    let rebuild_counts = IndexExclusions {
        ignored: 12,
        artefacts: 0,
        indexed: 0,
        ignore_looks_over_broad: true,
        config_generation: 7,
    };
    let deps = WatcherDeps {
        store,
        index: Arc::new(MemoryIndex::new()),
        ignore,
        exclusions: Arc::new(ArcSwap::from_pointee(rebuild_counts)),
    };

    let outcome = run_reconcile(&deps);

    assert!(outcome.ok, "the pass itself succeeded");
    assert!(
        !outcome.exclusions_changed,
        "a pass that raced a swap must not report a change, or the quiet \
         batch emits and the banner shows counts for globs that are gone"
    );
    assert_eq!(
        **deps.exclusions.load(),
        rebuild_counts,
        "the rebuild's counts are authoritative and must survive"
    );
}
