//! The batch decision table (`plan_batch`) — pure, no AppHandle.

use std::time::{Duration, Instant};

use cdno_core::path::VaultPath;
use cdno_core::watcher::FileEvent;
use cdno_tauri::events::VaultArea;
use cdno_tauri::state::{ECHO_WINDOW, WriteJournal};
use cdno_tauri::watcher::{BatchPlan, plan_batch};

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
// mislabelled as an invalid config in the banner.
// -------------------------------------------------------------------

use cdno_core::error::{ConfigError, IndexError, StoreError, ValidationError};
use cdno_domain::error::DomainError;
use cdno_tauri::watcher::is_invalid_config_error;

#[test]
fn genuinely_invalid_config_errors_are_classified_invalid() {
    // A rejected note-type / schema surfaces as a Validation error.
    assert!(is_invalid_config_error(&DomainError::Validation(
        ValidationError::MissingField {
            field: "collaborators".to_owned(),
        },
    )));
    // A bad ignore glob and other config-content problems are Config errors.
    assert!(is_invalid_config_error(&DomainError::Config(
        ConfigError::InvalidGlob("**[".to_owned()),
    )));
    assert!(is_invalid_config_error(&DomainError::Config(
        ConfigError::InvalidNoteType("empty folder".to_owned()),
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
