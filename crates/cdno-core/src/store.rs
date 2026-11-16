use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::error::StoreError;
use crate::file_meta::FileMeta;
use crate::path::VaultPath;
use crate::read_journal::{Observed, ReadJournals};

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

/// A store whose write lock the caller already holds: every operation goes
/// to `inner`, except [`acquire_write_lock`](VaultStore::acquire_write_lock),
/// which returns a no-op guard. The lock is one per open file, so code that
/// takes it again in the same thread (a reconcile pass's per-note
/// transactions) would otherwise wait for itself until the timeout. Only
/// [`crate::reconcile::rebuild`] uses it, for the pass it runs under the
/// lock; anything else would lose the lock's protection.
pub(crate) struct LockHeldStore(pub(crate) Arc<dyn VaultStore>);

impl VaultStore for LockHeldStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.0.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.0.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.0.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.0.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.0.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.0.delete_file(path)
    }
    fn remove_dir_if_empty(&self, dir: &VaultPath) -> Result<bool, StoreError> {
        self.0.remove_dir_if_empty(dir)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.0.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.0.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.0.walk_dir(path)
    }
    fn walk_dirs(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.0.walk_dirs(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.0.metadata(path)
    }
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.0.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        Ok(VaultWriteLock::noop())
    }
    // The comparison (#1001) is the inner store's: only the lock differs.
    fn read_journals(&self) -> Option<&ReadJournals> {
        self.0.read_journals()
    }
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        self.0.write_file_expecting(path, content, expected)
    }
    fn append_to_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        self.0.append_to_file_expecting(path, content, expected)
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

    /// Read a file's raw bytes — the binary escape hatch to `read_file`,
    /// for the non-text attachments the vault also holds (images embedded
    /// in a note, chiefly). Same confinement and `NotFound` semantics as
    /// [`read_file`](Self::read_file); the caller owns content-type and
    /// size decisions.
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError>;

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

    /// Remove a directory, but only when it exists and is empty.
    ///
    /// Returns `Ok(true)` when the directory was removed. Returns
    /// `Ok(false)` — and touches nothing — when `dir` is absent, is not
    /// a directory (a symlink to one counts as not a directory), or
    /// holds anything at all, a hidden file included. An error is
    /// returned only for an I/O failure outside those cases. A path
    /// that does not resolve inside the vault (a symlink pointing
    /// outside it, or a dangling one) is an error, as for every other
    /// method. Callers use it to prune emptied folders and so can never
    /// lose content.
    ///
    /// The default returns `Ok(false)`, which suits stores with no
    /// directories of their own.
    fn remove_dir_if_empty(&self, _dir: &VaultPath) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Report whether a file or directory exists at `path`.
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError>;

    /// List the direct children of a directory. Non-recursive;
    /// a recursive variant may arrive later as a separate method.
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Recursively enumerate all files beneath `path`.
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError>;

    /// Recursively enumerate every directory beneath `path`, at any depth,
    /// `path` itself excluded, sorted. Read-only. A symlink is never
    /// followed and never listed, whatever it points at. An absent `path`
    /// lists nothing. [`walk_dir`](Self::walk_dir) lists files only, so this
    /// is how a caller finds the empty folders it may prune with
    /// [`remove_dir_if_empty`](Self::remove_dir_if_empty).
    ///
    /// The default lists nothing, which suits stores with no directories of
    /// their own.
    fn walk_dirs(&self, _path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        Ok(Vec::new())
    }

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

    // ---- compare before rename (#1001) ---------------------------------
    //
    // The three methods below have no default on purpose: a default would
    // let a wrapping store silently lose the read journal, the pre-rename
    // check or the no-clobber create. Every store states its behaviour; a
    // wrapper delegates them to the store it wraps.

    /// The read journals this store records its reads into (#1001), so a
    /// transaction can compare what an operation read with what is on
    /// disk before replacing it. A store that records reads returns its
    /// [`ReadJournals`] and calls [`ReadJournals::record`] from
    /// [`read_file`](Self::read_file) and [`read_bytes`](Self::read_bytes),
    /// and [`ReadJournals::record_absent`] when [`exists`](Self::exists)
    /// answers false.
    ///
    /// `None` means the store records nothing. A transaction over it then
    /// takes each path's baseline when the write is staged, later than the
    /// operation's read, so the comparison covers less of the
    /// read-modify-write.
    fn read_journals(&self) -> Option<&ReadJournals>;

    /// What is at `path` right now: [`Observed::Absent`], or the
    /// fingerprint of its bytes. Reads through
    /// [`read_bytes`](Self::read_bytes).
    fn observe(&self, path: &VaultPath) -> Result<Observed, StoreError> {
        match self.read_bytes(path) {
            Ok(bytes) => Ok(Observed::of_bytes(&bytes)),
            Err(StoreError::NotFound(_)) => Ok(Observed::Absent),
            Err(e) => Err(e),
        }
    }

    /// [`write_file`](Self::write_file), but only if `path` still holds
    /// `expected`; otherwise [`StoreError::Changed`] and nothing written.
    /// With `expected` [`Observed::Absent`] it is a create that must never
    /// replace a file another writer made.
    ///
    /// [`FsVaultStore`] compares right before the rename, after the new
    /// content is written and synced to its temp file, and creates with a
    /// rename that refuses to overwrite; [`MemoryVaultStore`] compares and
    /// writes under one mutex.
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError>;

    /// [`append_to_file`](Self::append_to_file), but only if `path` still
    /// holds `expected` (see [`write_file_expecting`](Self::write_file_expecting)).
    fn append_to_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError>;
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
    /// Reads recorded for open transactions (#1001).
    journals: ReadJournals,
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
        let result = files
            .get(path)
            .map(|f| f.content.clone())
            .ok_or_else(|| StoreError::NotFound(path.to_string()));
        drop(files);
        self.journals.record(path, &result);
        result
    }

    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        // The in-memory store keeps text content; hand back its bytes so
        // the plumbing is exercisable without real binary fixtures.
        let files = self.files.lock().expect("poisoned mutex");
        let result = files
            .get(path)
            .map(|f| f.content.clone().into_bytes())
            .ok_or_else(|| StoreError::NotFound(path.to_string()));
        drop(files);
        self.journals.record(path, &result);
        result
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

    // Directories are implicit here; the trait default applies.

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
        drop(files);
        if !is_dir {
            self.journals.record_absent(path);
        }
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

    fn walk_dirs(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        // Directories are implicit: every proper ancestor of a stored file
        // that lies strictly beneath `path`.
        let files = self.files.lock().expect("poisoned mutex");
        let prefix = path.as_path();
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        for key in files.keys() {
            for dir in key.as_path().ancestors().skip(1) {
                if dir == prefix || !dir.starts_with(prefix) || dir.as_os_str().is_empty() {
                    break;
                }
                dirs.insert(dir.to_path_buf());
            }
        }
        let mut result: Vec<VaultPath> = dirs
            .into_iter()
            .map(|p| VaultPath::new(p).expect("derived from validated VaultPaths"))
            .collect();
        result.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(result)
    }

    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        let files = self.files.lock().expect("poisoned mutex");
        files
            .get(path)
            .map(|f| FileMeta::new(f.mtime, f.content.len() as u64))
            .ok_or_else(|| StoreError::NotFound(path.to_string()))
    }

    fn read_journals(&self) -> Option<&ReadJournals> {
        Some(&self.journals)
    }

    // Check and write under the one mutex: no window at all here.
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        if memory_observed(&files, path) != expected {
            return Err(StoreError::Changed(path.clone()));
        }
        files.insert(
            path.clone(),
            MemoryFile {
                content: content.to_owned(),
                mtime: SystemTime::now(),
            },
        );
        Ok(())
    }

    fn append_to_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        let mut files = self.files.lock().expect("poisoned mutex");
        if memory_observed(&files, path) != expected {
            return Err(StoreError::Changed(path.clone()));
        }
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

/// What the in-memory store holds at `path`, for its conditional writes.
fn memory_observed(files: &HashMap<VaultPath, MemoryFile>, path: &VaultPath) -> Observed {
    files.get(path).map_or(Observed::Absent, |f| {
        Observed::of_bytes(f.content.as_bytes())
    })
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
    /// Where the write lock lives when not `.cuaderno/.lock`; see
    /// [`with_lock_file`](Self::with_lock_file).
    lock_file: Option<PathBuf>,
    /// Reads recorded for open transactions (#1001). Per instance: a
    /// clone starts with none open.
    journals: ReadJournals,
    /// Test seams for the conditional write (#1001); empty in production.
    hooks: TestHooks,
}

/// Points at which a test can step into a conditional write. Set only
/// through the `test-support` feature's setters; empty otherwise.
#[derive(Clone, Default)]
struct TestHooks {
    /// Runs after the temp file is written and synced, right before the
    /// comparison that precedes the rename.
    before_compare: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Makes the no-clobber rename of a create fail with this OS error.
    noclobber_error: Option<i32>,
}

impl std::fmt::Debug for TestHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestHooks")
            .field("before_compare", &self.before_compare.is_some())
            .field("noclobber_error", &self.noclobber_error)
            .finish()
    }
}

impl Clone for FsVaultStore {
    fn clone(&self) -> Self {
        // Don't copy the memoised canonical root: a clone is cheap and
        // re-memoises lazily, keeping the OnceLock semantics simple.
        let mut clone = Self::new(self.root.clone());
        clone.lock_file.clone_from(&self.lock_file);
        clone.hooks = self.hooks.clone();
        clone
    }
}

/// Whether a failed no-clobber rename means the filesystem cannot do one
/// at all (no rename flag, no hard links), as opposed to an ordinary
/// failure that must be reported.
fn noclobber_unsupported(e: &io::Error) -> bool {
    if matches!(
        e.kind(),
        io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput
    ) {
        return true;
    }
    #[cfg(unix)]
    {
        matches!(
            e.raw_os_error(),
            Some(libc::ENOSYS | libc::ENOTSUP | libc::EPERM | libc::EMLINK)
        ) || e.raw_os_error() == Some(libc::EOPNOTSUPP)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

impl FsVaultStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            root_canon: std::sync::OnceLock::new(),
            lock_file: None,
            journals: ReadJournals::new(),
            hooks: TestHooks::default(),
        }
    }

    /// Take the write lock on `lock_file` (an absolute path) instead of
    /// the vault's `.cuaderno/.lock`.
    ///
    /// A test seam, compiled only with the `test-support` feature: two
    /// stores over one folder with different lock files hold their locks
    /// independently, which is how a test reproduces a folder written from
    /// two kernels, where `flock` is not shared (#1001). A real vault
    /// always locks `.cuaderno/.lock`.
    #[cfg(feature = "test-support")]
    pub fn with_lock_file(mut self, lock_file: impl Into<PathBuf>) -> Self {
        self.lock_file = Some(lock_file.into());
        self
    }

    /// Run `hook` inside every conditional write, after the temp file is
    /// written and synced and before the comparison that precedes the
    /// rename (test seam, `test-support` only).
    #[cfg(feature = "test-support")]
    pub fn with_before_compare_hook(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.hooks.before_compare = Some(Arc::new(hook));
        self
    }

    /// Make the no-clobber rename of every create fail with the OS error
    /// `raw` instead of running (test seam, `test-support` only).
    #[cfg(feature = "test-support")]
    pub fn with_noclobber_error(mut self, raw: i32) -> Self {
        self.hooks.noclobber_error = Some(raw);
        self
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
        self.atomic_write_expecting(full, content, path, None)
    }

    /// [`atomic_write`](Self::atomic_write), refusing with
    /// [`StoreError::Changed`] when `expected` is given and `full` no
    /// longer holds it. The check runs after the temp file is written and
    /// fsync'd, immediately before the rename, so the window it leaves is
    /// one read and compare wide rather than the whole write (#1001). On
    /// refusal the temp file is removed and the target is untouched.
    fn atomic_write_expecting(
        &self,
        full: &Path,
        content: &str,
        path: &VaultPath,
        expected: Option<Observed>,
    ) -> Result<(), StoreError> {
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
        // Everything above (the temp write and its sync, the slow part) is
        // done; the comparisons below are the last thing before the rename.
        if let Some(hook) = &self.hooks.before_compare {
            hook();
        }
        match expected {
            None => {
                tmp.persist(full)
                    .map_err(|e| io_to_store_error(e.error, path))?;
            }
            // A create: the kernel refuses the rename if anything is there,
            // so a file another writer created first is never replaced, at
            // any moment. No window at all.
            Some(Observed::Absent) => {
                let refused = match self.hooks.noclobber_error {
                    Some(raw) => Err((io::Error::from_raw_os_error(raw), tmp)),
                    None => tmp
                        .persist_noclobber(full)
                        .map(drop)
                        .map_err(|e| (e.error, e.file)),
                };
                match refused {
                    Ok(()) => {}
                    Err((e, _)) if e.kind() == io::ErrorKind::AlreadyExists => {
                        return Err(StoreError::Changed(path.clone()));
                    }
                    // A filesystem that cannot do a no-clobber rename at all
                    // (some network and VM shares support neither the
                    // rename flag nor hard links) falls back to checking
                    // for absence and renaming: the window a replace has,
                    // rather than failing every creation there. Any other
                    // error is reported as it is.
                    Err((e, tmp)) if noclobber_unsupported(&e) => {
                        if file_identity(full).is_some() {
                            return Err(StoreError::Changed(path.clone()));
                        }
                        tmp.persist(full)
                            .map_err(|e| io_to_store_error(e.error, path))?;
                    }
                    Err((e, _)) => return Err(io_to_store_error(e, path)),
                }
            }
            // A replace: compare the content, then make sure the file is
            // still the one compared by its identity (inode, size, times),
            // which one `stat` answers. What is left is the gap between that
            // `stat` and the rename; no rename can be conditional on what
            // it replaces, so the gap cannot be closed here.
            Some(expected) => {
                let identity = file_identity(full);
                if observe_full(full, path)? != expected || file_identity(full) != identity {
                    // Dropping `tmp` deletes the temp file.
                    return Err(StoreError::Changed(path.clone()));
                }
                tmp.persist(full)
                    .map_err(|e| io_to_store_error(e.error, path))?;
            }
        }
        // Make the rename durable. Best-effort: a filesystem that
        // rejects a directory fsync (rare) shouldn't fail an otherwise
        // successful write, and atomicity already held regardless.
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

/// What is at `full` right now, read straight from disk (not recorded in
/// any read journal).
fn observe_full(full: &Path, path: &VaultPath) -> Result<Observed, StoreError> {
    match fs::read(full) {
        Ok(bytes) => Ok(Observed::of_bytes(&bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Observed::Absent),
        Err(e) => Err(io_to_store_error(e, path)),
    }
}

/// Which file is at `full`, as one `stat` tells it: a writer that replaces
/// the file by rename gives it a new inode, and one that edits it in place
/// changes its size or times. `None` when nothing (readable) is there.
fn file_identity(full: &Path) -> Option<(u64, u64, u64, i64, i64, i64, i64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::symlink_metadata(full).ok()?;
        Some((
            m.dev(),
            m.ino(),
            m.size(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        ))
    }
    #[cfg(not(unix))]
    {
        let m = fs::symlink_metadata(full).ok()?;
        let nanos = |t: io::Result<SystemTime>| {
            t.ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i64)
        };
        Some((0, 0, m.len(), nanos(m.modified()), 0, nanos(m.created()), 0))
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
        let result =
            fs::read_to_string(self.resolve(path)?).map_err(|e| io_to_store_error(e, path));
        self.journals.record(path, &result);
        result
    }

    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        let result = fs::read(self.resolve(path)?).map_err(|e| io_to_store_error(e, path));
        self.journals.record(path, &result);
        result
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

    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        // Checked once, right before the rename. The transaction, the one
        // caller, has just compared the file while taking its undo snapshot,
        // so an up-front check here would only repeat that read.
        let full = self.resolve(path)?;
        self.atomic_write_expecting(&full, content, path, Some(expected))
    }

    fn append_to_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        let full = self.resolve(path)?;
        let existing = match fs::read_to_string(&full) {
            Ok(content) => Some(content),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_to_store_error(e, path)),
        };
        let seen = existing
            .as_deref()
            .map_or(Observed::Absent, |c| Observed::of_bytes(c.as_bytes()));
        if seen != expected {
            return Err(StoreError::Changed(path.clone()));
        }
        let combined = format!("{}{content}", existing.unwrap_or_default());
        self.atomic_write_expecting(&full, &combined, path, Some(expected))
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

    fn remove_dir_if_empty(&self, dir: &VaultPath) -> Result<bool, StoreError> {
        let full = self.resolve(dir)?;
        // symlink_metadata, not metadata: a symlink to a directory is
        // not a directory here, and it must never be followed.
        let meta = match fs::symlink_metadata(&full) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io_to_store_error(e, dir)),
        };
        if !meta.is_dir() {
            return Ok(false);
        }
        let mut entries = fs::read_dir(&full).map_err(|e| io_to_store_error(e, dir))?;
        match entries.next() {
            None => {}
            Some(Ok(_)) => return Ok(false),
            Some(Err(e)) => return Err(io_to_store_error(e, dir)),
        }
        match fs::remove_dir(&full) {
            Ok(()) => Ok(true),
            // Something arrived between the emptiness check and the
            // removal: leave it. The kernel refuses the removal itself,
            // so nothing is lost.
            Err(e)
                if e.kind() == io::ErrorKind::NotFound
                    || e.kind() == io::ErrorKind::DirectoryNotEmpty =>
            {
                Ok(false)
            }
            Err(e) => Err(io_to_store_error(e, dir)),
        }
    }

    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        let exists = self.resolve(path)?.exists();
        if !exists {
            self.journals.record_absent(path);
        }
        Ok(exists)
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

    fn walk_dirs(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        let full = self.resolve(path)?;
        // symlink_metadata: a symlink to a directory is not one here.
        match fs::symlink_metadata(&full) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Ok(Vec::new()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_to_store_error(e, path)),
        }
        // Iterative DFS, as `walk_dir`, yielding the directories instead.
        // `DirEntry::file_type` does not follow symlinks.
        let mut stack: Vec<PathBuf> = vec![full];
        let mut out: Vec<VaultPath> = Vec::new();
        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir).map_err(|e| io_to_store_error(e, path))?;
            for entry in entries {
                let entry = entry.map_err(|e| io_to_store_error(e, path))?;
                let file_type = entry.file_type().map_err(|e| io_to_store_error(e, path))?;
                if !file_type.is_dir() {
                    continue;
                }
                let entry_path = entry.path();
                let rel = entry_path
                    .strip_prefix(&self.root)
                    .expect("entry is under vault root by construction");
                out.push(
                    VaultPath::new(rel)
                        .expect("rel is relative and free of traversal by construction"),
                );
                stack.push(entry_path);
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

    fn read_journals(&self) -> Option<&ReadJournals> {
        Some(&self.journals)
    }

    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        // The lock file lives in `.cuaderno/` — present in any real vault,
        // but create it defensively so a freshly-`init`ed vault locks too.
        let lock_path = match &self.lock_file {
            Some(lock_file) => lock_file.clone(),
            None => {
                let dir = self.root.join(crate::paths::CUADERNO_DIR);
                fs::create_dir_all(&dir).map_err(|e| StoreError::Io {
                    path: crate::paths::CUADERNO_DIR.to_string(),
                    source: e,
                })?;
                dir.join(".lock")
            }
        };
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
