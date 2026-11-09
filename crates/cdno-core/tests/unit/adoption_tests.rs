use std::fs;

use cdno_core::adoption::{AdoptionReadError, read_typst_adoption, read_typst_adoption_within};

fn vault() -> std::path::PathBuf {
    let v = tempfile::tempdir().unwrap();
    v.keep()
}

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
    let got = read_typst_adoption(&dir.path().join("deck.typ"), &vault()).unwrap();
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
    let err = read_typst_adoption(&dir.path().join("deck.typ"), &vault()).unwrap_err();
    assert!(matches!(&err, AdoptionReadError::Symlink(p) if p.ends_with("figs/link.png")));
    assert!(err.to_string().contains("link.png"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_folder_is_refused_too() {
    let dir = folder();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("elsewhere")).unwrap();
    let err = read_typst_adoption(&dir.path().join("deck.typ"), &vault()).unwrap_err();
    assert!(matches!(err, AdoptionReadError::Symlink(_)));
}

#[test]
fn a_non_typ_file_and_a_missing_file_are_refused() {
    let dir = folder();
    assert!(matches!(
        read_typst_adoption(&dir.path().join("refs.bib"), &vault()),
        Err(AdoptionReadError::NotTypst(_))
    ));
    assert!(matches!(
        read_typst_adoption(&dir.path().join("gone.typ"), &vault()),
        Err(AdoptionReadError::Io { .. })
    ));
}

#[test]
fn the_file_count_limit_is_inclusive() {
    let dir = folder(); // four assets
    let deck = dir.path().join("deck.typ");
    assert!(read_typst_adoption_within(&deck, &vault(), 4, u64::MAX).is_ok());
    let err = read_typst_adoption_within(&deck, &vault(), 3, u64::MAX).unwrap_err();
    assert!(matches!(
        err,
        AdoptionReadError::TooLarge { max_files: 3, .. }
    ));
    assert!(err.to_string().contains("folder of their own"), "{err}");
}

#[test]
fn the_size_limit_is_inclusive() {
    let dir = folder(); // 3 + 3 + 4 bytes of assets, plus "= Two" (5)
    let deck = dir.path().join("deck.typ");
    let total = 3 + 3 + 4 + 5 + 20; // the deck itself counts: `#image("figs/a.png")`
    assert!(read_typst_adoption_within(&deck, &vault(), 10, total).is_ok());
    let err = read_typst_adoption_within(&deck, &vault(), 10, total - 1).unwrap_err();
    assert!(matches!(err, AdoptionReadError::TooLarge { .. }));
}

#[cfg(unix)]
#[test]
fn a_symlinked_deck_is_refused() {
    let dir = folder();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("real.typ"), "= x").unwrap();
    std::os::unix::fs::symlink(outside.path().join("real.typ"), dir.path().join("link.typ"))
        .unwrap();
    let err = read_typst_adoption(&dir.path().join("link.typ"), &vault()).unwrap_err();
    assert!(matches!(&err, AdoptionReadError::Symlink(p) if p.ends_with("link.typ")));
}

#[test]
fn dot_entries_are_skipped_at_any_depth() {
    let dir = folder();
    let p = dir.path();
    fs::write(p.join(".env"), "SECRET=1").unwrap();
    fs::write(p.join(".DS_Store"), "x").unwrap();
    fs::create_dir_all(p.join(".git/objects")).unwrap();
    fs::write(p.join(".git/HEAD"), "ref").unwrap();
    fs::write(p.join("figs/.hidden"), "x").unwrap();
    fs::create_dir_all(p.join("figs/.cache")).unwrap();
    fs::write(p.join("figs/.cache/y"), "x").unwrap();
    let got = read_typst_adoption(&p.join("deck.typ"), &vault()).unwrap();
    let names: Vec<&str> = got.assets.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(
        names,
        ["chapter.typ", "figs/a.png", "figs/deep/b.png", "refs.bib"]
    );
}

#[test]
fn a_folder_that_is_under_or_holds_the_vault_is_refused() {
    let dir = folder();
    let deck = dir.path().join("deck.typ");
    // The folder is the vault.
    let err = read_typst_adoption(&deck, dir.path()).unwrap_err();
    assert!(
        matches!(err, AdoptionReadError::OverlapsVault { .. }),
        "{err}"
    );
    // The folder is under the vault.
    let err = read_typst_adoption(&deck, dir.path().parent().unwrap()).unwrap_err();
    assert!(
        matches!(err, AdoptionReadError::OverlapsVault { .. }),
        "{err}"
    );
    // The folder holds the vault.
    fs::create_dir(dir.path().join("vault")).unwrap();
    let err = read_typst_adoption(&deck, &dir.path().join("vault")).unwrap_err();
    assert!(
        matches!(err, AdoptionReadError::OverlapsVault { .. }),
        "{err}"
    );
    // An unrelated vault is fine.
    assert!(read_typst_adoption(&deck, &vault()).is_ok());
}

#[test]
fn an_oversized_deck_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("big.typ");
    fs::write(&deck, vec![b'a'; 11]).unwrap();
    assert!(read_typst_adoption_within(&deck, &vault(), 10, 11).is_ok());
    let err = read_typst_adoption_within(&deck, &vault(), 10, 10).unwrap_err();
    assert!(matches!(err, AdoptionReadError::TooLarge { files: 0, .. }));
}

#[cfg(unix)]
#[test]
fn an_unreadable_asset_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let dir = folder();
    let locked = dir.path().join("refs.bib");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let got = read_typst_adoption(&dir.path().join("deck.typ"), &vault());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    if let Err(err) = got {
        assert!(err.to_string().contains("refs.bib"), "{err}");
    } // running as root can read it, and then there is nothing to refuse
}
