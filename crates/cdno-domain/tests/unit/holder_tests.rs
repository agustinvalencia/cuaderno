//! `hand_off` (RFC 0006 §7.2, §8.2): handing a project to a named holder,
//! taking it back, and the guards around both.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{
    HolderReading, HolderSentinels, ProjectFrontmatter, ProjectStatus, holder_reading_from_yaml,
};
use cdno_domain::{HolderChange, LOG_HOLDER_PREFIX, NO_HOLDER, Vault};
use chrono::{NaiveDate, NaiveDateTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

const DAILY: &str = "journal/2026/daily/2026-10-20.md";

struct Fixture {
    vault: Vault,
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
}

fn fixture_with_config(notes: &[(&str, &str)], config: VaultConfig) -> Fixture {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config).expect("Vault::new");
    Fixture {
        vault,
        store,
        index,
    }
}

fn fixture(notes: &[(&str, &str)]) -> Fixture {
    fixture_with_config(notes, VaultConfig::default())
}

/// A project map with an optional raw `held_by:` line and one open action.
fn map(status: &str, title: &str, held_by: Option<&str>) -> String {
    let holder = held_by
        .map(|h| format!("held_by: {h}\n"))
        .unwrap_or_default();
    format!(
        "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-01-01\n{holder}---\n\n\
         # {title}\n\n## Current State\nGoing.\n\n## Next Actions\n- [ ] Draft the survey request (medium)\n\n\
         ## Waiting On\n(nothing yet)\n"
    )
}

/// Enough own active maps to fill the default cap, as `(path, body)`.
fn own_maps_at_the_cap() -> Vec<(String, String)> {
    let cap = VaultConfig::default().vault.max_active_projects;
    (1..=cap)
        .map(|i| {
            (
                format!("projects/own-{i}.md"),
                map("active", &format!("Own {i}"), None),
            )
        })
        .collect()
}

fn as_refs(notes: &[(String, String)]) -> Vec<(&str, &str)> {
    notes
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect()
}

fn frontmatter_of(store: &Arc<dyn VaultStore>, path: &str) -> ProjectFrontmatter {
    let raw = store.read_file(&vp(path)).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    ProjectFrontmatter::try_from(fm).unwrap()
}

/// How the reader classifies the map's `held_by:` straight from YAML.
fn holder_of(store: &Arc<dyn VaultStore>, path: &str) -> HolderReading {
    let raw = store.read_file(&vp(path)).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    let value = fm.optional_field::<serde_yaml::Value>("held_by").unwrap();
    holder_reading_from_yaml(
        value.as_ref(),
        &HolderSentinels::from_config(&VaultConfig::default()),
    )
}

fn hand_to(name: &str) -> HolderChange {
    HolderChange::HandTo(name.to_owned())
}

#[test]
fn hand_off_writes_a_quoted_holder_and_one_log_entry() {
    let f = fixture(&[("projects/harbour.md", &map("active", "Harbour", None))]);

    let outcome = f
        .vault
        .hand_off(at(20, 9, 15), "harbour", hand_to("the harbour office"))
        .expect("hand off");
    assert!(outcome.touched());
    assert_eq!(outcome.primary, vp("projects/harbour.md"));

    let raw = f.store.read_file(&vp("projects/harbour.md")).unwrap();
    assert!(
        raw.contains("\nheld_by: \"the harbour office\"\n---\n"),
        "quoted, inserted before the closing marker: {raw}"
    );
    assert_eq!(
        holder_of(&f.store, "projects/harbour.md"),
        HolderReading::Named("the harbour office".to_owned())
    );
    assert_eq!(
        f.vault.slot_usage().unwrap().counted,
        0,
        "the map stops counting"
    );

    let daily = f.store.read_file(&vp(DAILY)).unwrap();
    let entry =
        format!("{LOG_HOLDER_PREFIX}[[harbour]]\n  was: {NO_HOLDER}\n  now: the harbour office");
    assert_eq!(daily.matches(LOG_HOLDER_PREFIX).count(), 1, "{daily}");
    assert!(
        daily.contains("- **09:15**: holder on [[harbour]]"),
        "{daily}"
    );
    assert!(daily.contains(&entry[LOG_HOLDER_PREFIX.len()..]), "{daily}");

    // Taking back strikes the name: `held_by: null`, and a second entry.
    f.vault
        .hand_off(at(20, 16, 0), "harbour", HolderChange::TakeBack)
        .expect("take back");
    let raw = f.store.read_file(&vp("projects/harbour.md")).unwrap();
    assert!(raw.contains("\nheld_by: null\n"), "{raw}");
    assert_eq!(raw.matches("held_by:").count(), 1, "{raw}");
    let daily = f.store.read_file(&vp(DAILY)).unwrap();
    assert!(
        daily.contains(&format!(
            "holder on [[harbour]]\n  was: the harbour office\n  now: {NO_HOLDER}"
        )),
        "{daily}"
    );
    assert_eq!(f.vault.slot_usage().unwrap().counted, 1);
}

#[test]
fn a_holder_with_quotes_colons_and_a_wikilink_round_trips() {
    let names = [
        "Ana \"the boat\" O'Neil: hull",
        r"the yard \ dry dock",
        "- the night shift",
        "[[people/ana]]",
        "#harbour # office",
    ];
    for name in names {
        let f = fixture(&[("projects/harbour.md", &map("active", "Harbour", None))]);
        f.vault
            .hand_off(at(20, 9, 0), "harbour", hand_to(name))
            .unwrap_or_else(|e| panic!("hand off to {name:?}: {e}"));
        assert_eq!(
            holder_of(&f.store, "projects/harbour.md"),
            HolderReading::Named(name.to_owned()),
            "{name:?} reads back exactly"
        );
        assert_eq!(
            frontmatter_of(&f.store, "projects/harbour.md")
                .held_by
                .as_deref(),
            Some(name)
        );
        // Replacing an existing value keeps the shape too.
        f.vault
            .hand_off(at(20, 10, 0), "harbour", hand_to("the harbour office"))
            .unwrap();
        f.vault
            .hand_off(at(20, 11, 0), "harbour", hand_to(name))
            .unwrap();
        assert_eq!(
            holder_of(&f.store, "projects/harbour.md"),
            HolderReading::Named(name.to_owned()),
            "{name:?} survives a rewrite"
        );
    }
}

#[test]
fn a_wikilink_holder_appears_in_the_person_notes_backlinks() {
    let person = "---\ntype: person\ncreated: 2026-01-01\n---\n\n# Ana\n";
    let f = fixture(&[
        ("projects/harbour.md", &map("active", "Harbour", None)),
        ("people/ana.md", person),
    ]);
    assert!(
        f.index
            .find_backlinks(&vp("people/ana.md"))
            .unwrap()
            .is_empty()
    );

    f.vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("[[people/ana]]"))
        .expect("hand off");

    let backlinks = f.index.find_backlinks(&vp("people/ana.md")).unwrap();
    assert!(
        backlinks.contains(&vp("projects/harbour.md")),
        "the map links its holder: {backlinks:?}"
    );
}

#[test]
fn sentinels_and_placeholders_are_refused_with_take_back_advice() {
    let body = map("active", "Harbour", None);
    let f = fixture(&[("projects/harbour.md", &body)]);

    for value in ["me", " Myself ", "I", "(mine)", "TBD", "someone", "?", "-"] {
        let err = f
            .vault
            .hand_off(at(20, 9, 0), "harbour", hand_to(value))
            .unwrap_err();
        match &err {
            DomainError::HolderNotAName { value: v } => assert_eq!(v, value.trim()),
            other => panic!("{value:?}: expected HolderNotAName, got {other:?}"),
        }
        let message = err.to_string();
        assert!(message.contains("names you or nobody"), "{message}");
        assert!(message.contains("take it back"), "{message}");
    }

    let blank = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("   "))
        .unwrap_err();
    assert!(
        matches!(blank, DomainError::EmptyField { field: "held_by" }),
        "{blank:?}"
    );
    let broken = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("Ana\nstatus: parked"))
        .unwrap_err();
    assert!(
        matches!(broken, DomainError::UnrepresentableFrontmatterValue { .. }),
        "{broken:?}"
    );

    // The deny-list is the configured one: a vault that lists "nadie"
    // refuses it, and one that empties the list accepts "me".
    let mut config = VaultConfig::default();
    config.projects.holder_sentinels = vec!["nadie".to_owned()];
    let g = fixture_with_config(&[("projects/harbour.md", &body)], config);
    assert!(matches!(
        g.vault
            .hand_off(at(20, 9, 0), "harbour", hand_to("Nadie"))
            .unwrap_err(),
        DomainError::HolderNotAName { .. }
    ));
    g.vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("me"))
        .expect("an unlisted value is a name");

    // Nothing was written by any refusal.
    assert_eq!(f.store.read_file(&vp("projects/harbour.md")).unwrap(), body);
    assert!(!f.store.exists(&vp(DAILY)).unwrap());
}

#[test]
fn hand_off_while_an_action_on_the_map_is_in_focus_is_project_in_focus() {
    let body = map("active", "Harbour", None);
    let f = fixture(&[("projects/harbour.md", &body)]);
    f.vault
        .start_action(at(20, 9, 0), "harbour", "Draft the survey request")
        .expect("start");
    let before = f.store.read_file(&vp(DAILY)).unwrap();

    let err = f
        .vault
        .hand_off(at(20, 9, 30), "harbour", hand_to("the harbour office"))
        .unwrap_err();
    match &err {
        DomainError::ProjectInFocus { slug, action } => {
            assert_eq!(slug, "harbour");
            assert_eq!(action, "Draft the survey request (medium)");
        }
        other => panic!("expected ProjectInFocus, got {other:?}"),
    }
    assert!(err.to_string().contains("pause it first"), "{err}");
    assert_eq!(f.store.read_file(&vp("projects/harbour.md")).unwrap(), body);
    assert_eq!(f.store.read_file(&vp(DAILY)).unwrap(), before);

    // Pausing first, then handing off, succeeds.
    f.vault
        .pause_action(at(20, 10, 0), None, None)
        .expect("pause");
    f.vault
        .hand_off(at(20, 10, 5), "harbour", hand_to("the harbour office"))
        .expect("hand off after pausing");
}

#[test]
fn a_focus_on_another_project_does_not_block_a_hand_off() {
    let f = fixture(&[
        ("projects/harbour.md", &map("active", "Harbour", None)),
        ("projects/thesis.md", &map("active", "Thesis", None)),
    ]);
    f.vault
        .start_action(at(20, 9, 0), "thesis", "Draft the survey request")
        .expect("start");

    f.vault
        .hand_off(at(20, 9, 30), "harbour", hand_to("the harbour office"))
        .expect("a focus elsewhere does not block");
    assert_eq!(
        holder_of(&f.store, "projects/harbour.md"),
        HolderReading::Named("the harbour office".to_owned())
    );
}

#[test]
fn take_back_on_a_parked_map_at_the_cap_clears_the_name_and_leaves_it_parked() {
    let mut notes = own_maps_at_the_cap();
    notes.push((
        "projects/_parked/harbour.md".to_owned(),
        map("parked", "Harbour", Some("\"the harbour office\"")),
    ));
    let f = fixture(&as_refs(&notes));
    let before = f.vault.slot_usage().unwrap();
    assert_eq!(before.counted, usize::from(before.cap));

    f.vault
        .hand_off(at(20, 9, 0), "harbour", HolderChange::TakeBack)
        .expect("a parked take-back needs no slot");

    let fm = frontmatter_of(&f.store, "projects/_parked/harbour.md");
    assert_eq!(fm.status, ProjectStatus::Parked);
    assert_eq!(fm.held_by, None);
    assert!(!f.store.exists(&vp("projects/harbour.md")).unwrap());
    assert_eq!(f.vault.slot_usage().unwrap(), before);
    let daily = f.store.read_file(&vp(DAILY)).unwrap();
    assert!(!daily.contains("activated"), "{daily}");
}

#[test]
fn take_back_on_an_active_held_map_at_the_cap_is_refused() {
    let mut notes = own_maps_at_the_cap();
    let held = map("active", "Harbour", Some("\"the harbour office\""));
    notes.push(("projects/harbour.md".to_owned(), held.clone()));
    let f = fixture(&as_refs(&notes));

    let err = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", HolderChange::TakeBack)
        .unwrap_err();
    match err {
        DomainError::ProjectCapReached {
            current,
            max,
            active_projects,
        } => {
            assert_eq!((current, max), (5, 5));
            assert!(!active_projects.contains(&"harbour".to_owned()));
        }
        other => panic!("expected ProjectCapReached, got {other:?}"),
    }
    assert_eq!(f.store.read_file(&vp("projects/harbour.md")).unwrap(), held);
    assert!(!f.store.exists(&vp(DAILY)).unwrap());
}

#[test]
fn hand_off_on_a_closed_map_is_project_not_active() {
    let closed = "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2026-01-01\n\
                  closed: 2026-09-01\n---\n\n# Harbour\n";
    let f = fixture(&[("projects/_done/2026/harbour.md", closed)]);

    for change in [hand_to("the harbour office"), HolderChange::TakeBack] {
        let err = f
            .vault
            .hand_off(at(20, 9, 0), "harbour", change)
            .unwrap_err();
        assert!(
            matches!(
                err,
                DomainError::ProjectNotActive {
                    status: ProjectStatus::Completed,
                    ..
                }
            ),
            "{err:?}"
        );
    }
}

#[test]
fn hand_off_changes_nothing_in_commitments() {
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\n---\n\n\
                # Harbour\n\n## Current State\nGoing.\n\n## Next Actions\n\n## Milestones\n\
                - [ ] Survey booked (hard: 2026-11-02)\n";
    let f = fixture(&[("projects/harbour.md", body)]);
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let before = f.vault.commitments(today, 30).unwrap();
    assert!(!before.is_empty(), "the fixture carries a hard date");

    f.vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("the harbour office"))
        .unwrap();
    assert_eq!(f.vault.commitments(today, 30).unwrap(), before);

    f.vault
        .hand_off(at(20, 10, 0), "harbour", HolderChange::TakeBack)
        .unwrap();
    assert_eq!(f.vault.commitments(today, 30).unwrap(), before);
}

#[test]
fn starting_a_focus_on_a_held_map_never_changes_its_holder() {
    let held = map("active", "Harbour", Some("\"the harbour office\""));
    let f = fixture(&[("projects/harbour.md", &held)]);

    f.vault
        .start_action(at(20, 9, 0), "harbour", "Draft the survey request")
        .expect("start on a held map");

    assert_eq!(
        f.store.read_file(&vp("projects/harbour.md")).unwrap(),
        held,
        "the map is byte-identical"
    );
}

#[test]
fn park_keeps_the_holder() {
    let f = fixture(&[(
        "projects/harbour.md",
        &map("active", "Harbour", Some("\"the harbour office\"")),
    )]);

    f.vault.park_project(at(20, 9, 0), "harbour").expect("park");
    assert_eq!(
        frontmatter_of(&f.store, "projects/_parked/harbour.md")
            .held_by
            .as_deref(),
        Some("the harbour office")
    );

    f.vault
        .activate_project(at(20, 10, 0), "harbour")
        .expect("activate");
    assert_eq!(
        frontmatter_of(&f.store, "projects/harbour.md")
            .held_by
            .as_deref(),
        Some("the harbour office")
    );
}

#[test]
fn the_same_holder_again_is_a_noop() {
    let held = map("active", "Harbour", Some("\"the harbour office\""));
    let own = map("active", "Thesis", None);
    let f = fixture(&[("projects/harbour.md", &held), ("projects/thesis.md", &own)]);

    let outcome = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("  the harbour office "))
        .unwrap();
    assert!(!outcome.touched());
    let outcome = f
        .vault
        .hand_off(at(20, 9, 0), "thesis", HolderChange::TakeBack)
        .unwrap();
    assert!(
        !outcome.touched(),
        "a map that is yours is taken back already"
    );

    assert_eq!(f.store.read_file(&vp("projects/harbour.md")).unwrap(), held);
    assert_eq!(f.store.read_file(&vp("projects/thesis.md")).unwrap(), own);
    assert!(!f.store.exists(&vp(DAILY)).unwrap(), "no log entry");
}

#[test]
fn take_back_strikes_a_written_holder_that_reads_as_yours() {
    // An unquoted wikilink (a nested YAML sequence) and a sentinel both read
    // as yours, but the line is still there: taking back strikes it and
    // logs it, `was:` as written.
    for (written, was) in [("[[people/ana]]", "[[people/ana]]"), ("me", "me")] {
        let f = fixture(&[(
            "projects/harbour.md",
            &map("active", "Harbour", Some(written)),
        )]);

        let outcome = f
            .vault
            .hand_off(at(20, 9, 0), "harbour", HolderChange::TakeBack)
            .expect("take back");
        assert!(outcome.touched(), "{written:?}");

        let raw = f.store.read_file(&vp("projects/harbour.md")).unwrap();
        assert!(raw.contains("\nheld_by: null\n"), "{raw}");
        assert_eq!(raw.matches("held_by").count(), 1, "{raw}");
        let daily = f.store.read_file(&vp(DAILY)).unwrap();
        assert!(
            daily.contains(&format!(
                "holder on [[harbour]]\n  was: {was}\n  now: {NO_HOLDER}"
            )),
            "{daily}"
        );
    }
}

#[test]
fn take_back_on_a_null_holder_is_a_noop() {
    let body = map("active", "Harbour", Some("null"));
    let f = fixture(&[("projects/harbour.md", &body)]);

    let outcome = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", HolderChange::TakeBack)
        .unwrap();
    assert!(!outcome.touched());
    assert_eq!(f.store.read_file(&vp("projects/harbour.md")).unwrap(), body);
    assert!(!f.store.exists(&vp(DAILY)).unwrap(), "no log entry");
}

#[test]
fn a_multi_line_or_unquoted_holder_is_replaced_by_one_quoted_line() {
    // Each hand-written shape is replaced whole: exactly one `held_by:` line,
    // quoted, and no orphaned continuation lines.
    let shapes = [
        "\n  - people/ana\n  - people/ben",
        "[[people/ana]]",
        "the harbour office",
        "|\n  the harbour\n  office",
        "{ name: ana }",
    ];
    for shape in shapes {
        let f = fixture(&[(
            "projects/harbour.md",
            &map("active", "Harbour", Some(shape)),
        )]);
        f.vault
            .hand_off(at(20, 9, 0), "harbour", hand_to("the night shift"))
            .unwrap_or_else(|e| panic!("{shape:?}: {e}"));

        let raw = f.store.read_file(&vp("projects/harbour.md")).unwrap();
        let (frontmatter, _) = raw[4..].split_once("\n---\n").unwrap();
        let held: Vec<&str> = frontmatter
            .lines()
            .filter(|l| l.contains("held_by") || l.starts_with(' ') || l.starts_with('-'))
            .collect();
        assert_eq!(
            held,
            vec!["held_by: \"the night shift\""],
            "{shape:?}: {raw}"
        );
        assert_eq!(
            holder_of(&f.store, "projects/harbour.md"),
            HolderReading::Named("the night shift".to_owned())
        );
    }
}

#[test]
fn yaml_lookalike_holders_are_names_and_a_date_is_refused() {
    // Quoted, every YAML lookalike is a string, so it reads back as the name
    // given (padding trimmed). A date would read as no one, so it is refused.
    let names = [
        ("123", "123"),
        ("true", "true"),
        ("null", "null"),
        ("~", "~"),
        ("   the harbour office  ", "the harbour office"),
    ];
    for (given, read) in names {
        let f = fixture(&[("projects/harbour.md", &map("active", "Harbour", None))]);
        f.vault
            .hand_off(at(20, 9, 0), "harbour", hand_to(given))
            .unwrap_or_else(|e| panic!("{given:?}: {e}"));
        assert_eq!(
            holder_of(&f.store, "projects/harbour.md"),
            HolderReading::Named(read.to_owned()),
            "{given:?}"
        );
    }

    let f = fixture(&[("projects/harbour.md", &map("active", "Harbour", None))]);
    let err = f
        .vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("2026-10-20"))
        .unwrap_err();
    assert!(
        matches!(err, DomainError::InvalidFieldValue { ref field, .. } if field == "held_by"),
        "{err:?}"
    );
}

#[test]
fn the_logs_no_holder_marker_is_on_the_default_deny_list() {
    // Someone copying `(mine)` from a `holder on` entry into `held_by:`
    // must write a value that reads as theirs.
    assert!(
        cdno_core::config::DEFAULT_HOLDER_SENTINELS.contains(&NO_HOLDER),
        "{NO_HOLDER} missing from DEFAULT_HOLDER_SENTINELS"
    );
}

#[test]
fn hand_off_on_a_parked_map_writes_the_holder_and_leaves_it_parked() {
    let f = fixture(&[(
        "projects/_parked/harbour.md",
        &map("parked", "Harbour", None),
    )]);

    f.vault
        .hand_off(at(20, 9, 0), "harbour", hand_to("the harbour office"))
        .expect("a parked map takes a holder");

    let fm = frontmatter_of(&f.store, "projects/_parked/harbour.md");
    assert_eq!(fm.status, ProjectStatus::Parked);
    assert_eq!(fm.held_by.as_deref(), Some("the harbour office"));
    assert!(!f.store.exists(&vp("projects/harbour.md")).unwrap());
    let daily = f.store.read_file(&vp(DAILY)).unwrap();
    assert!(
        daily.contains("holder on [[harbour]]\n  was: (mine)\n  now: the harbour office"),
        "{daily}"
    );
}
