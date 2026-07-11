//! Unit tests for `Vault::create_question`, `Vault::set_question_status`,
//! and `Vault::active_questions` against `MemoryVaultStore` /
//! `MemoryIndex`.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::paths::daily_note_relpath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{QuestionDomain, QuestionFrontmatter, QuestionStatus};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn vault_with_seeded_store(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn read_question(store: &Arc<dyn VaultStore>, path: &VaultPath) -> QuestionFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    QuestionFrontmatter::try_from(fm).unwrap()
}

fn read_body(store: &Arc<dyn VaultStore>, path: &VaultPath) -> String {
    let raw = store.read_file(path).unwrap();
    let (_fm, body) = Frontmatter::parse(&raw).unwrap();
    body.to_owned()
}

// ---------------------------------------------------------------------
// create_question
// ---------------------------------------------------------------------

#[test]
fn create_question_uniquifies_a_stem_that_collides_with_another_note_type() {
    // #225: global stem uniqueness spans note types — a question whose slug
    // matches an existing project gets a `-2` stem, so the project's
    // `[[projects/surrogate]]` fallback stays unambiguous when it parks.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n---\n\n# Surrogate\n";
    let (vault, _store) = vault_with_seeded_store(&[("projects/surrogate.md", project)]);
    let path = vault
        .create_question(dt(2026, 1, 10, 9, 0), QuestionDomain::Research, "surrogate")
        .expect("create_question");
    assert_eq!(path, vp("questions/research/surrogate-2.md"));
}

#[test]
fn create_question_writes_under_research_folder_with_slug_from_text() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Can learned surrogates reduce simulation cost by 10x?",
        )
        .expect("create_question");

    assert_eq!(
        path,
        vp("questions/research/can-learned-surrogates-reduce-simulation-cost.md")
    );
    let fm = read_question(&store, &path);
    assert_eq!(fm.domain, QuestionDomain::Research);
    assert_eq!(fm.status, QuestionStatus::Active);
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
    assert_eq!(fm.updated, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
    let body = read_body(&store, &path);
    assert!(
        body.contains("# Can learned surrogates reduce simulation cost by 10x?"),
        "body should carry question as H1:\n{body}"
    );
}

#[test]
fn create_question_writes_under_life_folder_when_domain_is_life() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Life,
            "Where do I want to be in five years?",
        )
        .unwrap();
    assert_eq!(path, vp("questions/life/where-do-i-want-to-be.md"));
}

#[test]
fn create_question_errors_on_empty_text() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let err = vault
        .create_question(dt(2026, 1, 10, 9, 0), QuestionDomain::Research, "   ")
        .expect_err("empty question should error");
    assert!(matches!(err, DomainError::EmptyField { field: "question" }));
}

#[test]
fn create_question_suffixes_a_duplicate_slug_in_the_same_domain() {
    // #225: a second same-title question no longer errors — it gets a `-2`
    // stem so both keep resolvable backlinks.
    let (vault, _store) = vault_with_seeded_store(&[]);
    let first = vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();
    let second = vault
        .create_question(
            dt(2026, 1, 11, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .expect("a duplicate question now suffixes rather than erroring");
    assert_eq!(first, vp("questions/research/does-sparse-beat-dense.md"));
    assert_eq!(second, vp("questions/research/does-sparse-beat-dense-2.md"));
}

#[test]
fn create_question_suffixes_a_cross_domain_slug_collision() {
    // #225: global stem uniqueness spans the whole vault, so a life
    // question whose slug matches an existing research one gets a `-2` stem
    // — which also means the two never collide as an ambiguous slug.
    let (vault, _store) = vault_with_seeded_store(&[]);
    vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "What truly matters?",
        )
        .unwrap();
    let life = vault
        .create_question(
            dt(2026, 1, 11, 9, 0),
            QuestionDomain::Life,
            "What truly matters?",
        )
        .expect("cross-domain collision now suffixes");
    assert_eq!(life, vp("questions/life/what-truly-matters-2.md"));
}

// ---------------------------------------------------------------------
// set_question_status
// ---------------------------------------------------------------------

/// Daily note pre-seeded with a `## Logs` section so the
/// `stage_daily_log` write can append into it.
const DAILY_2026_05_01_WITH_LOGS: &str = "---\ndate: 2026-05-01\ntype: daily\n---\n\n# Friday, 1 May 2026\n\n## Logs\n- **08:00**: standup\n";

#[test]
fn set_question_status_bumps_status_and_updated_field() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();

    vault
        .set_question_status(
            dt(2026, 5, 1, 14, 30),
            "does-sparse-beat-dense",
            QuestionStatus::Answered,
        )
        .expect("set_question_status");

    let fm = read_question(&store, &path);
    assert_eq!(fm.status, QuestionStatus::Answered);
    assert_eq!(fm.updated, NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    // `created` is preserved.
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
}

#[test]
fn set_question_status_logs_was_now_to_daily_note() {
    let daily_path = daily_note_relpath(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    let (vault, store) =
        vault_with_seeded_store(&[(daily_path.as_str(), DAILY_2026_05_01_WITH_LOGS)]);
    vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();

    vault
        .set_question_status(
            dt(2026, 5, 1, 14, 30),
            "does-sparse-beat-dense",
            QuestionStatus::Parked,
        )
        .unwrap();

    let daily = store.read_file(&vp(&daily_path)).unwrap();
    assert!(
        daily.contains("status on [[questions/research/does-sparse-beat-dense]]"),
        "daily should reference the question by wikilink:\n{daily}"
    );
    assert!(daily.contains("  was: active"), "daily:\n{daily}");
    assert!(daily.contains("  now: parked"), "daily:\n{daily}");
    // The pre-existing standup entry is preserved.
    assert!(daily.contains("- **08:00**: standup"), "daily:\n{daily}");
}

#[test]
fn set_question_status_creates_daily_note_when_absent() {
    let (vault, store) = vault_with_seeded_store(&[]);
    vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();

    vault
        .set_question_status(
            dt(2026, 5, 1, 14, 30),
            "does-sparse-beat-dense",
            QuestionStatus::Retired,
        )
        .unwrap();

    let daily_path = daily_note_relpath(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    let daily = store.read_file(&vp(&daily_path)).unwrap();
    assert!(daily.contains("## Logs"), "daily:\n{daily}");
    assert!(daily.contains("status on [[questions/research/does-sparse-beat-dense]]"));
    assert!(daily.contains("  now: retired"));
}

#[test]
fn set_question_status_is_noop_when_status_unchanged() {
    let (vault, store) = vault_with_seeded_store(&[]);
    let path = vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();

    // No daily note seeded — a real status change would error on the
    // missing-section path; the no-op must short-circuit before that.
    vault
        .set_question_status(
            dt(2026, 5, 1, 14, 30),
            "does-sparse-beat-dense",
            QuestionStatus::Active,
        )
        .expect("noop must not error");

    let fm = read_question(&store, &path);
    // updated stays at created — proves no rewrite happened.
    assert_eq!(fm.updated, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());

    // And no daily note was created.
    let daily_path = daily_note_relpath(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    assert!(!store.exists(&vp(&daily_path)).unwrap());
}

#[test]
fn set_question_status_errors_when_slug_not_found() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    let err = vault
        .set_question_status(
            dt(2026, 5, 1, 14, 30),
            "nonexistent",
            QuestionStatus::Answered,
        )
        .expect_err("missing slug should error");
    assert!(matches!(err, DomainError::Store(StoreError::NotFound(_))));
}

#[test]
fn set_question_status_errors_on_ambiguous_cross_domain_slug() {
    // Hand-seed both files directly so we bypass the create-time
    // collision check; this is exactly the manual-edit scenario the
    // defensive branch protects against.
    let body = |domain: &str| {
        format!(
            "---\ntype: question\ndomain: {domain}\nstatus: active\ncreated: 2026-01-10\nupdated: 2026-01-10\n---\n\n# Same slug, two domains\n"
        )
    };
    let (vault, _store) = vault_with_seeded_store(&[
        ("questions/research/clash.md", &body("research")),
        ("questions/life/clash.md", &body("life")),
    ]);

    let err = vault
        .set_question_status(dt(2026, 5, 1, 14, 30), "clash", QuestionStatus::Parked)
        .expect_err("ambiguous slug should error");
    assert!(matches!(err, DomainError::AmbiguousSlug(s) if s == "clash"));
}

// ---------------------------------------------------------------------
// active_questions
// ---------------------------------------------------------------------

#[test]
fn active_questions_returns_only_active_sorted_by_domain_then_slug() {
    let (vault, _store) = vault_with_seeded_store(&[]);

    // Two research questions, one life, one parked (excluded).
    vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();
    vault
        .create_question(
            dt(2026, 1, 11, 9, 0),
            QuestionDomain::Research,
            "Can surrogates cut cost 10x?",
        )
        .unwrap();
    vault
        .create_question(
            dt(2026, 1, 12, 9, 0),
            QuestionDomain::Life,
            "Where do I want to be in five years?",
        )
        .unwrap();
    // Park one so it's filtered out.
    let daily_path = daily_note_relpath(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    let daily =
        "---\ndate: 2026-02-01\ntype: daily\n---\n\n# Sun, 1 Feb 2026\n\n## Logs\n".to_owned();
    let (vault, _store) = {
        let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
        let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
        store.write_file(&vp(&daily_path), &daily).unwrap();
        let (v, _r) =
            Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
        v.create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();
        v.create_question(
            dt(2026, 1, 11, 9, 0),
            QuestionDomain::Research,
            "Can surrogates cut cost 10x?",
        )
        .unwrap();
        v.create_question(
            dt(2026, 1, 12, 9, 0),
            QuestionDomain::Life,
            "Where do I want to be in five years?",
        )
        .unwrap();
        v.set_question_status(
            dt(2026, 2, 1, 9, 0),
            "where-do-i-want-to-be",
            QuestionStatus::Parked,
        )
        .unwrap();
        (v, store)
    };

    let active = vault.active_questions().unwrap();
    assert_eq!(active.len(), 2, "{active:?}");

    // Both research; sorted by slug.
    assert_eq!(active[0].slug, "can-surrogates-cut-cost-10x");
    assert_eq!(active[0].domain, QuestionDomain::Research);
    assert_eq!(active[0].question_text, "Can surrogates cut cost 10x?");
    assert_eq!(
        active[0].updated,
        NaiveDate::from_ymd_opt(2026, 1, 11).unwrap()
    );
    assert_eq!(active[1].slug, "does-sparse-beat-dense");
    assert_eq!(active[1].domain, QuestionDomain::Research);
}

#[test]
fn active_questions_carries_updated_date_from_frontmatter() {
    let daily_path = daily_note_relpath(NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    let daily =
        "---\ndate: 2026-04-05\ntype: daily\n---\n\n# Sun, 5 Apr 2026\n\n## Logs\n".to_owned();
    let (vault, _store) = vault_with_seeded_store(&[(daily_path.as_str(), &daily)]);
    vault
        .create_question(
            dt(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse beat dense?",
        )
        .unwrap();
    vault
        .set_question_status(
            dt(2026, 4, 5, 12, 0),
            "does-sparse-beat-dense",
            QuestionStatus::Active,
        )
        .unwrap();

    // No-op above (already active) — updated stays at created.
    let mut active = vault.active_questions().unwrap();
    assert_eq!(active.len(), 1);
    let q = active.pop().unwrap();
    assert_eq!(q.updated, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
}

#[test]
fn active_questions_returns_empty_for_empty_vault() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    assert!(vault.active_questions().unwrap().is_empty());
}

#[test]
fn question_not_found_lists_available_questions_sorted_across_domains() {
    let (vault, _store) = vault_with_seeded_store(&[]);
    // Two questions in different domains, created out of slug order.
    vault
        .create_question(dt(2026, 1, 10, 9, 0), QuestionDomain::Research, "Zeta path")
        .unwrap();
    vault
        .create_question(dt(2026, 1, 11, 9, 0), QuestionDomain::Life, "Alpha path")
        .unwrap();

    let err = vault
        .set_question_status(dt(2026, 1, 12, 9, 0), "missing", QuestionStatus::Parked)
        .unwrap_err();
    let DomainError::Store(StoreError::NotFound(msg)) = err else {
        panic!("expected Store(NotFound), got {err:?}");
    };
    assert!(
        msg.ends_with("available questions: alpha-path, zeta-path"),
        "expected slug-sorted hint across domains, got: {msg}"
    );
}
