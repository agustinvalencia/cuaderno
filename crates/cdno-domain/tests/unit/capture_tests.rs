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
fn capture_upserts_the_index_so_lint_can_see_the_note() {
    let (vault, _) = make_vault();
    vault.capture_to_inbox(moment(), "indexed note").unwrap();

    let report = vault.lint_all_notes().expect("lint");
    // `inbox` is a known NoteType, so a vanilla capture should
    // produce no lint issues — proving the index has the entry
    // (lint iterates the index, not the filesystem).
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}
