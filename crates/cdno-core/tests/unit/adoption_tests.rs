use std::fs;

use cdno_core::adoption::{AdoptionReadError, read_typst_adoption};

fn folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join("deck.typ"), "#image(\"figs/a.png\")").unwrap();
    fs::create_dir_all(p.join("figs/deep")).unwrap();
    fs::write(p.join("figs/a.png"), b"png").unwrap();
    fs::write(p.join("figs/deep/b.png"), b"png").unwrap();
    fs::write(p.join("refs.bib"), "@x{}").unwrap();
    fs::write(p.join("chapter.typ"), "= Two").unwrap();
    dir
}

#[test]
fn enumerates_every_regular_file_but_the_adopted_one_with_relative_names() {
    let dir = folder();
    let got = read_typst_adoption(&dir.path().join("deck.typ")).unwrap();
    assert_eq!(got.file_name, "deck.typ");
    assert_eq!(got.contents, "#image(\"figs/a.png\")");
    let names: Vec<&str> = got.assets.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(
        names,
        ["chapter.typ", "figs/a.png", "figs/deep/b.png", "refs.bib"]
    );
    for (src, _) in &got.assets {
        assert!(src.is_file());
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_folder_is_refused_by_name() {
    let dir = folder();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "x").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        dir.path().join("figs/link.png"),
    )
    .unwrap();
    let err = read_typst_adoption(&dir.path().join("deck.typ")).unwrap_err();
    assert!(matches!(&err, AdoptionReadError::Symlink(p) if p.ends_with("figs/link.png")));
    assert!(err.to_string().contains("link.png"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_folder_is_refused_too() {
    let dir = folder();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("elsewhere")).unwrap();
    let err = read_typst_adoption(&dir.path().join("deck.typ")).unwrap_err();
    assert!(matches!(err, AdoptionReadError::Symlink(_)));
}

#[test]
fn a_non_typ_file_and_a_missing_file_are_refused() {
    let dir = folder();
    assert!(matches!(
        read_typst_adoption(&dir.path().join("refs.bib")),
        Err(AdoptionReadError::NotTypst(_))
    ));
    assert!(matches!(
        read_typst_adoption(&dir.path().join("gone.typ")),
        Err(AdoptionReadError::Io { .. })
    ));
}
