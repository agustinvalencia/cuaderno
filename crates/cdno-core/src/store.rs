use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::StoreError;
use crate::file_meta::FileMeta;
use crate::path::VaultPath;

/// Abstract storage backend for a vault.
///
/// Implementations provide file-level operations against a rooted
/// location: the filesystem ([`FsVaultStore`](struct.FsVaultStore.html),
/// added later), in-memory for tests ([`MemoryVaultStore`]), or any
/// other backing medium.
///
/// All paths are [`VaultPath`]s — absolute paths and `..` components
/// are rejected at construction time, so implementations never need to
/// defend against vault-escape bugs.
///
/// Text content only: the markdown notes that make up a vault are
/// UTF-8. Attachments (PDFs, `.ipynb`, images) are discoverable via
/// [`exists`](Self::exists), [`list_dir`](Self::list_dir),
/// [`metadata`](Self::metadata), and relocatable via
/// [`move_file`](Self::move_file), but their binary content is not
/// read through this trait.
pub trait VaultStore: Send + Sync {
    /// Read a text file. Fails with [`StoreError::NotFound`] if the
    /// file does not exist.
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError>;

    /// Overwrite a file with the given text content, creating parent
    /// directories as needed.
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError>;

    /// Append text to an existing file, or create it if absent.
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError>;

    /// Move a file from `src` to `dest`. Fails with
    /// [`StoreError::AlreadyExists`] if `dest` is already present —
    /// callers that want to overwrite must delete first.
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError>;

    /// Report whether a file or directory exists at `path`.
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError>;

    /// List the direct children of a directory. Non-recursive;
    /// a recursive variant may arrive later as a separate method.
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Recursively enumerate all files beneath `path`.
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Return the modification time and size of a file.
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError>;
}

/// In-memory [`VaultStore`] used for fast, deterministic domain tests.
///
/// Backed by a `Mutex<HashMap>` so it satisfies the trait's
/// `Send + Sync` bound. Directories are implicit — they exist if any
/// file key has the directory as a prefix. There is no explicit
/// "create directory" operation and no way to represent an empty
/// directory, which matches how vaults are structured in practice:
/// directories only matter when they contain notes.
#[derive(Debug, Default)]
pub struct MemoryVaultStore {
    // Every file is keyed by its VaultPath. Since VaultPath rejects
    // absolute paths and `..` at construction, we never have to
    // defend against those here.
    files: Mutex<HashMap<VaultPath, MemoryFile>>,
}

/// One stored file's payload plus a modification timestamp.
/// `mtime` is refreshed on every write/append so `metadata()` can
/// return a realistic [`FileMeta`] for index-staleness checks.
#[derive(Debug, Clone)]
struct MemoryFile {
    content: String,
    mtime: SystemTime,
}

impl MemoryVaultStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl VaultStore for MemoryVaultStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        let files = self.files.lock().expect("poisoned mutex");
        files
            .get(path)
            .map(|f| f.content.clone())
            .ok_or_else(|| StoreError::NotFound(path.to_string()))
    }

    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        files.insert(
            path.clone(),
            MemoryFile {
                content: content.to_owned(),
                mtime: SystemTime::now(),
            },
        );
        Ok(())
    }

    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        let now = SystemTime::now();
        files
            .entry(path.clone())
            .and_modify(|f| {
                f.content.push_str(content);
                f.mtime = now;
            })
            .or_insert_with(|| MemoryFile {
                content: content.to_owned(),
                mtime: now,
            });
        Ok(())
    }

    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        if !files.contains_key(src) {
            return Err(StoreError::NotFound(src.to_string()));
        }
        if files.contains_key(dest) {
            return Err(StoreError::AlreadyExists(dest.to_string()));
        }
        let file = files.remove(src).expect("presence checked above");
        files.insert(dest.clone(), file);
        Ok(())
    }

    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        let files = self.files.lock().expect("poisoned mutex");

        // A direct key hit means the file itself is stored.
        if files.contains_key(path) {
            return Ok(true);
        }

        // Otherwise the path might still exist as an *implicit*
        // directory — i.e. a prefix under which some file is stored.
        // Three cases to handle:
        //
        //   1. Root (empty PathBuf): the vault "exists" as a directory
        //      whenever any file is stored.
        //   2. Regular prefix: some key strictly extends `target`.
        //      We exclude equality because that case already returned
        //      above, and `Path::starts_with` treats equal paths as
        //      matching (a file isn't its own parent directory).
        //   3. Nothing matches: the path doesn't exist as file or dir.
        let target = path.as_path();
        if target.as_os_str().is_empty() {
            return Ok(!files.is_empty());
        }
        let is_dir = files
            .keys()
            .any(|k| k.as_path().starts_with(target) && k.as_path() != target);
        Ok(is_dir)
    }

    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        // Listing a directory means reporting every *direct child* —
        // both files stored at this level and implicit subdirectories
        // that contain deeper files. We do this by walking all stored
        // keys, stripping the query prefix, and keeping only the first
        // path component of each remainder. A HashSet collapses
        // duplicates so a subdirectory with ten files shows up once.
        let files = self.files.lock().expect("poisoned mutex");
        let prefix = path.as_path();
        let mut children: HashSet<PathBuf> = HashSet::new();

        for key in files.keys() {
            let key_path = key.as_path();

            // Skip any key that isn't under `prefix`.
            let Ok(rel) = key_path.strip_prefix(prefix) else {
                continue;
            };

            // If the key *equals* the prefix, strip_prefix returns an
            // empty relative path with no components — skip it, since
            // a file isn't its own child.
            let Some(first) = rel.components().next() else {
                continue;
            };

            // Rebuild the child's full path. The root case (empty
            // prefix) needs special handling because `Path::new("")`
            // joined with a component yields `./component`, which
            // would fail VaultPath validation downstream.
            let child: PathBuf = if prefix.as_os_str().is_empty() {
                Path::new(first.as_os_str()).to_path_buf()
            } else {
                prefix.join(first)
            };
            children.insert(child);
        }

        // Children are derived from stored VaultPaths by taking
        // prefixes or components, so validation can't fail here.
        let mut result: Vec<VaultPath> = children
            .into_iter()
            .map(|p| VaultPath::new(p).expect("derived from validated VaultPaths"))
            .collect();
        result.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(result)
    }

    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        // Recursive variant of list_dir: every file beneath `prefix`,
        // not just direct children. Exact matches are excluded for
        // the same reason as in `exists` — a file isn't a descendant
        // of itself.
        let files = self.files.lock().expect("poisoned mutex");
        let prefix = path.as_path();
        let mut descendants: Vec<VaultPath> = files
            .keys()
            .filter(|key| {
                let key_path = key.as_path();
                key_path != prefix && key_path.starts_with(prefix)
            })
            .cloned()
            .collect();
        descendants.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(descendants)
    }

    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        let files = self.files.lock().expect("poisoned mutex");
        files
            .get(path)
            .map(|f| FileMeta::new(f.mtime, f.content.len() as u64))
            .ok_or_else(|| StoreError::NotFound(path.to_string()))
    }
}
