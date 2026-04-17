use cdno_domain::note_type::{NoteType, ParseNoteTypeError};

#[test]
fn display_uses_kebab_case() {
    assert_eq!(NoteType::Daily.to_string(), "daily");
    assert_eq!(NoteType::Weekly.to_string(), "weekly");
    assert_eq!(NoteType::Project.to_string(), "project");
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
fn all_contains_ten_variants() {
    assert_eq!(NoteType::ALL.len(), 10);
}

#[test]
fn parse_accepts_kebab_case() {
    assert_eq!("daily".parse::<NoteType>().unwrap(), NoteType::Daily);
    assert_eq!("weekly".parse::<NoteType>().unwrap(), NoteType::Weekly);
    assert_eq!("project".parse::<NoteType>().unwrap(), NoteType::Project);
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
