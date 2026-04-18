use crate::error::StoreError;
use crate::file_meta::FileMeta;
use crate::path::VaultPath;

/// Abstract storage backend for a vault.
///
/// Implementations provide file-level operations against a rooted
/// location: the filesystem ([`FsVaultStore`](struct.FsVaultStore.html),
/// added later), in-memory for tests
/// ([`MemoryVaultStore`](struct.MemoryVaultStore.html), added later),
/// or any other backing medium.
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
