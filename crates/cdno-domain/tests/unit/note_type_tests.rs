use cdno_domain::note_type::{NoteType, ParseNoteTypeError};

#[test]
fn display_uses_kebab_case() {
    assert_eq!(NoteType::Daily.to_string(), "daily");
    assert_eq!(NoteType::Weekly.to_string(), "weekly");
    assert_eq!(NoteType::Monthly.to_string(), "monthly");
    assert_eq!(NoteType::Project.to_string(), "project");
    assert_eq!(NoteType::Action.to_string(), "action");
    assert_eq!(NoteType::Portfolio.to_string(), "portfolio");
    assert_eq!(NoteType::Evidence.to_string(), "evidence");
    assert_eq!(NoteType::Stewardship.to_string(), "stewardship");
    assert_eq!(NoteType::Tracking.to_string(), "tracking");
    assert_eq!(NoteType::Question.to_string(), "question");
    assert_eq!(NoteType::Commitment.to_string(), "commitment");
    assert_eq!(NoteType::Inbox.to_string(), "inbox");
}

#[test]
fn as_str_matches_display() {
    for nt in NoteType::ALL {
        assert_eq!(nt.as_str(), nt.to_string());
    }
}

#[test]
fn all_contains_twelve_variants() {
    assert_eq!(NoteType::ALL.len(), 12);
}

#[test]
fn parse_accepts_kebab_case() {
    assert_eq!("daily".parse::<NoteType>().unwrap(), NoteType::Daily);
    assert_eq!("weekly".parse::<NoteType>().unwrap(), NoteType::Weekly);
    assert_eq!("monthly".parse::<NoteType>().unwrap(), NoteType::Monthly);
    assert_eq!("project".parse::<NoteType>().unwrap(), NoteType::Project);
    assert_eq!("action".parse::<NoteType>().unwrap(), NoteType::Action);
    assert_eq!(
        "portfolio".parse::<NoteType>().unwrap(),
        NoteType::Portfolio
    );
    assert_eq!("evidence".parse::<NoteType>().unwrap(), NoteType::Evidence);
    assert_eq!(
        "stewardship".parse::<NoteType>().unwrap(),
        NoteType::Stewardship
    );
    assert_eq!("tracking".parse::<NoteType>().unwrap(), NoteType::Tracking);
    assert_eq!("question".parse::<NoteType>().unwrap(), NoteType::Question);
    assert_eq!(
        "commitment".parse::<NoteType>().unwrap(),
        NoteType::Commitment
    );
    assert_eq!("inbox".parse::<NoteType>().unwrap(), NoteType::Inbox);
}

#[test]
fn parse_rejects_unknown() {
    let err = "journal".parse::<NoteType>().unwrap_err();
    assert!(matches!(err, ParseNoteTypeError(s) if s == "journal"));
}

#[test]
fn parse_is_case_sensitive() {
    assert!("Daily".parse::<NoteType>().is_err());
    assert!("DAILY".parse::<NoteType>().is_err());
}

#[test]
fn serde_roundtrip_yaml() {
    let json = serde_json::to_string(&NoteType::Stewardship).unwrap();
    assert_eq!(json, "\"stewardship\"");
    let back: NoteType = serde_json::from_str(&json).unwrap();
    assert_eq!(back, NoteType::Stewardship);
}

#[test]
fn serde_roundtrip_yaml_literal() {
    let yaml = serde_yaml::to_string(&NoteType::Commitment).unwrap();
    assert_eq!(yaml.trim(), "commitment");
    let back: NoteType = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(back, NoteType::Commitment);
}

#[test]
fn reference_fields_match_the_rfc_table() {
    // RFC 0007 §3.5: the bare-slug fields each type carries, and the type
    // each one names.
    assert_eq!(
        NoteType::Action.reference_fields(),
        &[("project", NoteType::Project)]
    );
    assert_eq!(
        NoteType::Commitment.reference_fields(),
        &[
            ("project", NoteType::Project),
            ("stewardship", NoteType::Stewardship),
        ]
    );
    assert_eq!(
        NoteType::Tracking.reference_fields(),
        &[("stewardship", NoteType::Stewardship)]
    );
    assert_eq!(
        NoteType::Evidence.reference_fields(),
        &[("portfolio", NoteType::Portfolio)]
    );
    assert_eq!(
        NoteType::Inbox.reference_fields(),
        &[("captured_during", NoteType::Project)]
    );
    for nt in NoteType::ALL {
        if !matches!(
            nt,
            NoteType::Action
                | NoteType::Commitment
                | NoteType::Tracking
                | NoteType::Evidence
                | NoteType::Inbox
        ) {
            assert!(nt.reference_fields().is_empty(), "{nt} names no other note");
        }
    }
}

#[test]
fn reference_keys_are_the_four_keys_once_each() {
    let mut keys = NoteType::reference_keys().to_vec();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["captured_during", "portfolio", "project", "stewardship"]
    );
    // Every key a type's table names is in the union. Changing this set
    // changes what is masked: bump `cdno_core::hash::MASK_VERSION` with it.
    for nt in NoteType::ALL {
        for (key, _) in nt.reference_fields() {
            assert!(NoteType::reference_keys().contains(key));
        }
    }
}
