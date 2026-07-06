use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::error::StoreError;
use crate::file_meta::FileMeta;
use crate::path::VaultPath;

/// How long [`VaultStore::acquire_write_lock`] waits for the vault write
/// lock before giving up. Bounded so a wedged holder surfaces as a
/// [`StoreError::LockTimeout`] rather than an unbounded hang; mirrors the
/// SQLite index's `busy_timeout`. The OS releases the lock on process
/// death, so a crashed holder never deadlocks the next writer.
const WRITE_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Filename prefix for the in-progress temp file that atomic writes
/// stage next to their target. Stable and recognisable so the git
/// checkpoint (GH #303) can exclude it via `.git/info/exclude` — an
/// in-flight temp must never enter the recovery history, independent
/// of any lock. Exposed for the checkpoint to build its exclude rule.
pub const WIP_TEMP_PREFIX: &str = ".cdno-wip-";

/// Guard for the vault write lock (#196). For [`FsVaultStore`] it holds an
/// OS advisory lock on `.cuaderno/.lock`; for in-memory stores it is a
/// no-op. The lock releases when this guard drops — and, being tied to an
/// open file descriptor, on process death too.
#[derive(Debug)]
pub struct VaultWriteLock {
    file: Option<File>,
}

impl VaultWriteLock {
    /// A no-op guard, for stores that share no on-disk lock file.
    fn noop() -> Self {
        Self { file: None }
    }
}

impl Drop for VaultWriteLock {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            // Dropping the file closes the fd, which releases the lock
            // anyway; the explicit unlock just makes release prompt.
            let _ = file.unlock();
        }
    }
}

/// Abstract storage backend for a vault.
///
/// Implementations provide file-level operations against a rooted
/// location: the filesystem ([`FsVaultStore`](struct.FsVaultStore.html),
/// added later), in-memory for tests ([`MemoryVaultStore`]), or any
/// other backing medium.
///
/// All paths are [`VaultPath`]s, which guarantee only *lexical*
/// safety: absolute paths and `..` components are rejected at
/// construction. That is **not** sufficient for a real filesystem —
/// a symlink inside the vault can still point outside it — so
/// filesystem-backed implementations must enforce confinement
/// themselves and may reject a path with [`StoreError::OutsideVault`].
/// [`FsVaultStore`](struct.FsVaultStore.html) does this (canonicalise
/// against the root; also deny the `.git` control plane). Consequently
/// **any** method taking a `VaultPath` — reads included — may return
/// `OutsideVault` if the path escapes on disk.
///
/// Text content only: the markdown notes that make up a vault are
/// UTF-8. Attachments (PDFs, `.ipynb`, images) are discoverable via
/// [`exists`](Self::exists), [`list_dir`](Self::list_dir),
/// [`metadata`](Self::metadata), and relocatable via
/// [`move_file`](Self::move_file), but their binary content is not
/// read through this trait.
pub trait VaultStore: Send + Sync {
    /// Read a text file. Fails with [`StoreError::NotFound`] if the
    /// file does not exist (or [`StoreError::OutsideVault`] if the
    /// path escapes confinement — see the trait docs).
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError>;

    /// Overwrite a file with the given text content, creating parent
    /// directories as needed. Filesystem implementations write
    /// atomically (temp file + rename) so a reader never sees a
    /// partial note and a crash never truncates one.
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError>;

    /// Append text to an existing file, or create it if absent.
    ///
    /// **Concurrency contract:** filesystem implementations achieve
    /// this as a read-concat-atomic-rewrite (not `O_APPEND`), so the
    /// caller must hold the vault write lock
    /// ([`acquire_write_lock`](Self::acquire_write_lock)) across the
    /// call — as every transactional caller does — or a concurrent
    /// non-cdno writer's append can be lost rather than interleaved.
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError>;

    /// Move a file from `src` to `dest`. Fails with
    /// [`StoreError::AlreadyExists`] if `dest` is already present —
    /// callers that want to overwrite must delete first.
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError>;

    /// Remove a file. Fails with [`StoreError::NotFound`] if the
    /// file does not exist. Directory removal is out of scope — the
    /// vault never asks for an empty directory to be cleaned up.
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError>;

    /// Report whether a file or directory exists at `path`.
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError>;

    /// List the direct children of a directory. Non-recursive;
    /// a recursive variant may arrive later as a separate method.
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Recursively enumerate all files beneath `path`.
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Return the modification time and size of a file.
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError>;

    /// Copy an external file (`src`, an absolute or CWD-relative real
    /// filesystem path) into the vault at `dest`, creating parent
    /// directories as needed.
    ///
    /// This is the one trait method that handles arbitrary (possibly
    /// binary) bytes — it's how non-markdown attachments enter the vault
    /// (#154). The bytes are never read back through this trait; the
    /// imported file is referenced by a markdown stub, not parsed.
    ///
    /// **Create-only.** Fails with [`StoreError::NotFound`] (naming `src`)
    /// if the source doesn't exist, and with [`StoreError::AlreadyExists`]
    /// if `dest` is already occupied — it never overwrites. That keeps the
    /// transaction's import rollback (which deletes the created file) from
    /// ever deleting a pre-existing one, the same no-clobber posture as
    /// [`move_file`](Self::move_file).
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError>;

    /// Acquire the vault's exclusive write lock, to be held for the whole
    /// read-modify-write of a write operation so concurrent cdno
    /// processes serialise their writes instead of clobbering each other
    /// (#196). The returned guard releases on drop.
    ///
    /// The default is a no-op — correct for in-memory stores and test
    /// doubles, which share no on-disk file across processes.
    /// [`FsVaultStore`] overrides it with an OS advisory lock.
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        Ok(VaultWriteLock::noop())
    }
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

    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        if files.remove(path).is_none() {
            return Err(StoreError::NotFound(path.to_string()));
        }
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

    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        if !src.exists() {
            return Err(StoreError::NotFound(format!(
                "attachment source: {}",
                src.display()
            )));
        }
        let mut files = self.files.lock().expect("poisoned mutex");
        // Create-only, like the FS impl: refuse to clobber so the
        // transaction's import rollback (delete-the-created-file) can never
        // delete something that pre-existed.
        if files.contains_key(dest) {
            return Err(StoreError::AlreadyExists(dest.to_string()));
        }
        // The in-memory store holds text, but an imported attachment may be
        // binary — read it lossily. That round-trips exactly for the UTF-8
        // fixtures tests use and is harmless otherwise: the bytes are never
        // read back as meaningful content. Reading the *source* is
        // unavoidable (it's a real external file); no vault disk I/O happens.
        let bytes = fs::read(src).map_err(|e| io_to_store_error(e, dest))?;
        files.insert(
            dest.clone(),
            MemoryFile {
                content: String::from_utf8_lossy(&bytes).into_owned(),
                mtime: SystemTime::now(),
            },
        );
        Ok(())
    }
}

/// Filesystem-backed [`VaultStore`].
///
/// Takes a vault root directory on construction; all [`VaultPath`]
/// arguments are resolved relative to that root. The root is stored
/// as-is without validation — callers that need "root must exist" or
/// "root must be a directory" semantics should check upstream.
///
/// Two hardening layers added for remote serving (GH #303):
///
/// - **Confinement**: [`VaultPath`] already rejects `..` and absolute
///   paths lexically; `resolve` additionally verifies, per operation,
///   that the target — after following any symlinks that exist on the
///   way — stays under the (canonicalised) vault root. A symlink
///   *inside* the vault pointing *outside* is refused with
///   [`StoreError::OutsideVault`], as is anything whose confinement
///   cannot be verified (fail closed).
/// - **Atomic content writes**: `write_file`, `append_to_file` and
///   `import_external` materialise content in a temp file in the
///   target's directory and `rename(2)` it over the destination, so a
///   reader never observes a half-written note and a crash never
///   truncates one. (`append_to_file` is a read-concat-rewrite under
///   the hood — appends gain the same all-or-nothing guarantee at the
///   cost of rewriting the file, negligible at note scale.)
///
/// - **Control-plane deny-list**: even *inside* the root, a `.git`
///   component is refused (GH #303 security review) — the git audit
///   repository and a `.git` gitfile are never legitimate targets of
///   a store operation, and letting one through would let a write
///   corrupt or redirect the very recovery trail #303 exists to
///   provide. `VaultPath`'s slugging already prevents the MCP tools
///   from producing such a path; this makes the guarantee fail-closed
///   at the store rather than relying on that upstream convention.
///   (`.cuaderno/templates/` and `.cuaderno/config.toml` stay
///   writable — they are legitimate CLI targets.)
///
/// `move_file` checks destination presence manually before calling
/// `fs::rename` so the trait's [`StoreError::AlreadyExists`] contract
/// holds on Unix, where `rename` otherwise silently overwrites.
#[derive(Debug)]
pub struct FsVaultStore {
    root: PathBuf,
    /// Canonicalised root, memoised on the first SUCCESSFUL
    /// canonicalisation (the root is fixed for the store's lifetime,
    /// so a success never changes). Failure — root not yet on disk —
    /// is deliberately NOT cached: caching it would leave the symlink
    /// confinement layer permanently fail-open for a store built
    /// before its root exists (PR #306 verification finding).
    root_canon: std::sync::OnceLock<PathBuf>,
}

impl Clone for FsVaultStore {
    fn clone(&self) -> Self {
        // Don't copy the memoised canonical root: a clone is cheap and
        // re-memoises lazily, keeping the OnceLock semantics simple.
        Self::new(self.root.clone())
    }
}

impl FsVaultStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            root_canon: std::sync::OnceLock::new(),
        }
    }

    /// Resolve a vault-relative path to an absolute filesystem path,
    /// enforcing confinement (see the type docs).
    fn resolve(&self, path: &VaultPath) -> Result<PathBuf, StoreError> {
        let full = self.root.join(path.as_path());
        self.check_confinement(&full, path)?;
        Ok(full)
    }

    /// Filesystem-level confinement check. Two layers:
    ///
    /// 1. **Control-plane deny-list** (lexical, always on): any `.git`
    ///    path component is refused outright.
    /// 2. **Symlink escape** (filesystem): canonicalise the deepest
    ///    existing ancestor of `full` (which follows symlinks) and
    ///    require it under the canonicalised root.
    ///    - Root missing/un-canonicalisable ⇒ nothing on disk to
    ///      escape through; the lexically-validated join is safe.
    ///    - A dangling symlink on the path ⇒ refused: its confinement
    ///      cannot be verified, and fail-closed beats guessing.
    ///
    /// Cost: the deepest-ancestor `canonicalize` is one `realpath`
    /// syscall per op (acceptable at note scale — this store never
    /// runs hot loops of single-file ops); the root `canonicalize` is
    /// memoised so it is paid once, not per call.
    fn check_confinement(&self, full: &Path, path: &VaultPath) -> Result<(), StoreError> {
        // Layer 1: never touch the git control plane, even in-root.
        if path.as_path().components().any(|c| c.as_os_str() == ".git") {
            return Err(StoreError::OutsideVault(path.to_string()));
        }

        // Layer 2: symlink escape. Root canonicalisation memoised on
        // success only — failure is re-probed next call (see field).
        let root_canon = match self.root_canon.get() {
            Some(canon) => canon,
            None => match self.root.canonicalize() {
                Ok(canon) => self.root_canon.get_or_init(|| canon),
                // Root not on disk yet: nothing exists to escape
                // through, and the lexically-validated join is safe.
                Err(_) => return Ok(()),
            },
        };
        // Deepest component of `full` that exists (symlink_metadata
        // so a dangling symlink itself counts as "existing" and gets
        // canonicalised — and refused — rather than skipped).
        let mut probe: &Path = full;
        let existing = loop {
            if fs::symlink_metadata(probe).is_ok() {
                break probe;
            }
            match probe.parent() {
                Some(parent) => probe = parent,
                // Walked past the filesystem root without finding
                // anything: nothing exists to traverse through.
                None => return Ok(()),
            }
        };
        let canon = existing
            .canonicalize()
            .map_err(|_| StoreError::OutsideVault(path.to_string()))?;
        if !canon.starts_with(root_canon) {
            return Err(StoreError::OutsideVault(path.to_string()));
        }
        Ok(())
    }

    /// Write `content` atomically and durably: temp file in the
    /// target's directory, fsync'd, renamed over `full`, then the
    /// directory itself fsync'd. Same-directory placement keeps the
    /// `rename(2)` on one filesystem, which is what makes it atomic.
    ///
    /// Both fsyncs matter (PR #306 review, F5): the file fsync means a
    /// crash never exposes half-written bytes; the *directory* fsync
    /// means the rename itself survives a crash, so a write this
    /// function returns `Ok` for cannot silently roll back to the old
    /// content on power loss. Without the directory fsync the file
    /// fsync would be nearly wasted — durable data blocks orphaned by
    /// a non-durable rename.
    fn atomic_write(&self, full: &Path, content: &str, path: &VaultPath) -> Result<(), StoreError> {
        use std::io::Write;
        let parent = full
            .parent()
            .expect("resolved path always has a parent (root is a prefix)");
        fs::create_dir_all(parent).map_err(|e| io_to_store_error(e, path))?;
        let mut tmp = tempfile::Builder::new()
            .prefix(WIP_TEMP_PREFIX)
            .tempfile_in(parent)
            .map_err(|e| io_to_store_error(e, path))?;
        tmp.write_all(content.as_bytes())
            .map_err(|e| io_to_store_error(e, path))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| io_to_store_error(e, path))?;
        tmp.persist(full)
            .map_err(|e| io_to_store_error(e.error, path))?;
        // Make the rename durable. Best-effort: a filesystem that
        // rejects a directory fsync (rare) shouldn't fail an otherwise
        // successful write, and atomicity already held regardless.
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

/// Translate an `io::Error` into a `StoreError`, using the vault path
/// as the human-readable location (rather than the absolute disk path,
/// which would leak `TempDir` or `$HOME` into error messages).
fn io_to_store_error(err: io::Error, path: &VaultPath) -> StoreError {
    match err.kind() {
        io::ErrorKind::NotFound => StoreError::NotFound(path.to_string()),
        io::ErrorKind::PermissionDenied => StoreError::PermissionDenied(path.to_string()),
        io::ErrorKind::AlreadyExists => StoreError::AlreadyExists(path.to_string()),
        _ => StoreError::Io {
            path: path.to_string(),
            source: err,
        },
    }
}

impl VaultStore for FsVaultStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        fs::read_to_string(self.resolve(path)?).map_err(|e| io_to_store_error(e, path))
    }

    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        let full = self.resolve(path)?;
        self.atomic_write(&full, content, path)
    }

    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        // Read-concat-rewrite so the append inherits atomic_write's
        // all-or-nothing guarantee: a crash mid-append can no longer
        // leave a torn tail on the note. Cost: rewriting the file —
        // negligible at note scale. The read-modify window is covered
        // by the vault write lock for all transactional callers.
        let full = self.resolve(path)?;
        let existing = match fs::read_to_string(&full) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io_to_store_error(e, path)),
        };
        let combined = format!("{existing}{content}");
        self.atomic_write(&full, &combined, path)
    }

    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        let src_full = self.resolve(src)?;
        let dest_full = self.resolve(dest)?;

        // Preflight the destination manually: `fs::rename` on Unix
        // silently overwrites the target, which violates the trait's
        // AlreadyExists contract. A TOCTOU race with an external
        // process is theoretically possible but irrelevant for a
        // single-writer vault.
        if dest_full.exists() {
            return Err(StoreError::AlreadyExists(dest.to_string()));
        }
        if !src_full.exists() {
            return Err(StoreError::NotFound(src.to_string()));
        }
        if let Some(parent) = dest_full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_to_store_error(e, dest))?;
        }
        fs::rename(&src_full, &dest_full).map_err(|e| io_to_store_error(e, src))
    }

    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        let full = self.resolve(path)?;
        // `fs::remove_file` errors with NotFound if the path doesn't
        // exist; let io_to_store_error translate that into the trait's
        // contract variant.
        fs::remove_file(&full).map_err(|e| io_to_store_error(e, path))
    }

    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        Ok(self.resolve(path)?.exists())
    }

    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        let full = self.resolve(path)?;
        // Non-existent paths return empty rather than erroring, so
        // callers can treat "no such dir" and "empty dir" uniformly
        // and match MemoryVaultStore's semantics.
        if !full.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&full).map_err(|e| io_to_store_error(e, path))?;
        let mut children: Vec<VaultPath> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io_to_store_error(e, path))?;
            let name = entry.file_name();
            // Rebuild the child as a vault-relative path. Equivalent
            // of `path.join(name)` but VaultPath has no join yet, so
            // we rebuild via PathBuf and revalidate.
            let child_path = path.as_path().join(&name);
            let child =
                VaultPath::new(child_path).expect("child derived from VaultPath + valid file name");
            children.push(child);
        }
        children.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(children)
    }

    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        let full = self.resolve(path)?;
        if !full.exists() {
            return Ok(Vec::new());
        }

        // Iterative DFS over the subtree rooted at `full`. Yields
        // only regular files; directories are traversed but not
        // included. Output order is sorted at the end for
        // deterministic test comparisons.
        let mut stack: Vec<PathBuf> = vec![full.clone()];
        let mut out: Vec<VaultPath> = Vec::new();
        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir).map_err(|e| io_to_store_error(e, path))?;
            for entry in entries {
                let entry = entry.map_err(|e| io_to_store_error(e, path))?;
                let file_type = entry.file_type().map_err(|e| io_to_store_error(e, path))?;
                let entry_path = entry.path();
                if file_type.is_dir() {
                    stack.push(entry_path);
                } else {
                    // Strip the vault root so we end up with a
                    // vault-relative path that VaultPath accepts.
                    let rel = entry_path
                        .strip_prefix(&self.root)
                        .expect("entry is under vault root by construction");
                    let vp = VaultPath::new(rel)
                        .expect("rel is relative and free of traversal by construction");
                    out.push(vp);
                }
            }
        }
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        let std_meta = fs::metadata(self.resolve(path)?).map_err(|e| io_to_store_error(e, path))?;
        FileMeta::try_from(std_meta).map_err(|e| io_to_store_error(e, path))
    }

    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        // Name the *source* on a missing-source error — that's the path the
        // user typed, not the (not-yet-existing) vault destination.
        if !src.exists() {
            return Err(StoreError::NotFound(format!(
                "attachment source: {}",
                src.display()
            )));
        }
        let full = self.resolve(dest)?;
        // Create-only: refuse to overwrite an existing file, so the
        // transaction's import rollback (delete-the-created-file) is sound
        // and can never delete something that pre-existed. Mirrors
        // `move_file`'s no-clobber contract.
        if full.exists() {
            return Err(StoreError::AlreadyExists(dest.to_string()));
        }
        let parent = full
            .parent()
            .expect("resolved path always has a parent (root is a prefix)");
        fs::create_dir_all(parent).map_err(|e| io_to_store_error(e, dest))?;
        // Copy into a temp sibling, then no-clobber persist: the
        // destination appears atomically and never half-copied
        // (attachments can be large), and a TOCTOU race on the
        // exists() preflight can't silently overwrite. Same WIP
        // prefix as `atomic_write` so the git checkpoint's exclude
        // rule covers in-flight import temps too (PR #306
        // verification finding — this path was originally missed).
        let tmp = tempfile::Builder::new()
            .prefix(WIP_TEMP_PREFIX)
            .tempfile_in(parent)
            .map_err(|e| io_to_store_error(e, dest))?;
        fs::copy(src, tmp.path()).map_err(|e| io_to_store_error(e, dest))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| io_to_store_error(e, dest))?;
        tmp.persist_noclobber(&full)
            .map_err(|e| io_to_store_error(e.error, dest))?;
        // Same durability posture as `atomic_write`: make the rename
        // itself survive a crash (best-effort — see there).
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        // The lock file lives in `.cuaderno/` — present in any real vault,
        // but create it defensively so a freshly-`init`ed vault locks too.
        let dir = self.root.join(crate::paths::CUADERNO_DIR);
        fs::create_dir_all(&dir).map_err(|e| StoreError::Io {
            path: crate::paths::CUADERNO_DIR.to_string(),
            source: e,
        })?;
        let lock_path = dir.join(".lock");
        let lock_name = ".cuaderno/.lock";
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| StoreError::Io {
                path: lock_name.to_string(),
                source: e,
            })?;

        // Poll `try_lock` to a deadline rather than block forever: a wedged
        // holder times out, and the OS frees the lock on process death so a
        // crashed holder never deadlocks us.
        let deadline = Instant::now() + WRITE_LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(VaultWriteLock { file: Some(file) }),
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(StoreError::LockTimeout(WRITE_LOCK_TIMEOUT));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::Error(e)) => {
                    return Err(StoreError::Io {
                        path: lock_name.to_string(),
                        source: e,
                    });
                }
            }
        }
    }
}
