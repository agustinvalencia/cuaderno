//! `From<DomainError>` mapping directions + the serialised shape
//! `commands.ts` pattern-matches on.

use cdno_core::error::StoreError;
use cdno_domain::error::DomainError;
use cdno_tauri::error::CmdError;

#[test]
fn cap_and_ambiguity_map_to_their_structured_variants() {
    let cap: CmdError = DomainError::ProjectCapReached {
        current: 5,
        max: 5,
        active_projects: vec!["a".into()],
    }
    .into();
    assert!(matches!(cap, CmdError::ProjectCapReached { max: 5, .. }));

    let ambiguous: CmdError = DomainError::AmbiguousSlug("gym".into()).into();
    assert!(matches!(
        ambiguous,
        CmdError::Ambiguous { ref candidates, .. } if candidates.is_empty()
    ));
}

#[test]
fn user_fixable_errors_never_degrade_to_internal() {
    let fixable: CmdError = DomainError::EmptyField { field: "title" }.into();
    assert!(matches!(fixable, CmdError::Invalid(_)));

    let missing: CmdError =
        DomainError::Store(StoreError::NotFound("projects/ghost.md".into())).into();
    assert!(matches!(missing, CmdError::NotFound(_)));
}

#[test]
fn io_and_escape_failures_are_internal_and_generic() {
    let io: CmdError = DomainError::Store(StoreError::Io {
        path: "projects/alpha.md".into(),
        source: std::io::Error::other("disk exploded: secret detail"),
    })
    .into();
    let CmdError::Internal(message) = io else {
        panic!("Io must map to Internal");
    };
    assert!(
        !message.contains("secret detail"),
        "internals must not cross the bridge"
    );

    let escape: CmdError =
        DomainError::Store(StoreError::OutsideVault("../../etc/passwd".into())).into();
    assert!(matches!(escape, CmdError::Internal(_)));
}

#[test]
fn serialised_shape_matches_the_frontend_contract() {
    // The adjacently-tagged form commands.ts and the ts-rs binding
    // both rely on: {"kind": ..., "data": ...}.
    let value = serde_json::to_value(CmdError::NotFound("projects/ghost.md".into())).unwrap();
    assert_eq!(value["kind"], "not_found");
    assert_eq!(value["data"], "projects/ghost.md");

    let value = serde_json::to_value(CmdError::ProjectCapReached {
        current: 5,
        max: 5,
        active: vec!["a".into()],
    })
    .unwrap();
    assert_eq!(value["kind"], "project_cap_reached");
    assert_eq!(value["data"]["active"][0], "a");
}
