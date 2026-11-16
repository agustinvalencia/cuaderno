//! The vault write lock every kernel sees (#1012).
//!
//! The `flock` on `.cuaderno/.lock` is enforced by one kernel. A vault
//! folder reachable from two (a host and a container that bind-mounts it
//! through a VM file share, two machines on a network share) has two
//! writers that both hold "the" `flock`, and their read-modify-writes of
//! one note can lose a line even with the compare before rename of #1001.
//! This module adds a lock both sides see: a lock *file*,
//! `.cuaderno/.write-lock`, whose creation is exclusive on the filesystem
//! that holds it, however many kernels reach it.
//!
//! `FsVaultStore`'s
//! [`acquire_write_lock`](crate::store::VaultStore::acquire_write_lock)
//! takes it after the `flock`, so only one process per kernel ever polls
//! for it, and holds it for as long as the `flock`: the life of a
//! transaction, or of any other holder of the vault write lock.
//! `docs/concurrent-writers.md` has the design and the reasoning; in short:
//!
//! - **Create.** The owner record is written to a temp file beside the
//!   lock, which is then hard-linked to the lock's name. A link refuses an
//!   existing name, on the host, so exactly one contender's link succeeds.
//!   On a network filesystem a lost reply to a link that was made returns
//!   an error; a link count of 2 on the temp file says it was made. A
//!   filesystem without hard links gets the no-clobber rename instead, and
//!   one without that either an exclusive create (which leaves the lock
//!   briefly empty: a reader then sees no owner, nothing worse).
//! - **Wait.** A held lock is polled with doubling, jittered pauses inside
//!   the vault lock's timeout, then reported as busy with its owner and
//!   age ([`StoreError::LockBusy`]).
//! - **Age.** The lock's mtime is compared with that of a probe file made
//!   now in the same folder, so both times come from one clock (the
//!   filesystem's). The probe is made only when the lock is found held, and
//!   once per lock seen; later polls add the time waited since.
//! - **Break.** A lock at least [`STALE_AFTER`] old is taken for a crashed
//!   holder's. It is renamed to a name unique to the contender; of two
//!   contenders that judged it stale, only one rename finds it. The winner
//!   checks that what it moved is the lock it judged (a younger lock may
//!   have replaced it in between: that one is put back) and then contends
//!   afresh.
//! - **Ownership.** Age cannot tell a crashed holder from a suspended one,
//!   which wakes after its lock was broken. So a holder checks that the
//!   file at the lock's name is still its own (one open, `fstat` and read:
//!   the same inode and the same token in its record) before every file
//!   operation of a commit, and refuses to go on if not
//!   ([`StoreError::LockLost`]). Release removes the lock only if it is
//!   still the holder's own.
//!
//! No heartbeat and no `fsync`: the lock is a coordination file, not data,
//! and a crash at worst leaves it to be broken after [`STALE_AFTER`].

use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::error::StoreError;

/// The lock's file name inside `.cuaderno/`.
pub const LOCK_NAME: &str = ".write-lock";

/// The lock's vault-relative path, as messages and ignore rules name it.
pub const LOCK_PATH: &str = ".cuaderno/.write-lock";

/// The start of the name of every file this protocol makes in `.cuaderno/`:
/// the lock itself, the temp file a new lock's record is written to
/// (`.write-lock.<random>.new`), the probe that reads the folder's clock
/// (`.write-lock.<token>.probe`), and the name a broken lock is moved to
/// (`.write-lock.<token>.stale`). Ignore rules cover them all with
/// `.write-lock*`. All but the lock live for microseconds; one left by a
/// crash is removed when a later contender breaks a stale lock.
pub const LOCK_PREFIX: &str = ".write-lock";

/// The age at which a lock is taken for a crashed holder's and broken.
///
/// Far above the longest legitimate hold (a rename or a method migration
/// rewriting many notes, a git checkpoint, the locked pass of an index
/// rebuild: seconds, see `docs/concurrent-writers.md`), and above any clock
/// skew between two synced machines. The cost of the margin: a holder that
/// crashed makes writes fail as busy until its lock is this old, and then
/// the next writer breaks it with nobody's help. A holder that does run
/// past it is not trusted to: its next file operation refuses once its
/// lock has been broken.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// The first pause between polls of a held lock; each pause doubles up to
/// [`LONGEST_PAUSE`], and each is jittered down to half its length.
const FIRST_PAUSE: Duration = Duration::from_millis(2);
const LONGEST_PAUSE: Duration = Duration::from_millis(50);

/// How much of a lock's owner record a contender reads.
const RECORD_LIMIT: u64 = 4096;

/// Points at which a test can step into the protocol (`test-support`).
#[derive(Clone, Default)]
pub(crate) struct Hooks {
    /// Runs when a contender has judged a lock stale, right before it
    /// renames it aside.
    pub(crate) before_break: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("before_break", &self.before_break.is_some())
            .finish()
    }
}

/// Which lock is at the lock's name: its inode (on Unix) and the token in
/// its record. Either alone could mislead: an inode number is reused once
/// the file is gone (locally the holder prevents that by keeping its lock
/// open, but an NFSv3 server knows nothing of a client's open file), and a
/// record is empty for the moment an exclusive create leaves it so.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    inode: Option<(u64, u64)>,
    token: String,
}

fn identity_of(meta: &fs::Metadata, record: &str) -> Identity {
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    };
    #[cfg(not(unix))]
    let inode = {
        let _ = meta;
        None
    };
    Identity {
        inode,
        token: record_field(record, "token").unwrap_or_default().to_owned(),
    }
}

/// A lock found at the lock's name.
struct Seen {
    id: Identity,
    mtime: SystemTime,
    record: String,
}

/// The vault write lock file, held. Released when dropped.
#[derive(Debug)]
pub(crate) struct SharedLock {
    dir: PathBuf,
    id: Identity,
    /// The lock file, kept open while held: the inode cannot be freed, so
    /// its number cannot be given to a successor's lock and fool the
    /// ownership check.
    _file: File,
    broke_stale: bool,
}

impl SharedLock {
    /// Whether the lock at the lock's name is still this one. A lock that
    /// was broken as stale, and possibly replaced by a successor's, is
    /// [`StoreError::LockLost`]. The open revalidates a network client's
    /// cached view of the name, which a plain `stat` may not.
    pub(crate) fn verify(&self) -> Result<(), StoreError> {
        match look(&self.dir.join(LOCK_NAME)) {
            Ok(Some(seen)) if seen.id == self.id => Ok(()),
            Ok(_) => Err(StoreError::LockLost {
                lock: LOCK_PATH.to_owned(),
            }),
            Err(e) => Err(io_error(e)),
        }
    }

    /// Whether taking this lock broke a stale one.
    pub(crate) fn broke_stale(&self) -> bool {
        self.broke_stale
    }
}

impl Drop for SharedLock {
    fn drop(&mut self) {
        // Only this holder's own lock is removed: one broken while held may
        // be a successor's by now, and is left to it.
        match self.verify() {
            Ok(()) => {
                if let Err(e) = fs::remove_file(self.dir.join(LOCK_NAME)) {
                    tracing::warn!(
                        lock = LOCK_PATH,
                        error = %e,
                        "could not remove the vault write lock on release; the next writer breaks \
                         it once it is stale"
                    );
                }
            }
            Err(StoreError::LockLost { .. }) => tracing::warn!(
                lock = LOCK_PATH,
                "the vault write lock was broken as stale while this process held it; left as it \
                 is for its new holder"
            ),
            Err(e) => tracing::warn!(
                lock = LOCK_PATH,
                error = %e,
                "could not check the vault write lock on release; left in place"
            ),
        }
    }
}

/// How [`acquire`] behaves; the defaults are the production values.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) stale_after: Duration,
    pub(crate) hooks: Hooks,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stale_after: STALE_AFTER,
            hooks: Hooks::default(),
        }
    }
}

/// Take the lock in `dir` (the vault's `.cuaderno/`), waiting for a holder
/// until `deadline` and breaking a stale lock on the way.
pub(crate) fn acquire(
    dir: &Path,
    deadline: Instant,
    waited_from: Instant,
    options: &Options,
) -> Result<SharedLock, StoreError> {
    let token = new_token();
    let record = owner_record(&token);
    let lock = dir.join(LOCK_NAME);
    let mut pause = FIRST_PAUSE;
    let mut broke_stale = false;
    // The lock last seen, its age when first seen, and when that was: a
    // lock is probed for its age once, and its age then advanced by the
    // time waited since.
    let mut known: Option<(Identity, Duration, Instant)> = None;
    // Attempts to create made after the deadline: a lock that vanished, or
    // was just broken, still gets one, but a name released and retaken
    // faster than this process can create it cannot keep it here for ever.
    let mut late_attempts = 0;
    loop {
        if Instant::now() >= deadline {
            late_attempts += 1;
            if late_attempts > 2 {
                return Err(StoreError::LockTimeout(
                    deadline.duration_since(waited_from),
                ));
            }
        }
        if let Some((file, id)) = create(dir, &lock, &record).map_err(io_error)? {
            return Ok(SharedLock {
                dir: dir.to_path_buf(),
                id,
                _file: file,
                broke_stale,
            });
        }
        // Held: wait for it to go (then contend again at once), or to turn
        // stale.
        while let Some(seen) = look(&lock).map_err(io_error)? {
            let age = match &known {
                Some((id, age, at)) if *id == seen.id => *age + at.elapsed(),
                _ => {
                    let age = age_of(dir, &token, seen.mtime).map_err(io_error)?;
                    known = Some((seen.id.clone(), age, Instant::now()));
                    age
                }
            };
            if age >= options.stale_after {
                if let Some(hook) = &options.hooks.before_break {
                    hook();
                }
                if break_stale(dir, &token, &seen).map_err(io_error)? {
                    broke_stale = true;
                    tracing::warn!(
                        lock = LOCK_PATH,
                        owner = %describe_owner(&seen.record),
                        age_secs = age.as_secs(),
                        "broke a stale vault write lock, taken for a crashed writer's"
                    );
                }
                known = None;
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(StoreError::LockBusy {
                    lock: LOCK_PATH.to_owned(),
                    owner: describe_owner(&seen.record),
                    age_secs: age.as_secs(),
                    stale_after_secs: options.stale_after.as_secs(),
                    waited: now.duration_since(waited_from),
                });
            }
            std::thread::sleep(jitter(pause).min(deadline - now));
            pause = (pause * 2).min(LONGEST_PAUSE);
        }
    }
}

/// Try to create the lock at `lock` with `record` inside. `None` when a
/// lock is already there.
fn create(dir: &Path, lock: &Path, record: &str) -> io::Result<Option<(File, Identity)>> {
    let mut temp = tempfile::Builder::new()
        .prefix(&format!("{LOCK_PREFIX}."))
        .suffix(".new")
        .tempfile_in(dir)?;
    temp.write_all(record.as_bytes())?;
    match fs::hard_link(temp.path(), lock) {
        Ok(()) => return Ok(Some(held(temp.into_file(), record)?)),
        Err(e) => {
            // A network filesystem can lose the reply to a link it made: a
            // retransmitted link then fails (with "exists", or otherwise)
            // although the first one succeeded. The temp file's link count
            // says which.
            if link_count(temp.path()) >= 2 {
                return Ok(Some(held(temp.into_file(), record)?));
            }
            if e.kind() == io::ErrorKind::AlreadyExists {
                return Ok(None);
            }
            if !crate::store::noclobber_unsupported(&e) {
                return Err(e);
            }
        }
    }
    // No hard links on this filesystem: a rename that refuses to replace.
    match temp.persist_noclobber(lock) {
        Ok(file) => Ok(Some(held(file, record)?)),
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) if crate::store::noclobber_unsupported(&e.error) => {
            drop(e.file);
            // Neither: an exclusive create, then the record. A contender
            // reading the lock in between finds no owner named, and ages
            // it by its mtime all the same.
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(lock)
            {
                Ok(mut file) => {
                    file.write_all(record.as_bytes())?;
                    Ok(Some(held(file, record)?))
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e.error),
    }
}

/// The held lock's open file and identity. (`NamedTempFile::into_file`
/// has removed the temp file's own name by now, leaving the lock's.)
fn held(file: File, record: &str) -> io::Result<(File, Identity)> {
    let meta = file.metadata()?;
    let id = identity_of(&meta, record);
    Ok((file, id))
}

#[cfg(unix)]
fn link_count(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).map_or(0, |m| m.nlink())
}

#[cfg(not(unix))]
fn link_count(_path: &Path) -> u64 {
    0
}

/// The lock at `path`, if any, opened afresh (an open revalidates a network
/// client's cached attributes, which a plain `stat` may answer from its
/// cache), with its `fstat` and its record.
fn look(path: &Path) -> io::Result<Option<Seen>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    let mut bytes = Vec::new();
    file.take(RECORD_LIMIT).read_to_end(&mut bytes)?;
    let record = String::from_utf8_lossy(&bytes).into_owned();
    Ok(Some(Seen {
        id: identity_of(&meta, &record),
        mtime: meta.modified()?,
        record,
    }))
}

/// How old a lock whose mtime is `mtime` is, by the folder's own clock: a
/// probe file is created beside it now, and the two mtimes compared. A
/// lock stamped ahead of the probe is new.
fn age_of(dir: &Path, token: &str, mtime: SystemTime) -> io::Result<Duration> {
    let probe = dir.join(format!("{LOCK_PREFIX}.{token}.probe"));
    let file = match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&probe)?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)?
        }
        Err(e) => return Err(e),
    };
    let now = file.metadata().and_then(|m| m.modified());
    drop(file);
    let _ = fs::remove_file(&probe);
    Ok(now?.duration_since(mtime).unwrap_or(Duration::ZERO))
}

/// Break the stale lock `judged`: rename it to a name of this contender's
/// own, which only one contender's rename can do, and check that what moved
/// is the lock judged. A younger lock that replaced it in the meantime is
/// put back. True when this contender broke it.
fn break_stale(dir: &Path, token: &str, judged: &Seen) -> io::Result<bool> {
    let lock = dir.join(LOCK_NAME);
    let aside = dir.join(format!("{LOCK_PREFIX}.{token}.stale"));
    match fs::rename(&lock, &aside) {
        Ok(()) => {}
        // Another contender broke it first.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    }
    match look(&aside)? {
        Some(moved) if moved.id == judged.id => {
            let _ = fs::remove_file(&aside);
            remove_leftovers(dir, judged.mtime);
            Ok(true)
        }
        Some(_) => {
            // A live lock, made after the one judged was broken by someone
            // else: put it back. If a third lock has taken the name in the
            // meantime, the one moved has lost its hold, and its holder
            // refuses at its next check.
            match fs::hard_link(&aside, &lock) {
                Ok(()) => {}
                Err(e) if crate::store::noclobber_unsupported(&e) && look(&lock)?.is_none() => {
                    fs::rename(&aside, &lock)?;
                    return Ok(false);
                }
                Err(_) => {}
            }
            let _ = fs::remove_file(&aside);
            Ok(false)
        }
        None => Ok(false),
    }
}

/// Remove the protocol's files left by crashes: every `.write-lock.*` file
/// no newer than a lock that was just found stale. A live process's temp,
/// probe or aside file exists for microseconds, so none is that old.
fn remove_leftovers(dir: &Path, stale_mtime: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{LOCK_PREFIX}.");
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t <= stale_mtime);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The owner record written into a lock: who took it, for a person reading
/// the busy error. Nothing in it is trusted for a decision except the
/// token, which identifies the lock where inodes do not.
fn owner_record(token: &str) -> String {
    let program = std::env::args_os()
        .next()
        .and_then(|a| {
            Path::new(&a)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "cdno".to_owned());
    format!(
        "cuaderno vault write lock\ntoken: {token}\nprogram: {program} {version}\nprocess: {pid}\n\
         taken: {taken}\n",
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        taken = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z"),
    )
}

fn record_field<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    record.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(": "))
            .map(str::trim)
    })
}

/// The owner of a lock, in words, from its record.
fn describe_owner(record: &str) -> String {
    match (
        record_field(record, "program"),
        record_field(record, "process"),
        record_field(record, "taken"),
    ) {
        (Some(program), Some(process), Some(taken)) => format!(
            "{program} (process {process} on the machine that took it, at {taken} by that \
             machine's clock)"
        ),
        _ => "a writer that has not written its owner record yet".to_owned(),
    }
}

/// A token unique to one acquisition, from the standard library's
/// per-process random hash keys, the process id, the time and a counter.
fn new_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut token = String::with_capacity(32);
    for half in 0u8..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u8(half);
        h.write_u64(count);
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        token.push_str(&format!("{:016x}", h.finish()));
    }
    token
}

/// `pause` shortened by a random part of up to half, so two processes
/// that collided do not poll in step.
fn jitter(pause: Duration) -> Duration {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    let fraction = (h.finish() % 1000) as u32;
    pause - pause / 2 * fraction / 1000
}

fn io_error(source: io::Error) -> StoreError {
    StoreError::Io {
        path: LOCK_PATH.to_owned(),
        source,
    }
}
