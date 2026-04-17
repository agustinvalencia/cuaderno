use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::error::PathError;

/// A validated path inside a vault.
///
/// Always relative to the vault root; guaranteed free of absolute
/// prefixes and `..` components. The root of the vault is represented
/// by an empty inner `PathBuf` and is constructed via [`VaultPath::root`]
/// or by passing `"."` to [`VaultPath::new`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VaultPath(PathBuf);

impl VaultPath {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, PathError> {
        let input = path.as_ref();
        let as_str = input.to_string_lossy();

        if as_str.is_empty() {
            return Err(PathError::Empty);
        }

        let mut normalised = PathBuf::new();
        for component in input.components() {
            match component {
                Component::CurDir => continue,
                Component::ParentDir => {
                    return Err(PathError::ParentTraversal(as_str.into_owned()));
                }
                Component::Normal(part) => normalised.push(part),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(PathError::Absolute(as_str.into_owned()));
                }
            }
        }

        Ok(Self(normalised))
    }

    pub fn root() -> Self {
        Self(PathBuf::new())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.as_os_str().is_empty() {
            f.write_str(".")
        } else {
            write!(f, "{}", self.0.display())
        }
    }
}
