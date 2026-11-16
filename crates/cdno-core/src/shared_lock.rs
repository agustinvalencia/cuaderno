//! The vault write lock every kernel sees (#1012).
//!
//! The `flock` on `.cuaderno/.lock` is enforced by one kernel. A vault
//! folder reachable from two (a host and a container that bind-mounts it
//! through a VM file share, two machines on a network share) has two
//! writers that both hold "the" `flock`, and their read-modify-writes of
//! one note can lose a line even with the compare before rename of #1001.
//! This module adds a lock both sides see: a lock *file*,
//! `.cuaderno/locks/write.lock`, whose creation is exclusive on the
//! filesystem that holds it, however many kernels reach it. Every file of
//! the protocol lives in `.cuaderno/locks/`, so one rule excludes them all.
//!
//! `FsVaultStore`'s
//! [`acquire_write_lock`](crate::store::VaultStore::acquire_write_lock)
//! takes it after the `flock`, so only one process per kernel ever polls
//! for it, and holds it for as long as the `flock`: the life of a
//! transaction, or of the git checkpoint. Holders that write only this
//! machine's index take the `flock` alone
//! ([`acquire_index_lock`](crate::store::VaultStore::acquire_index_lock)).
//! `docs/concurrent-writers.md` has the design and the reasoning; in short:
//!
//! - **Create.** The owner record is written to a temp file in the locks
//!   folder, which is then hard-linked to the lock's name. A link refuses
//!   an existing name, on the filesystem that holds the folder, so exactly
//!   one contender's link succeeds. On a network filesystem a lost reply to
//!   a link that was made returns an error; a link count of 2 on the temp
//!   file, or the contender's own token found at the lock's name, says it
//!   was made. A filesystem without hard links gets the no-clobber rename
//!   instead, and one without that either an exclusive create (which leaves
//!   the lock briefly empty: a reader then sees no owner, and does not
//!   break it on that look).
//! - **Wait.** The contender keeps its one temp file and polls by trying
//!   the link again, with doubling, jittered pauses of at most
//!   [`LONGEST_PAUSE`], inside the vault lock's timeout, then reports busy
//!   with the owner and age ([`StoreError::LockBusy`]).
//! - **Age.** The lock's mtime is compared with that of a probe file made
//!   now in the same folder. That is one clock only where the filesystem,
//!   not the writing kernel, stamps both. The probe is made only when the
//!   lock is found held, and once per lock seen.
//! - **Break.** A lock at least [`STALE_AFTER`] old, or one whose record
//!   names the very `flock` this process holds (this kernel's boot and the
//!   same device and inode of the `flock` file) and another process
//!   instance, or this one for a lock it no longer holds (so its holder is
//!   dead), is renamed to a name unique to the contender, after one more
//!   look that it is still the lock judged and still as old; of two
//!   contenders, only one rename finds it. The winner checks what it moved
//!   is the lock it judged (a younger lock may have replaced it in between:
//!   that one is put back) and contends afresh. A record of a later
//!   [`PROTOCOL`] is broken by age only.
//! - **Ownership.** A holder checks, before every file operation of a
//!   commit and again just before each rename, unlink or copy into place,
//!   that the file at the lock's
//!   name carries its token (one open and read; a name found missing is
//!   looked at once more after a pause, since a contender may have moved it
//!   aside by mistake and be putting it back). It refuses to go on if not.
//!   The same check refreshes the lock's mtime, with the filesystem's
//!   clock, once it is a few seconds old, so a long commit is not taken for
//!   a crashed one. Release removes the lock only if it is still the
//!   holder's own.
//!
//! No heartbeat thread and no `fsync`: the lock is a coordination file, not
//! data, and a crash at worst leaves it to be broken after [`STALE_AFTER`]
//! (at once on the same kernel).

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use crate::error::StoreError;

/// The folder, inside the vault, that holds every file of the protocol.
pub const LOCKS_DIR: &str = ".cuaderno/locks";

/// The lock's file name inside [`LOCKS_DIR`].
pub const LOCK_NAME: &str = "write.lock";

/// The lock's vault-relative path, as messages name it.
pub const LOCK_PATH: &str = ".cuaderno/locks/write.lock";

/// The age at which a lock is taken for a crashed holder's and broken.
///
/// A live holder refreshes its lock at every file operation once it is
/// [`REFRESH_AFTER`] old, so only a holder that stops (crashed, suspended,
/// or stuck in one step for this long) reaches it. The cost of the margin:
/// a holder that crashed on another kernel makes writes fail as busy until
/// its lock is this old, and then the next writer breaks it with nobody's
/// help.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// How old a holder lets its lock get, by its own monotonic clock, before
/// its next ownership check refreshes the lock's mtime.
pub const REFRESH_AFTER: Duration = Duration::from_secs(5);

/// The first pause between attempts on a held lock; each pause doubles up
/// to [`LONGEST_PAUSE`], and each is jittered down to half its length.
const FIRST_PAUSE: Duration = Duration::from_millis(1);
/// The longest pause between two attempts: short, so a side that waits is
/// not starved by one that takes the lock back to back.
pub const LONGEST_PAUSE: Duration = Duration::from_millis(10);

/// How often a waiting contender reads the lock it is waiting on (for its
/// age and owner); the attempts in between are one `link` each.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// The pause after the lock's name was found missing by its holder, before
/// the one more look that decides it was lost; longer than a contender's
/// rename aside, open and link back on a slow share.
const RELOOK_PAUSE: Duration = Duration::from_millis(50);

/// The pause when the link said the name is taken and a look then found it
/// free: no spin while a client's cache disagrees with itself.
const VANISHED_PAUSE: Duration = Duration::from_millis(2);

/// How much of a lock's owner record is read.
const RECORD_LIMIT: u64 = 4096;

/// The version of these rules a record declares (`protocol: 1`). A reader
/// that meets a higher one does not take the fast path for a dead holder on
/// this kernel (it cannot know what a later release writes there), breaks
/// by age only, and says so in the busy message.
pub const PROTOCOL: u32 = 1;

/// The tokens of the locks this process holds right now. A record that
/// names this process instance and a token not in this set was left by
/// this process (a release that failed) and is dead; one whose token is in
/// the set is live, which matters where `flock` is emulated per process
/// (Linux NFS) and two threads hold it at once.
static HELD_TOKENS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn held_tokens() -> std::sync::MutexGuard<'static, Vec<String>> {
    HELD_TOKENS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A value drawn once per process, written into its records. Unlike the
/// process id it is never reused: a server restarted in a container is
/// often process 1 again, and must still see its predecessor as another.
fn process_instance() -> &'static str {
    static INSTANCE: OnceLock<String> = OnceLock::new();
    INSTANCE.get_or_init(new_token)
}

/// Why a lock was broken, for the warning and for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakReason {
    /// It was at least [`STALE_AFTER`] old.
    Age,
    /// Its record names this kernel's boot, the very `flock` file this
    /// process holds (same device and inode), and another process instance
    /// (or this one, for a lock it no longer holds): a live holder would
    /// still hold that `flock`.
    DeadOnThisKernel,
}

/// Points at which a test can step into the protocol (`test-support`).
#[derive(Clone, Default)]
pub(crate) struct Hooks {
    /// Runs when a contender has judged a lock breakable and looked once
    /// more, right before it renames it aside.
    pub(crate) before_break: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Runs when a contender has moved aside a lock that is not the one it
    /// judged, right before it puts it back.
    pub(crate) after_wrong_move: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Runs when a holder's ownership check finds the lock's name missing,
    /// before it pauses and looks once more.
    pub(crate) on_name_missing: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("before_break", &self.before_break.is_some())
            .field("after_wrong_move", &self.after_wrong_move.is_some())
            .field("on_name_missing", &self.on_name_missing.is_some())
            .finish()
    }
}

/// How [`acquire`] behaves; the defaults are the production values.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) stale_after: Duration,
    pub(crate) refresh_after: Duration,
    /// This kernel's boot, as written into a record; `None` where it is
    /// not known, which turns the same-kernel break off.
    pub(crate) boot: Option<String>,
    pub(crate) hooks: Hooks,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stale_after: STALE_AFTER,
            refresh_after: REFRESH_AFTER,
            boot: kernel_boot_id(),
            hooks: Hooks::default(),
        }
    }
}

/// Which lock is at the lock's name: the token in its record and, as a
/// tie-break for a record not yet written, its inode. The token decides:
/// inode numbers are per name on some filesystems (high-level FUSE), are
/// reused at once on others (FAT), and change on others when a file is
/// first written.
#[derive(Debug, Clone)]
struct Identity {
    token: String,
    inode: Option<(u64, u64)>,
}

impl Identity {
    fn same(&self, other: &Identity) -> bool {
        if self.token.is_empty() || other.token.is_empty() {
            self.token == other.token && self.inode.is_some() && self.inode == other.inode
        } else {
            self.token == other.token
        }
    }
}

/// A lock found at a name.
struct Seen {
    id: Identity,
    mtime: SystemTime,
    record: String,
}

impl Seen {
    fn field(&self, key: &str) -> Option<&str> {
        record_field(&self.record, key)
    }
}

/// The state of a held lock, shared with the commit's late check.
#[derive(Debug)]
struct Held {
    dir: PathBuf,
    token: String,
    /// The lock file, kept open while held: its link count says whether it
    /// still has a name at all, and its descriptor refreshes its mtime.
    file: File,
    /// Whether the descriptor's link count means anything here (it was at
    /// least 1 when the lock was taken).
    nlink_known: bool,
    refresh_after: Duration,
    refreshed: Mutex<Instant>,
    hooks: Hooks,
}

impl Held {
    /// Whether the lock is still this holder's, refreshing it when due.
    fn check(&self) -> io::Result<bool> {
        if self.nlink_known && descriptor_link_count(&self.file) == Some(0) {
            return Ok(false);
        }
        let lock = self.dir.join(LOCK_NAME);
        let mut seen = look(&lock)?;
        if seen.is_none() {
            // Maybe moved aside by a contender that judged another lock
            // and is putting this one back: look once more.
            if let Some(hook) = &self.hooks.on_name_missing {
                hook();
            }
            std::thread::sleep(RELOOK_PAUSE);
            seen = look(&lock)?;
        }
        let mine = seen.is_some_and(|s| s.id.token == self.token);
        if mine {
            self.refresh_if_due();
        }
        Ok(mine)
    }

    fn refresh_if_due(&self) {
        let mut refreshed = self.refreshed.lock().unwrap_or_else(|e| e.into_inner());
        if refreshed.elapsed() < self.refresh_after {
            return;
        }
        match touch_now(&self.file) {
            Ok(()) => *refreshed = Instant::now(),
            Err(e) => tracing::warn!(
                lock = LOCK_PATH,
                error = %e,
                "could not refresh the vault write lock; a long write may be taken for a \
                 crashed one"
            ),
        }
    }
}

/// The vault write lock file, held. Released when dropped.
#[derive(Debug)]
pub(crate) struct SharedLock {
    held: Arc<Held>,
    broke: Option<BreakReason>,
}

impl SharedLock {
    /// Whether the lock is still this one: `Ok(false)` when it was broken
    /// (and possibly replaced by a successor's). Refreshes it when due.
    pub(crate) fn still_held(&self) -> Result<bool, StoreError> {
        let held = self.held.check().map_err(io_error)?;
        if !held {
            tracing::warn!(
                lock = LOCK_PATH,
                "this process's vault write lock was broken by another writer while it held it"
            );
        }
        Ok(held)
    }

    /// Refresh the lock if it is due, which checks ownership; nothing
    /// otherwise.
    pub(crate) fn keep_alive(&self) {
        let due = self
            .held
            .refreshed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
            >= self.held.refresh_after;
        if due {
            let _ = self.held.check();
        }
    }

    /// Why taking this lock broke another, if it did.
    pub(crate) fn broke(&self) -> Option<BreakReason> {
        self.broke
    }

    /// Make the store's late check (just before each rename) see this lock
    /// until the returned scope drops. One commit thread at a time.
    pub(crate) fn in_commit(&self) -> CommitScope {
        let outer = COMMIT_HOLD.with(|h| h.borrow_mut().replace(Arc::clone(&self.held)));
        CommitScope(outer)
    }
}

impl Drop for SharedLock {
    fn drop(&mut self) {
        // Only this holder's own lock is removed: one broken while held may
        // be a successor's by now, and is left to it.
        let checked = self.held.check();
        held_tokens().retain(|t| *t != self.held.token);
        match checked {
            Ok(true) => {
                if let Err(e) = fs::remove_file(self.held.dir.join(LOCK_NAME)) {
                    tracing::warn!(
                        lock = LOCK_PATH,
                        error = %e,
                        "could not remove the vault write lock on release; the next writer breaks \
                         it once it is stale"
                    );
                }
            }
            Ok(false) => tracing::warn!(
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

thread_local! {
    /// The lock of the commit running on this thread, for the store's
    /// check just before a rename.
    static COMMIT_HOLD: RefCell<Option<Arc<Held>>> = const { RefCell::new(None) };
}

/// Restores this thread's previous commit lock (normally none) when
/// dropped.
pub(crate) struct CommitScope(Option<Arc<Held>>);

impl Drop for CommitScope {
    fn drop(&mut self) {
        let outer = self.0.take();
        COMMIT_HOLD.with(|h| *h.borrow_mut() = outer);
    }
}

/// The store's check, as late as it can be made (right before a rename,
/// an unlink or a copy into place): [`StoreError::LockLost`] when a commit
/// runs on this thread and its lock is no longer its own. Passes when no
/// commit with a lock file runs on this thread, so a store that applies an
/// op on another thread skips it (fails open to the commit's own check
/// before the op).
pub(crate) fn commit_still_holds() -> Result<(), StoreError> {
    let held = COMMIT_HOLD.with(|h| h.borrow().clone());
    match held {
        Some(held) => match held.check().map_err(io_error)? {
            true => Ok(()),
            false => Err(StoreError::LockLost {
                lock: LOCK_PATH.to_owned(),
            }),
        },
        None => Ok(()),
    }
}

/// Take the lock in `dir` (the vault's [`LOCKS_DIR`]), waiting for a holder
/// until `deadline` and breaking a stale or dead one on the way. The caller
/// holds this kernel's `flock`.
///
/// `flock_id` is the device and inode of the `flock` file the caller holds,
/// as its descriptor reports them: written into the record, and compared
/// with a dead holder's before the fast path (two processes on one kernel
/// hold "the" `flock` at once if they reach the file through two mounts
/// whose locks do not meet, or if it was replaced under a holder).
pub(crate) fn acquire(
    dir: &Path,
    deadline: Instant,
    waited_from: Instant,
    options: &Options,
    flock_id: Option<&str>,
) -> Result<SharedLock, StoreError> {
    let token = new_token();
    let record = owner_record(&token, options.boot.as_deref(), flock_id);
    let lock = dir.join(LOCK_NAME);
    let mut attempt = Attempt::new(dir, &record).map_err(io_error)?;
    let mut pause = FIRST_PAUSE;
    let mut broke = None;
    // The lock last seen, and its age then and when, so it is probed once.
    let mut known: Option<(Identity, Duration, Instant)> = None;
    let mut last: Option<(String, u64)> = None;
    let mut looked_at: Option<Instant> = None;
    loop {
        if let Some(file) = attempt.try_place(&lock).map_err(io_error)? {
            return Ok(holding(dir, &token, file, broke, options));
        }
        let due = looked_at.is_none_or(|at| at.elapsed() >= LOOK_EVERY);
        if due || Instant::now() >= deadline {
            looked_at = Some(Instant::now());
            match look(&lock).map_err(io_error)? {
                None => {
                    // Released since, or a cache that disagrees with itself.
                    // A lock seen next is a new one, whatever its inode.
                    known = None;
                    std::thread::sleep(VANISHED_PAUSE);
                    looked_at = None;
                    if Instant::now() < deadline {
                        continue;
                    }
                }
                Some(seen) if seen.id.token == token => {
                    // A link whose reply was lost, seen through a fresher open
                    // than the link count's `stat`: it is ours.
                    let file = attempt.take_file().map_err(io_error)?;
                    return Ok(holding(dir, &token, file, broke, options));
                }
                Some(seen) => {
                    let age = match &known {
                        Some((id, age, at)) if id.same(&seen.id) => *age + at.elapsed(),
                        _ => {
                            let age = age_of(dir, &token, seen.mtime).map_err(io_error)?;
                            known = Some((seen.id.clone(), age, Instant::now()));
                            age
                        }
                    };
                    let reason = if dead_on_this_kernel(&seen, options, flock_id) {
                        Some(BreakReason::DeadOnThisKernel)
                    } else if age >= options.stale_after && !seen.id.token.is_empty() {
                        Some(BreakReason::Age)
                    } else if age >= options.stale_after + options.stale_after {
                        // A record still empty after twice the interval: an
                        // exclusive create whose writer died before writing.
                        Some(BreakReason::Age)
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        let sweep = reason == BreakReason::Age;
                        if break_lock(dir, &token, &seen, sweep, &options.hooks)
                            .map_err(io_error)?
                        {
                            broke = Some(reason);
                            tracing::warn!(
                                lock = LOCK_PATH,
                                owner = %describe_owner(&seen.record),
                                age_secs = age.as_secs(),
                                reason = match reason {
                                    BreakReason::Age => "stale",
                                    BreakReason::DeadOnThisKernel =>
                                        "its holder on this kernel is dead",
                                },
                                "broke the vault write lock of another writer"
                            );
                        }
                        known = None;
                        looked_at = None;
                        continue;
                    }
                    last = Some((describe_owner(&seen.record), age.as_secs()));
                }
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(match last {
                Some((owner, age_secs)) => StoreError::LockBusy {
                    lock: LOCK_PATH.to_owned(),
                    owner,
                    age_secs,
                    stale_after_secs: options.stale_after.as_secs(),
                    waited: now.duration_since(waited_from),
                },
                None => StoreError::LockTimeout(now.duration_since(waited_from)),
            });
        }
        std::thread::sleep(jitter(pause).min(deadline - now));
        pause = (pause * 2).min(LONGEST_PAUSE);
    }
}

fn holding(
    dir: &Path,
    token: &str,
    file: File,
    broke: Option<BreakReason>,
    options: &Options,
) -> SharedLock {
    let nlink_known = descriptor_link_count(&file).is_some_and(|n| n >= 1);
    if nlink_known {
        NLINK_TRUSTED.store(true, Ordering::Relaxed);
    }
    held_tokens().push(token.to_owned());
    SharedLock {
        held: Arc::new(Held {
            dir: dir.to_path_buf(),
            token: token.to_owned(),
            file,
            nlink_known,
            refresh_after: options.refresh_after,
            refreshed: Mutex::new(Instant::now()),
            hooks: options.hooks.clone(),
        }),
        broke,
    }
}

/// Whether `seen` was taken through the very `flock` the caller holds (the
/// same kernel boot, and the same device and inode of the `flock` file),
/// which a live holder would still hold: so its holder is dead. A record of
/// another process instance qualifies; one of this process qualifies only
/// when its token is not among the locks this process holds (a release that
/// failed), since a filesystem that emulates `flock` per process (Linux
/// NFS) lets two threads of one process hold it at once. A record of a
/// later protocol never qualifies.
fn dead_on_this_kernel(seen: &Seen, options: &Options, flock_id: Option<&str>) -> bool {
    let (Some(boot), Some(flock_id)) = (options.boot.as_deref(), flock_id) else {
        return false;
    };
    if record_protocol(&seen.record) > PROTOCOL
        || seen.field("boot") != Some(boot)
        || seen.field("flock") != Some(flock_id)
    {
        return false;
    }
    match seen.field("instance") {
        Some(instance) if instance == process_instance() => !held_tokens().contains(&seen.id.token),
        Some(_) => true,
        None => false,
    }
}

/// The protocol a record declares; 1 for one that declares none.
fn record_protocol(record: &str) -> u32 {
    record_field(record, "protocol")
        .and_then(|p| p.parse().ok())
        .unwrap_or(1)
}

/// Set once any lock taken by this process reports a link count of at
/// least 1 on its descriptor: this filesystem's link counts mean something,
/// so the store may read a count of 0 on an opened note as "replaced".
static NLINK_TRUSTED: AtomicBool = AtomicBool::new(false);

/// Whether a link count of 0 on an open file means it has no name left here.
pub(crate) fn link_counts_trusted() -> bool {
    NLINK_TRUSTED.load(Ordering::Relaxed)
}

/// How a contender places its record at the lock's name, by the best
/// primitive the filesystem has. Kept for the whole wait, so each attempt
/// is one call.
struct Attempt {
    dir: PathBuf,
    record: String,
    temp: Option<tempfile::NamedTempFile>,
    tier: Tier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Link,
    Rename,
    Exclusive,
}

impl Attempt {
    fn new(dir: &Path, record: &str) -> io::Result<Self> {
        let mut attempt = Self {
            dir: dir.to_path_buf(),
            record: record.to_owned(),
            temp: None,
            tier: Tier::Link,
        };
        attempt.temp = Some(attempt.write_temp()?);
        Ok(attempt)
    }

    fn write_temp(&self) -> io::Result<tempfile::NamedTempFile> {
        let mut temp = tempfile::Builder::new()
            .prefix("")
            .suffix(".new")
            .tempfile_in(&self.dir)?;
        temp.write_all(self.record.as_bytes())?;
        Ok(temp)
    }

    /// One attempt: the lock's file when placed, `None` when the name is
    /// taken.
    fn try_place(&mut self, lock: &Path) -> io::Result<Option<File>> {
        loop {
            match self.tier {
                Tier::Link => {
                    let temp = match &self.temp {
                        Some(temp) => temp,
                        None => {
                            self.temp = Some(self.write_temp()?);
                            self.temp.as_ref().expect("just set")
                        }
                    };
                    match fs::hard_link(temp.path(), lock) {
                        Ok(()) => return self.placed().map(Some),
                        Err(e) => {
                            // A lost reply to a link that was made.
                            if link_count(temp.path()) >= 2 {
                                return self.placed().map(Some);
                            }
                            match e.kind() {
                                io::ErrorKind::AlreadyExists => return Ok(None),
                                // The temp file went (a leftover sweep under
                                // a skewed clock): write it again, retry.
                                io::ErrorKind::NotFound => {
                                    self.temp = None;
                                    return Ok(None);
                                }
                                _ => {}
                            }
                            // `EPERM` from linking a file this process owns
                            // is the filesystem refusing links.
                            if !crate::store::noclobber_unsupported(&e) {
                                return Err(e);
                            }
                            self.tier = Tier::Rename;
                        }
                    }
                }
                Tier::Rename => {
                    let temp = match self.temp.take() {
                        Some(temp) => temp,
                        None => self.write_temp()?,
                    };
                    match temp.persist_noclobber(lock) {
                        Ok(file) => {
                            announce(Tier::Rename);
                            return Ok(Some(file));
                        }
                        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {
                            self.temp = Some(e.file);
                            return Ok(None);
                        }
                        // Not `EPERM` here: a rename in a folder this process
                        // writes to has no "unsupported" reading of it.
                        Err(e)
                            if crate::store::noclobber_unsupported(&e.error)
                                && e.error.raw_os_error() != Some(eperm()) =>
                        {
                            drop(e.file);
                            self.tier = Tier::Exclusive;
                        }
                        Err(e) => return Err(e.error),
                    }
                }
                Tier::Exclusive => {
                    // An exclusive create, then the record: a contender
                    // reading the lock in between finds no owner and does
                    // not break it on that look.
                    return match OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(lock)
                    {
                        Ok(mut file) => {
                            file.write_all(self.record.as_bytes())?;
                            announce(Tier::Exclusive);
                            Ok(Some(file))
                        }
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                        Err(e) => Err(e),
                    };
                }
            }
        }
    }

    /// The link was made: the lock's file, the temp name removed.
    fn placed(&mut self) -> io::Result<File> {
        announce(Tier::Link);
        self.take_file()
    }

    /// The temp file's handle, its own name removed (the lock's stays).
    fn take_file(&mut self) -> io::Result<File> {
        match self.temp.take() {
            Some(temp) => Ok(temp.into_file()),
            None => File::open(self.dir.join(LOCK_NAME)),
        }
    }
}

/// One debug line per process for each way a lock was created, so a run
/// shows which primitive the filesystem offered.
fn announce(tier: Tier) {
    static SEEN: [AtomicBool; 3] = [
        AtomicBool::new(false),
        AtomicBool::new(false),
        AtomicBool::new(false),
    ];
    let (slot, how) = match tier {
        Tier::Link => (0, "a hard link"),
        Tier::Rename => (1, "a no-clobber rename (this filesystem has no hard links)"),
        Tier::Exclusive => (
            2,
            "an exclusive create (this filesystem has neither hard links nor a no-clobber rename)",
        ),
    };
    if !SEEN[slot].swap(true, Ordering::Relaxed) {
        tracing::debug!(
            lock = LOCK_PATH,
            "the vault write lock file is created by {how}"
        );
    }
}

fn eperm() -> i32 {
    #[cfg(unix)]
    {
        libc::EPERM
    }
    #[cfg(not(unix))]
    {
        1
    }
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

#[cfg(unix)]
fn descriptor_link_count(file: &File) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    file.metadata().ok().map(|m| m.nlink())
}

#[cfg(not(unix))]
fn descriptor_link_count(_file: &File) -> Option<u64> {
    None
}

/// Set the file's times to now by the filesystem's clock (`UTIME_NOW`,
/// which an NFS client sends as "the server's time"), never this process's.
/// Both times, not the mtime alone: setting both to now needs only write
/// access, any other combination ownership, which a share may not present.
#[cfg(unix)]
fn touch_now(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let times = [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        },
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        },
    ];
    // SAFETY: a valid open descriptor and a two-element array, as
    // `futimens` requires.
    if unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn touch_now(_file: &File) -> io::Result<()> {
    Ok(())
}

/// The lock at `path`, if any, opened afresh (an open revalidates a network
/// client's cached attributes, which a plain `stat` may answer from its
/// cache), with its record. A stale handle is retried once.
fn look(path: &Path) -> io::Result<Option<Seen>> {
    match look_once(path) {
        Err(e) if is_stale_handle(&e) => look_once(path),
        other => other,
    }
}

fn look_once(path: &Path) -> io::Result<Option<Seen>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    let mut bytes = Vec::new();
    file.take(RECORD_LIMIT).read_to_end(&mut bytes)?;
    let record = String::from_utf8_lossy(&bytes).into_owned();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    };
    #[cfg(not(unix))]
    let inode = None;
    Ok(Some(Seen {
        id: Identity {
            token: record_field(&record, "token")
                .unwrap_or_default()
                .to_owned(),
            inode,
        },
        mtime: meta.modified()?,
        record,
    }))
}

fn is_stale_handle(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::ESTALE)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

/// How old a lock whose mtime is `mtime` is, by the folder's clock: a probe
/// file is created beside it now, and the two mtimes compared. A lock
/// stamped ahead of the probe is new.
fn age_of(dir: &Path, token: &str, mtime: SystemTime) -> io::Result<Duration> {
    let probe = dir.join(format!("{token}.probe"));
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

/// Break the lock `judged`: look once more that it is still at the name,
/// rename it to a name of this contender's own, which only one contender's
/// rename can do, and check that what moved is the lock judged. A younger
/// lock that replaced it in the meantime is put back. True when this
/// contender broke it.
fn break_lock(
    dir: &Path,
    token: &str,
    judged: &Seen,
    sweep: bool,
    hooks: &Hooks,
) -> io::Result<bool> {
    let lock = dir.join(LOCK_NAME);
    match look(&lock)? {
        // Still the lock judged, and still as old: a refresh since means
        // its holder is alive.
        Some(now) if now.id.same(&judged.id) && now.mtime == judged.mtime => {}
        // Released, broken by another, refreshed or replaced: nothing to
        // break.
        _ => return Ok(false),
    }
    if let Some(hook) = &hooks.before_break {
        hook();
    }
    let aside = dir.join(format!("{token}.stale"));
    match fs::rename(&lock, &aside) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    }
    match look(&aside)? {
        Some(moved) if moved.id.same(&judged.id) && moved.mtime == judged.mtime => {
            let _ = fs::remove_file(&aside);
            if sweep {
                remove_leftovers(dir, judged.mtime);
            }
            Ok(true)
        }
        Some(_) => {
            if let Some(hook) = &hooks.after_wrong_move {
                hook();
            }
            put_back(&aside, &lock);
            Ok(false)
        }
        None => Ok(false),
    }
}

/// Put a live lock moved aside by mistake back at the lock's name, never
/// over a lock that took the name meanwhile: that one wins, and the moved
/// lock's holder finds its lock gone at its next check and refuses.
fn put_back(aside: &Path, lock: &Path) {
    match fs::hard_link(aside, lock) {
        Ok(()) => {
            let _ = fs::remove_file(aside);
            return;
        }
        Err(e) if !crate::store::noclobber_unsupported(&e) => {
            let _ = fs::remove_file(aside);
            return;
        }
        Err(_) => {}
    }
    // No hard links: a rename that refuses to replace, if there is one.
    let moved = match tempfile::TempPath::try_from_path(aside) {
        Ok(moved) => moved,
        Err(_) => {
            let _ = fs::remove_file(aside);
            return;
        }
    };
    if let Err(e) = moved.persist_noclobber(lock) {
        tracing::warn!(
            lock = LOCK_PATH,
            error = %e.error,
            "moved a live vault write lock aside by mistake and could not put it back; its \
             holder's next write stops"
        );
        // Dropping the returned path removes the moved lock.
    }
}

/// Remove the protocol's files left by crashes: every `*.new`, `*.probe`
/// and `*.stale` in the locks folder that is no newer than a lock just
/// found breakable. A live process's temp, probe or aside file is younger.
/// Nothing else in the folder is ever touched: every release runs this, so
/// it is the folder's contract with every later one (its `.gitignore`, a
/// second lock).
fn remove_leftovers(dir: &Path, stale_mtime: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ours = [".new", ".probe", ".stale"]
            .iter()
            .any(|suffix| name.len() > suffix.len() && name.ends_with(suffix));
        if !ours {
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
/// the busy error, and the kernel's boot for the same-kernel break. The
/// token identifies the lock.
fn owner_record(token: &str, boot: Option<&str>, flock_id: Option<&str>) -> String {
    let program = std::env::args_os()
        .next()
        .and_then(|a| {
            Path::new(&a)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "cdno".to_owned());
    let boot = boot.map(|b| format!("boot: {b}\n")).unwrap_or_default();
    let flock = flock_id
        .map(|f| format!("flock: {f}\n"))
        .unwrap_or_default();
    format!(
        "cuaderno vault write lock\nprotocol: {PROTOCOL}\ntoken: {token}\n\
         program: {program} {version}\nprocess: {pid}\ninstance: {instance}\n{boot}{flock}\
         taken: {taken}\n",
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        instance = process_instance(),
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
        (Some(program), Some(process), Some(taken)) => {
            let protocol = record_protocol(record);
            let newer = if protocol > PROTOCOL {
                format!(
                    "; it follows version {protocol} of the lock's rules, newer than this \
                     cdno's {PROTOCOL}, so it is taken over only once it is stale, never sooner"
                )
            } else {
                String::new()
            };
            format!(
                "{program} (process {process} on the machine that took it, at {taken} by that \
                 machine's clock{newer})"
            )
        }
        _ => "a writer that has not written its owner record yet".to_owned(),
    }
}

/// This kernel's boot, as an opaque identifier that names nobody: shared by
/// every process and container of one kernel (the `flock`'s reach), and
/// different on another kernel. `None` where the platform does not say.
pub(crate) fn kernel_boot_id() -> Option<String> {
    static BOOT: OnceLock<Option<String>> = OnceLock::new();
    BOOT.get_or_init(read_boot_id).clone()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_boot_id() -> Option<String> {
    let id = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

#[cfg(target_vendor = "apple")]
fn read_boot_id() -> Option<String> {
    let name = c"kern.bootsessionuuid";
    let mut buf = [0u8; 128];
    let mut len: libc::size_t = buf.len();
    // SAFETY: a NUL-terminated name, a buffer and its length, no new value.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let id = String::from_utf8_lossy(&buf[..len.min(buf.len())]);
    let id = id.trim_end_matches('\0').trim();
    (!id.is_empty()).then(|| id.to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn read_boot_id() -> Option<String> {
    None
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
