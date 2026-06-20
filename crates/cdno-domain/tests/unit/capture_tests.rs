//! Tests for `Vault::capture_to_inbox` and the slug logic that
//! produces inbox filenames.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn moment() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 26)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(15, 47, 12).unwrap())
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, VaultConfig::default())
        .expect("Vault::new on empty store");
    (vault, store)
}

#[test]
fn capture_writes_a_file_in_inbox_with_a_slugged_filename() {
    let (vault, store) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "Buy groceries tomorrow")
        .expect("capture succeeds");

    assert_eq!(
        path,
        VaultPath::new("inbox/2026-04-26-buy-groceries-tomorrow.md").unwrap()
    );
    let content = store.read_file(&path).unwrap();
    assert!(content.contains("type: inbox"));
    assert!(content.contains("created: 2026-04-26T15:47:12"));
    assert!(content.contains("Buy groceries tomorrow"));
}

#[test]
fn capture_strips_punctuation_when_building_the_slug() {
    let (vault, _) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "Hello, world!")
        .expect("capture succeeds");

    assert_eq!(
        path,
        VaultPath::new("inbox/2026-04-26-hello-world.md").unwrap()
    );
}

#[test]
fn capture_falls_back_to_untitled_when_text_has_no_alphanumerics() {
    let (vault, _) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "...???   ")
        .expect("capture succeeds");

    assert_eq!(
        path,
        VaultPath::new("inbox/2026-04-26-untitled.md").unwrap()
    );
}

#[test]
fn capture_falls_back_to_untitled_for_empty_text() {
    let (vault, _) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "")
        .expect("capture succeeds");

    assert_eq!(
        path,
        VaultPath::new("inbox/2026-04-26-untitled.md").unwrap()
    );
}

#[test]
fn capture_keeps_only_the_first_six_words_in_the_slug() {
    let (vault, _) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "one two three four five six seven eight")
        .expect("capture succeeds");

    assert_eq!(
        path,
        VaultPath::new("inbox/2026-04-26-one-two-three-four-five-six.md").unwrap()
    );
}

#[test]
fn capture_truncates_pathologically_long_slugs() {
    let (vault, _) = make_vault();
    // Single 80-char word should land within the 50-char cap.
    let long = "a".repeat(80);

    let path = vault
        .capture_to_inbox(moment(), &long)
        .expect("capture succeeds");

    let stem = path.to_string();
    let slug_part = stem
        .strip_prefix("inbox/2026-04-26-")
        .and_then(|s| s.strip_suffix(".md"))
        .unwrap();
    assert!(slug_part.chars().count() <= 50, "slug: {slug_part}");
}

#[test]
fn capture_disambiguates_same_day_collisions_with_a_counter_suffix() {
    let (vault, _) = make_vault();

    let first = vault
        .capture_to_inbox(moment(), "buy groceries")
        .expect("capture #1");
    let second = vault
        .capture_to_inbox(moment(), "buy groceries")
        .expect("capture #2");
    let third = vault
        .capture_to_inbox(moment(), "buy groceries")
        .expect("capture #3");

    assert_eq!(
        first,
        VaultPath::new("inbox/2026-04-26-buy-groceries.md").unwrap()
    );
    assert_eq!(
        second,
        VaultPath::new("inbox/2026-04-26-buy-groceries-2.md").unwrap()
    );
    assert_eq!(
        third,
        VaultPath::new("inbox/2026-04-26-buy-groceries-3.md").unwrap()
    );
}

#[test]
fn capture_trims_leading_and_trailing_whitespace_in_the_body() {
    let (vault, store) = make_vault();

    let path = vault
        .capture_to_inbox(moment(), "   notable thought  \n")
        .expect("capture succeeds");
    let content = store.read_file(&path).unwrap();

    // Body sits after the closing `---\n\n` of the frontmatter.
    let (_, body) = content
        .split_once("---\n\n")
        .expect("frontmatter delimiter");
    assert_eq!(body, "notable thought\n");
}

#[test]
fn capture_errors_when_the_collision_safety_limit_is_exhausted() {
    // Pre-populate every available slot for `(date, slug)` up to the
    // counter cap so the next capture call hits the safety bound.
    // The cap matches `COLLISION_LIMIT` in `vault/capture.rs`; if
    // that constant changes, this loop must too.
    let (vault, store) = make_vault();
    const LIMIT: u32 = 100;
    for n in 1..LIMIT {
        let path = if n == 1 {
            VaultPath::new("inbox/2026-04-26-x.md").unwrap()
        } else {
            VaultPath::new(format!("inbox/2026-04-26-x-{n}.md")).unwrap()
        };
        store
            .write_file(&path, "---\ntype: inbox\n---\n")
            .expect("seed inbox slot");
    }

    let err = vault
        .capture_to_inbox(moment(), "x")
        .expect_err("collision limit must be reachable");
    let msg = format!("{err}");
    assert!(
        msg.contains("inbox/2026-04-26-x"),
        "unexpected error: {msg}"
    );
}

#[test]
fn capture_upserts_the_index_so_lint_can_see_the_note() {
    let (vault, _) = make_vault();
    vault.capture_to_inbox(moment(), "indexed note").unwrap();

    let report = vault.lint_all_notes().expect("lint");
    // `inbox` is a known NoteType, so a vanilla capture should
    // produce no lint issues — proving the index has the entry
    // (lint iterates the index, not the filesystem).
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

// ---------------------------------------------------------------------
// list_inbox / discard_inbox_item (triage, #208)
// ---------------------------------------------------------------------

fn dt(year: i32, month: u32, day: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

#[test]
fn list_inbox_returns_captures_oldest_first() {
    let (vault, _store) = make_vault();
    vault
        .capture_to_inbox(dt(2026, 4, 26), "second thing")
        .unwrap();
    vault
        .capture_to_inbox(dt(2026, 4, 25), "first thing")
        .unwrap();

    let items = vault.list_inbox().expect("list_inbox");
    assert_eq!(items.len(), 2);
    // The filename's date prefix sorts chronologically.
    assert_eq!(items[0].text, "first thing");
    assert!(items[0].slug.starts_with("2026-04-25-"));
    assert_eq!(items[1].text, "second thing");
}

#[test]
fn discard_inbox_item_removes_the_note() {
    let (vault, store) = make_vault();
    let path = vault
        .capture_to_inbox(moment(), "ephemeral thought")
        .unwrap();
    let slug = path
        .as_path()
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    vault
        .discard_inbox_item(moment(), &slug)
        .expect("discard succeeds");

    assert!(!store.exists(&path).unwrap(), "inbox note file deleted");
    assert!(
        vault.list_inbox().unwrap().is_empty(),
        "discarded item no longer listed"
    );
}

#[test]
fn discard_inbox_item_preserves_the_text_in_the_daily_log() {
    // The note is hard-deleted, so the daily log is the recovery record:
    // it must carry both the discard marker and the captured text.
    let (vault, store) = make_vault();
    let path = vault
        .capture_to_inbox(moment(), "remember the milk")
        .unwrap();
    let slug = path
        .as_path()
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    vault.discard_inbox_item(moment(), &slug).unwrap();

    // moment() is 2026-04-26.
    let daily = store
        .read_file(&VaultPath::new("journal/2026/daily/2026-04-26.md").unwrap())
        .expect("daily note written");
    assert!(daily.contains("discarded"), "daily:\n{daily}");
    assert!(daily.contains("remember the milk"), "daily:\n{daily}");
}

#[test]
fn discard_inbox_item_errors_on_missing_slug() {
    let (vault, _store) = make_vault();
    let err = vault
        .discard_inbox_item(moment(), "2026-01-01-nope")
        .unwrap_err();
    assert!(
        matches!(err, cdno_domain::error::DomainError::Store(_)),
        "got {err:?}"
    );
}
