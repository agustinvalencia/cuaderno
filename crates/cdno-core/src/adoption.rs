//! Reading a Typst file and the folder beside it, for adoption (RFC 0008 §5.7).
//!
//! The domain does no file I/O, so the CLI and the server both call this to turn a path into
//! the file's text and the `(source, relative name)` list `adopt_typst_evidence` takes.
//! It lives in core because it is filesystem mechanics with no business rule in it.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Why a file and its folder could not be read for adoption.
#[derive(Debug, Error)]
pub enum AdoptionReadError {
    #[error("cannot adopt '{0}': it is not a `.typ` file")]
    NotTypst(PathBuf),
    #[error("cannot adopt '{0}': it is not a regular file")]
    NotAFile(PathBuf),
    #[error(
        "cannot adopt: '{0}' is a symbolic link, which could bring in a file from outside the folder; replace it with the real file or move it out of the folder"
    )]
    Symlink(PathBuf),
    #[error(
        "cannot adopt: '{0}' is neither a regular file nor a folder; move it out of the folder"
    )]
    Special(PathBuf),
    #[error("cannot adopt: '{0}' has a name that is not valid UTF-8")]
    NonUtf8Name(PathBuf),
    #[error("cannot adopt '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A Typst file read for adoption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypstAdoption {
    /// The file's text.
    pub contents: String,
    /// The file's name, e.g. `deck.typ`.
    pub file_name: String,
    /// Every regular file under the file's directory except the file itself, as the real
    /// source path and its `/`-separated name relative to that directory. Sorted by name.
    pub assets: Vec<(PathBuf, String)>,
}

/// Read `file` and enumerate the folder it sits in.
///
/// The folder is the file's own directory: the plan and the domain contract both take names
/// "relative to the file's directory", so a sibling `.typ` or `.bib` beside it is an asset.
/// Subfolders are included. A symbolic link anywhere in the folder is refused by name rather
/// than followed or skipped, so a link cannot pull in a file from outside it.
pub fn read_typst_adoption(file: &Path) -> Result<TypstAdoption, AdoptionReadError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| AdoptionReadError::Io { path, source }
    };
    let file_name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AdoptionReadError::NotTypst(file.to_path_buf()))?
        .to_owned();
    if !file_name.ends_with(".typ") {
        return Err(AdoptionReadError::NotTypst(file.to_path_buf()));
    }
    if !fs::metadata(file).map_err(io(file))?.is_file() {
        return Err(AdoptionReadError::NotAFile(file.to_path_buf()));
    }
    let contents = fs::read_to_string(file).map_err(io(file))?;
    let dir = match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut assets = Vec::new();
    collect(dir, "", Some(&file_name), &mut assets)?;
    assets.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(TypstAdoption {
        contents,
        file_name,
        assets,
    })
}

fn collect(
    dir: &Path,
    prefix: &str,
    skip: Option<&str>,
    out: &mut Vec<(PathBuf, String)>,
) -> Result<(), AdoptionReadError> {
    let entries = fs::read_dir(dir).map_err(|source| AdoptionReadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| AdoptionReadError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| AdoptionReadError::NonUtf8Name(path.clone()))?;
        if skip == Some(name.as_str()) {
            continue;
        }
        let kind = entry.file_type().map_err(|source| AdoptionReadError::Io {
            path: path.clone(),
            source,
        })?;
        let relative = format!("{prefix}{name}");
        if kind.is_symlink() {
            return Err(AdoptionReadError::Symlink(path));
        } else if kind.is_dir() {
            collect(&path, &format!("{relative}/"), None, out)?;
        } else if kind.is_file() {
            out.push((path, relative));
        } else {
            return Err(AdoptionReadError::Special(path));
        }
    }
    Ok(())
}
