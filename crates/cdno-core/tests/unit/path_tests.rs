use cdno_core::error::PathError;
use cdno_core::path::VaultPath;
use std::collections::HashSet;
use std::path::PathBuf;

#[test]
fn new_accepts_simple_relative_path() {
    let p = VaultPath::new("projects/foo.md").unwrap();
    assert_eq!(p.as_path(), PathBuf::from("projects/foo.md").as_path());
}

#[test]
fn new_accepts_nested_relative_path() {
    let p = VaultPath::new("journal/daily/2026-04-17.md").unwrap();
    assert_eq!(
        p.as_path(),
        PathBuf::from("journal/daily/2026-04-17.md").as_path()
    );
}

#[test]
fn new_rejects_absolute_path() {
    let err = VaultPath::new("/etc/passwd").unwrap_err();
    assert!(matches!(err, PathError::Absolute(_)));
}

#[test]
fn new_rejects_parent_traversal_at_root() {
    let err = VaultPath::new("..").unwrap_err();
    assert!(matches!(err, PathError::ParentTraversal(_)));
}

#[test]
fn new_rejects_parent_traversal_in_middle() {
    let err = VaultPath::new("projects/../../etc").unwrap_err();
    assert!(matches!(err, PathError::ParentTraversal(_)));
}

#[test]
fn new_rejects_parent_traversal_at_end() {
    let err = VaultPath::new("projects/..").unwrap_err();
    assert!(matches!(err, PathError::ParentTraversal(_)));
}

#[test]
fn new_rejects_empty_string() {
    let err = VaultPath::new("").unwrap_err();
    assert!(matches!(err, PathError::Empty));
}

#[test]
fn new_normalises_dot_to_root() {
    let p = VaultPath::new(".").unwrap();
    assert_eq!(p, VaultPath::root());
}

#[test]
fn new_strips_leading_dot_slash() {
    let stripped = VaultPath::new("./projects/foo.md").unwrap();
    let plain = VaultPath::new("projects/foo.md").unwrap();
    assert_eq!(stripped, plain);
}

#[test]
fn root_is_empty_pathbuf() {
    let root = VaultPath::root();
    assert_eq!(root.as_path(), PathBuf::new().as_path());
}

#[test]
fn equality_ignores_constructor_form() {
    let a = VaultPath::new(".").unwrap();
    let b = VaultPath::root();
    assert_eq!(a, b);
}

#[test]
fn hash_consistent_with_equality() {
    let mut set = HashSet::new();
    set.insert(VaultPath::new(".").unwrap());
    assert!(set.contains(&VaultPath::root()));
}

#[test]
fn display_shows_path_string() {
    let p = VaultPath::new("projects/foo.md").unwrap();
    assert_eq!(format!("{p}"), "projects/foo.md");
}

#[test]
fn display_of_root_is_dot() {
    assert_eq!(format!("{}", VaultPath::root()), ".");
}

#[test]
fn accepts_pathbuf_input() {
    let pb = PathBuf::from("projects/foo.md");
    let p = VaultPath::new(&pb).unwrap();
    assert_eq!(p.as_path(), pb.as_path());
}
