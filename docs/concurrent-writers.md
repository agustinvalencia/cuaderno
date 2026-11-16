# Writers the vault lock does not exclude

Design note for #1001 and #1012 (part of the epic on vaults shared between two kernels, #1005). The
first half says what a commit checks so that a second writer's change is not silently discarded,
how that check reaches every write, how much it helps, and which windows remain open: it narrows
the loss and does not remove it. The second half, [The lock every kernel sees](#the-lock-every-kernel-sees-1012),
is the lock file that excludes cdno writers from each other on every kernel, leaving one window to a
holder stopped between its last check and its rename; the comparison stays for writers that take no
lock.

## The problem

Every domain write is read, modify, write-by-rename. Writers serialise on an advisory `flock` on
`.cuaderno/.lock`, held for the life of a `VaultTransaction` (#196). `flock` is enforced by one
kernel. When one vault folder is written from two kernels at once (a host and a container that
bind-mounts it through a VM file share, or a network share that does not forward locks), both sides
take "the" lock at the same moment. Two read-modify-writes then interleave, and the second rename
replaces the first writer's file with content composed before that writer's change. Each file stays
well formed, so nothing looks wrong afterwards.

A program that does not take the lock at all (an editor, a sync tool writing the working tree) is
the same case seen from another side.

## How much it helps

Measured with a CLI on a host and a server in a container that bind-mounts the vault over a VM file
share, both appending log lines to one daily note. The harness issues each call about a tenth of a
second apart per side, so contention is moderate. The container's index was kept separate (#998).

| Build | Appends attempted | Lines on disk | Lost |
|---|---|---|---|
| Last release, two runs of 80 + 80 | 320 | 153 and 152 of 160 | 7 and 8 (about 5%) |
| This change, four runs of 150 + 150 | 1,200 | 1,197 | 1 acknowledged and missing; 2 refused with the conflict error and correctly absent |

So the check takes the loss from about 5% to under 0.1% in that configuration, and does not reach
zero. The line that was lost was acknowledged to its writer: window 1 below. The two refused
appends had used up the retry budget then in place (four attempts, a 5 to 20 ms pause), which is
why it is now five attempts with pauses up to about a quarter of a second.

What was measured is the branch before its final rework of the retry and of the journal (the
thread rule, the disagreeing-read rule, `create_file`); the rename-side check, which decides window
1, is unchanged since.

**Why a one-machine figure is not representative.** Two writers in a tight loop on one Mac lose 0 to
2 lines of 300 with the check and about 150 without it (the ignored test
`on_a_real_folder_only_the_gap_before_the_rename_remains`). That low figure is an artefact: `sync_all`
on macOS is a full device flush (`F_FULLFSYNC`), which serialises the two writers into strict
alternation and acts as an accidental shared lock. The same protocol reproduced standalone (two
processes, 150 appends each, on APFS) lost 0 to 1 of 300 with the full flush, 38 to 52 of 300 with a
plain `fsync`, and nearly every contended line with a 300 µs pause between the last check and the
rename; with no check at all it lost 147 to 149. What decides the rate is the width of the gap
between the last check and the rename against the rest of the cycle, and across a VM file share
every step is a round trip through the share. A guest that answers the re-read or the `stat` from
its own cache would also compare stale data and pass.

## The design: compare before rename

A commit knows what the operation read of every file it is about to replace, append to or delete,
re-checks the file on disk just before writing it, and on a mismatch writes nothing and fails with a
distinct error.

### Where "what I read" lives: a read journal the store fills

Domain operations read through `Vault::store` directly and stage writes on the transaction; the two
do not meet. Changing the dozens of verbs to read through the transaction would be invasive, and a
verb that forgot would silently skip the check. So the store records instead:

- `VaultTransaction::new` takes the lock and then opens a `ReadJournal` on the store
  (`cdno-core/src/read_journal.rs`), owned by the calling thread. While it is open, `FsVaultStore`
  and `MemoryVaultStore` record every observation that thread makes: `read_file` and `read_bytes`
  record the content's xxh3 fingerprint and length, a `NotFound` records "absent", and an `exists`
  that answers false records "absent". An `exists` that answers true records nothing (it saw no
  content).
- **Only the opening thread's reads count.** A domain operation runs on one thread from opening its
  transaction to committing it (the MCP server runs each call on one blocking thread), while the
  store is shared by every thread of the process. A read on another thread, a read-only tool call
  beside the write, may have seen a newer or an older version than the operation did, so it never
  supplies the baseline.
- **The first observation is the baseline, and every later one must agree.** A second read of the
  operation that disagrees with the first marks the path as changed during the operation, and the
  commit refuses it, whatever is on disk by then. So a commit that passes replaces a file that
  holds, at its last check, exactly the bytes every read of the operation returned, which is the
  content it composed from. A file changed and changed back to those bytes in between is
  indistinguishable from untouched, and harmless.
- **A create says so.** `VaultTransaction::create_file` stages a write that expects the path absent
  whatever was observed, so a path chosen as free can never turn into a replace.
- **A path staged unread** (a blind overwrite) is observed when it is staged, and if that fails, at
  the start of the commit. No staged path goes unchecked. But that baseline is later than the
  operation's read, so the domain does not rely on it: its transactions declare `require_reads`, and
  staging a replace, append or delete of an existing file that was not read inside the transaction
  fails a debug assertion. The domain suite exercises every verb, so a verb that reads before it
  opens its transaction fails its tests; a release build keeps the staging-time comparison and
  logs a warning naming the path, so a server's log shows the broken rule.
- **No store opts out silently.** `read_journals`, `write_file_expecting` and
  `append_to_file_expecting` have no default on `VaultStore`: each store states whether it records
  reads, and a wrapper delegates (as the lock-holding store `reconcile::rebuild` uses does). A store
  that returns `None` from `read_journals` gets staging-time baselines and no debug assertion.

The journal closes when the commit starts, so the commit's own reads are never mistaken for the
operation's. Readers that never open a transaction (`Journal`, `Catalog`, every read tool) record
nothing; with no journal open, recording is one uncontended mutex and an empty scan.

Rejected: the transaction recording reads made through a new `tx.read_file` (every verb would have
to opt in, and one that forgot would skip the check); a versioned read returned by the store (same
problem, and it changes every read call site); size and mtime instead of a content fingerprint
(coarse on some shared filesystems, and a same-size rewrite within one tick is invisible).

### What the commit checks

1. **Before any file op** (`precheck` in `transaction.rs`), every path whose first touch in the
   commit is a write, an append, a delete, or a move whose source the operation read, is compared
   with its baseline. Every mismatch is collected, and the commit returns
   `TransactionError::Conflict { paths }` having written nothing. Later ops on a path the commit
   already touched are compared with what the earlier op left, not with the read.
2. **As each op applies**, the read that takes its undo snapshot is compared too (no extra read).
3. **In the store, right before the rename** (`write_file_expecting`, `append_to_file_expecting`):
   `FsVaultStore` writes and syncs the temp file, then reads the target again, compares it, checks
   with one `stat` that the target is still the same file (inode, size, times), and only then
   renames; a test hook placed after the sync pins that order. A file the commit expects to be
   absent is created with a no-clobber rename (`persist_noclobber`), which the kernel refuses if
   anything is there, so a create has no window. A filesystem that cannot do a no-clobber rename at
   all (the error says the operation is unsupported: no rename flag and no hard links) falls back to
   checking for absence and renaming, with the same gap as a replace; any other failure is reported.
   `MemoryVaultStore` compares and writes under one mutex.

A mismatch found in step 2 or 3 rolls back what was applied, in reverse order. The rollback is
itself conditional: each undo carries what its op left on disk, and a path that no longer holds that
(another writer changed it after this commit wrote it) is left alone and reported as a rollback
failure rather than overwritten. A clean rollback is reported as `Conflict`; one that could not
undo everything is `FileWrite { source: Changed, rollback_failures }`, because the vault may then be
part-written.

**Bare moves are compared only when their source was read.** A move the operation did not read the
source of (an attachment following its folder in a rename) carries the file along whatever it
holds, so it discards nothing, and comparing would mean reading the attachment's bytes, which a move
never did (a rename test pins that). Notes are moved by writing the new path and deleting the old
one, and both are compared. What a compared move expected travels to its destination, so a later
write there is compared too.

### What happens on a mismatch

The transaction cannot re-apply a change: that means re-running the domain operation that composed
it. Two layers:

- **Every verb** fails with the typed error: `TransactionError::Conflict`, wrapped as
  `DomainError::Transaction`, "… changed on disk while this write was being prepared, most likely by
  another writer; nothing was written, run the operation again". `DomainError::is_concurrent_change`
  names it.
- **The append-shaped verbs retry** (`Vault::retry_on_conflict`, `vault/conflict_retry.rs`, whose
  module doc holds the list the other docs follow): five attempts in all, the first retry after
  about 5 ms and the last after about 250 ms (about 0.4 s of pauses in all), before the conflict is
  returned. Each retried conflict is logged at debug with its path and attempt, and a final one at
  warn. A refused commit wrote nothing, every verb writes in one transaction, and the comparison is
  against the transaction's own reads, so a retry is exactly as safe as the verb's no-conflict path.
  Nearly every mutating verb appends a history line to today's daily note, so that note is where
  conflicts arise whichever verb is called. Retried: `log_to_daily_note`, `note_to_daily`,
  `upsert_daily_section` with `append`, `capture_to_inbox`, `discard_inbox_item`, the action verbs
  (`add`, `start`, `start_unplanned`, `switch`, `switch_unplanned`, `pause`, `resume`, `complete`,
  `drop`), the milestone verbs (`add`, `complete`, `drop`), the waiting-on verbs (`add`,
  `resolve`), and `add_tracking_entry`.
- **Not retried, and why.** The verbs that replace content a caller may have composed from what it
  read earlier (`upsert_*_section` without `append`, `update_project_state`, `set_core_question`,
  `set_frontmatter`, `revise_note`), and `rename_note`, which writes a plan the caller may have
  approved by its hash. Retrying them would be as safe as their no-conflict path, but a conflict
  says the note is being written right now, which is the moment to let the caller look again.
  Whether the caller's own earlier read is stale is a different question that this does not answer
  (#563). The create and lifecycle verbs (`create_*`, `promote_action`, `hand_off_project`, parking,
  activating and closing projects, commitments, questions, portfolio filing, migration) do not
  retry yet either; they are as safe to retry and are left out only to keep this change reviewable.


The interfaces surface the error so a caller can act on it:

- **MCP**: a tool result with code `concurrent_change` and `details.paths`, the one rolled-back
  transaction that is a rejection rather than a protocol error, because the caller's remedy is to
  call again. Its message says to read the file first if the content was composed from an earlier
  read, and to tell the person if it is refused again.
- **CLI**: exit status **75** (`EX_TEMPFAIL`), with the usual `Error: …` report, or under `--json`
  the same `{code, message, details}` object on stdout.

## The windows that remain

1. **Between the last `stat` and the rename of a replaced file.** No rename can be made conditional
   on what it replaces (an exchange-and-verify with `RENAME_EXCHANGE` / `RENAME_SWAP` was
   considered: it briefly publishes the conflicting content, which a third read can build on, and
   its support across VM file shares is unknown). A writer that renames inside this gap is still
   overwritten. This is the line lost in the two-kernel measurement above.
2. **Between the comparison and the unlink of a deleted file, or the rename of a moved one.** A
   delete compares the file it read and then removes it, and a move whose source was read compares
   it and then renames it away; a change landing between the two is removed or carried off with it.
3. **A move's destination** is checked for absence and then renamed onto (`FsVaultStore::move_file`,
   older than this change): a file another writer creates there in between is replaced.
4. **A caller's earlier read** is no baseline (#563, unchanged by this change). An agent that read a
   note in one call and replaces a section of it in the next gets a baseline taken milliseconds
   before the commit, so a third party's change in between is overwritten without an error.
   `concurrent_change` is not a stale-read check. The pieces #563 needs exist now: `Observed` is the
   token and `write_file_expecting` the conditional write.
5. **A verb that composes written content from the index** rather than from a store read would be
   compared only from staging; none is known to.
6. **Writers that do not use this protocol** (an editor, a sync tool) are protected against only as
   far as the comparison sees their change before the rename; nothing makes them check ours.
7. **An imported attachment's rollback** deletes whatever is at its destination, unconditionally:
   its bytes are never read. Imports land at fresh names.
8. **The raw writes** that bypass the transaction and the lock (`write_note_raw`, template eject and
   save, `save_config_raw`) bypass this check too. They were left as they are and must not be
   widened.
9. **The index.** The check is about the Markdown; a long-lived process's index can still lag
   another kernel's writes until it reconciles (#1002), and SQLite's own locking is not shared across
   kernels (#998).

One assumption is worth stating: on one local filesystem, `flock` keeps a process to one open
transaction at a time. On a Linux NFS client `flock` is emulated with POSIX record locks owned by the
process, so two threads of one process can hold transactions at once there. The journals are per
transaction and per thread and both commits compare, so this is the cross-kernel case again, not a
new hole.

**The durable fix** is a lock both kernels honour: an exclusive-create lock file beside the `flock`,
with a stale-break rule (#1012, option 2 of #1001), described below. With it, cdno writers exclude
each other everywhere, and windows 1 to 3 above are open to writers that take no lock, against which
this comparison remains the only protection, and to a cdno holder whose lock was broken while it was
stopped in its last step. The two are complements.

## Interactions

- **The index and `IndexStale`.** A conflicted commit returns before any index op, so the index is
  untouched. A commit whose files land behaves as before: an index failure after them is still
  `IndexStale`, and over MCP the two travel separately (the conflict as a rejection, a stale index in
  its own envelope).
- **Index repair.** `reconcile::rebuild` writes the index in place outside any transaction, and its
  locked pass runs index-only transactions through a store wrapper that delegates the comparison
  methods; they stage no file op, so there is nothing to compare.
- **The method-version gate** reads `.cuaderno/config.toml` after the lock is taken, so that read is
  recorded like any other; a commit that rewrites the config is compared against what the gate read.
- **Rename** rewrites many files in one commit; a conflict on any of them fails the whole rename
  with nothing written, and the new folder a failed rename might leave is pruned as after a failed
  write.
- **The append-only journal.** A refused commit adds nothing to and removes nothing from a daily
  note; the retry appends to the note as it now is, so the other writer's lines and this one's both
  stand.
- **Cost.** Per replaced file the commit adds one read (the precheck) and, in `FsVaultStore`, one
  more read and two `stat`s before the rename, plus xxh3 over the content (gigabytes per second).
  Measured with `log_to_daily_note` on a real folder (debug build, macOS): about 12.6 ms per commit
  before and 13.3 ms after on a small note, 14.6 ms and 16.2 ms on a note padded to 200 KB. The
  syncs dominate either way.

## Tests

- `cdno-core/tests/unit/unshared_lock_tests.rs`. Two stores over one folder with independent lock
  files (`FsVaultStore::with_lock_file`, behind the `test-support` feature that only the crate's own
  dev-dependency enables) interleave read-modify-writes by hand:
  - the second commit is refused and the first is intact; creates, deletes, appends and blind writes;
  - multi-file atomicity; a change landing mid-commit rolls back; a rollback does not overwrite a
    later change;
  - the no-clobber create at the store, including a file created after the temp file is synced;
  - the comparison point, after the sync;
  - the fallback for filesystems without a no-clobber rename, and that other failures are reported;
  - `create_file`, the compared move, and that another thread's reads do not count;
  - a note changed and changed back between two reads;
  - the debug assertion of `require_reads`;
  - an exact two-thread race over a shared in-memory store.

  The real-folder race is `#[ignore]`d because its outcome depends on scheduling. Run it with
  `cargo test -p cdno-core --test unit -- --ignored on_a_real_folder --nocapture`.
- `cdno-domain/tests/unit/unshared_lock_tests.rs`:
  - two `Vault`s logging to one daily note lose no line;
  - a stale read is retried onto the fresh note, including a notes entry and a section append;
  - a log that never converges returns the conflict after exactly five attempts and writes nothing;
  - `add_action` from a stale read of its map is retried onto the fresh map;
  - a section replace from a stale read is refused, not retried;
  - a capture and a stewardship never replace one made at the same path meanwhile;
  - a discard logs the text it deletes.
- `cdno-mcp`: the `concurrent_change` code, payload and message (`rejection.rs`), and the server
  instructions sentence (`tests/server.rs`). `cdno-cli/tests/exit.rs`: exit status 75 and the
  `--json` object.

## The lock every kernel sees (#1012)

The comparison above leaves a gap of one `stat` before each rename, and across a VM file share
that gap lost one acknowledged line in 1,200. The issue's own reproduction (two processes, 150
appends each to one note, independent `flock`s) lost 0 to 52 of 300 with the comparison, depending
on the storage, and none with an exclusive-create lock around a plain read-modify-rename. So cdno
writers now also take a lock that a second kernel sees: a lock **file**,
`.cuaderno/locks/write.lock`, whose creation the filesystem holding it decides, however many kernels
reach it. The mechanics are `cdno-core/src/shared_lock.rs`; nothing outside `cdno-core` knows of
it.

**Layout.** Every file of the protocol lives in `.cuaderno/locks/`: the lock, the temp file a
contender writes its record to (`<random>.new`), the age probe (`<token>.probe`) and the name a
broken lock is moved to (`<token>.stale`). One rule (`/.cuaderno/locks/`) excludes them all in every
tool. The path is a wire protocol between releases: two versions that disagree on it do not exclude
each other, so it must not move without a period in which both are taken. The `flock` file stays at
`.cuaderno/.lock`. (A layout decision by the lead, flagged for the owner before merge.)

### Who takes it

`FsVaultStore::acquire_write_lock` takes the `flock` first, as before, then the lock file, within
the one 5 s deadline. The `flock` still orders the writers of one kernel cheaply, and it means only
one process per kernel ever polls for the lock file. The guard releases the lock file first.

Holders that write notes take both: every `VaultTransaction::new` (every domain verb, the method
migration, the domain's creation and rename) and the HTTP server's git checkpoint. Holders that write
only this machine's index take the `flock` alone, through `VaultStore::acquire_index_lock`:
reconciliation's per-note transactions (`VaultTransaction::index_only`, on which staging a file op
panics in every build, so a file can never be written without the lock file), link re-resolution,
the snapshot backfill, an index rebuild's locked pass, the bootstrap's empty-in-place, the server's
set-aside and reopen of index files and its snapshot carry, and `reindex`'s set-aside and rebind.
The lock file guards notes across kernels; each kernel's index is its own (#998), and the `flock`
orders one kernel's index writers. So a read command that reconciles a changed note never takes
the lock file, never waits on another kernel, and never keeps it waiting. (The earlier version of
this note said the lock kept a reconciliation pass from reading a note in the middle of another
writer's commit. It did not: `reconcile_one` reads the note before it opens its transaction.)

### The protocol

1. **Create.** The owner record (a random token, the program and its version, the process id, the
   kernel's boot identifier where the platform gives one, the time by the writer's clock; no host
   name) is written to a temp file in the locks folder, which is then hard-linked to `write.lock`.
   `link` refuses an existing name, so exactly one contender's link succeeds, and the lock never
   exists without its record. A lost reply to a link that was made (a network filesystem) shows as
   a link count of 2 on the temp file, or as the contender's own token at the lock's name, read
   through an `open`, which is fresher than the count's `stat`. A filesystem without hard links gets
   the no-clobber rename the store already uses, and one without that either an exclusive create
   and then the record (the design review found macOS exFAT and FAT refuse both `link` and the
   exclusive rename with `ENOTSUP`, so they land on this third tier). On that
   tier the lock is empty for a moment; a contender does not break a lock with an empty record
   until it is twice the stale interval old (a creator that died before writing). `EPERM` from linking a file the process itself just created is read as "this
   filesystem has no hard links"; from the rename it is reported. One debug line per process says
   which tier was used. The lock file is not synced.
2. **Wait.** The contender keeps its one temp file for the whole wait and polls by trying the link
   again: one call that tests and takes. Pauses start at 1 ms, double to 10 ms, and are jittered
   down to half. It reads the lock (for its age and owner) at most every 100 ms; where the link
   said "exists" and the read found nothing, it pauses 2 ms rather than spin. At the deadline it
   fails with `StoreError::LockBusy`, naming the lock, its owner and its age.
3. **Age.** The lock's mtime is compared with the mtime of a probe file created at that moment in
   the locks folder and removed at once. This is one clock **only if the filesystem, not the
   writing kernel, stamps both files**. A share whose client keeps mtimes for written files (a FUSE
   writeback cache, some NFS and SMB client modes) stamps the lock with the holder's clock and the
   probe with the server's, and the age is then off by the skew between them, which for a VM after
   the host has slept can be minutes. With the holder's clock behind by more than the stale
   interval, the other side breaks its every lock on sight and its writes fail with `LockLost`;
   ahead, a crash heals that much later. The probe is made only when the lock is found held, and
   once per lock seen.
4. **Break.** A lock is broken when it is at least 30 s old (`STALE_AFTER`), or at once when its
   record names this kernel's boot and another process: the contender holds this kernel's `flock`,
   which a live holder on this kernel would still hold, so that holder is dead (Ctrl-C on a CLI, a
   killed server, an out-of-memory kill). The boot identifier is Linux's `boot_id`, macOS's
   `kern.bootsessionuuid`; it names nobody, and a container on a Linux host shares the host's, as it
   shares its `flock`. Before breaking, the contender reads the lock once more and goes on only if
   it is still the one judged. It then renames it to `<token>.stale`, a name of its own: of two
   contenders, only one rename finds it. It opens what it moved and checks it is the lock it judged
   (by token, the inode as a tie-break for an empty record). If so, it removes it, logs a warning
   with the owner, the age and the reason, and (on a break by age) removes every other file in the
   locks folder no newer than the broken lock, which only a crash leaves. If not (a younger lock
   replaced the judged one between the second look and the rename), it puts the moved lock back
   with a link, never over a lock that took the name meanwhile; where there are no hard links, with
   a no-clobber rename, and where there is neither, it removes the moved lock, whose holder then
   stops at its next check.
5. **Ownership.** Before each file op of a commit, and again in the store after the op's temp file
   is written and synced, right before the comparison and the rename, the holder checks that the
   file at `write.lock` carries its token. It also counts its own descriptor's links: zero means the
   lock has no name left. A name found missing is read once more after 50 ms, since a contender
   that moved it aside by mistake is putting it back. If the lock is not its own, the commit stops,
   rolls back what it applied (conditionally, as every rollback), and fails with
   `TransactionError::LockLost`, whose message says either that nothing it wrote was kept or which
   files could not be undone. A clean `LockLost` is a "try again" condition like a changed file: the
   append-shaped verbs retry it, the CLI exits 75, MCP returns `concurrent_change` with
   `lock_lost: true`. One whose undo failed is reported as the machine's failure. A lock lost after
   the last file op landed skips the commit's index rows and reports `IndexStale`.
6. **Refresh.** The same check refreshes the lock's mtime once it is 5 s old by the holder's
   monotonic clock, with `futimens(UTIME_NOW)` through the holder's descriptor, which stamps the
   filesystem's time (an NFS client sends it as "the server's time"). So a long commit (a rename or
   a migration over thousands of notes) is never taken for a crashed one while it progresses. No
   thread: a holder that stops also stops refreshing, and is broken as before.
7. **Release.** On drop (after a commit, a refused commit, or a transaction never committed) the
   guard checks the lock is still its own the same way, and only then removes it. A lock broken
   while held is left for its new holder, with a warning. A stale file handle (`ESTALE`) on any read
   of the lock is retried once.

### What remains

The lock is a lease without fencing at the filesystem, and that leaves one window: **a holder
stopped between its last check and its rename** (suspended, or partitioned with the rename in
flight on a hard-mounted share) can still replace a note after a successor broke its lock, read
the note and wrote it. The successor's line is then lost. The window is now the store's comparison
and rename alone (one read, two `stat`s and the rename), and the successor's own comparison does not
help, since the old holder's rename comes after it. Nothing short of fencing at the server closes
it. In every other interleaving found, the outcome is a refused or delayed write.

The other residues:

- **The break race costs time, not safety.** A contender that moves a live lock aside by mistake
  puts it back; its holder, checking in that gap, looks again after 50 ms. A third writer that takes
  the name in the gap wins, and the moved lock's holder refuses at its next check.
- **Release's check-then-unlink** can remove a successor's lock if its own was broken in the
  microseconds between the two; that needs a holder that stopped refreshing for 30 s, and the
  successor then refuses at its next check.
- **The same-kernel break trusts the `flock`.** On a filesystem where `flock` succeeds without
  locking (some FUSE filesystems), two live writers on one kernel could each take the other's lock
  for a dead one's; each then refuses at its next check, so writes fail rather than collide. Where
  `flock` is emulated per process (Linux NFS), a record naming the contender's own process is never
  taken for dead. Two clones of one suspended VM, resumed side by side, share a boot identifier
  and would break each other's locks at once in the same way.
- **The guest's view of the name.** The ownership check opens the lock to get a fresh view. On a
  FUSE share (virtiofs) whether an open revalidates a cached name depends on the mount's entry and
  attribute timeouts; under `cache=always` a broken holder's check can keep finding its own
  unlinked file. The link count catches it when attributes are fresher than names. The two-kernel
  measurement must include a host breaking a guest holder, with the share's cache mode recorded.
- **An object store mounted as files** (where "exclusive" means exclusive within one mount) gives no
  exclusion at all; nothing can be done about that but to say so.

### Mixed versions

0.46.0 has neither the lock file nor the compare before rename. With one side on 0.46.0, lines are
still lost, silently, at the rate of the first row of the table above, for the lines the upgraded
side writes: its comparison protects the old side's lines, and nothing protects its own from the old
side. The lead measured one lost line with a lock-less server beside a locking CLI. **Upgrade every
side together.** No minimum-version marker is written: nothing could make an old binary read it.

### Synced folders

The lock serves a folder two kernels reach **at the same time** (a VM share, a network share). It
does nothing across copies that a sync tool (git, Syncthing, Dropbox, iCloud Drive) reconciles, and
there each machine's `flock` does all the work. A lock that travels by sync excludes nobody, since
each machine's link succeeds on its own disk; it can only cost: the other machine waits on it until
the delete arrives or it is 30 s old (a copy keeps its mtime, so 30 s from the original write; a tool
that stamps arrival time makes it 30 s from arrival), and a break there can sync back as a delete
onto a newer live lock, whose holder then refuses its write. No sequence blocks for ever and none
loses a line. So exclude the folder: `/.cuaderno/locks/` in `.gitignore`, `.cuaderno/locks` in
Syncthing's `.stignore`, the ignore attribute for Dropbox. A tool with no ignore rules (iCloud Drive)
costs occasional waits of up to 30 s on the other machine. Conflict copies a sync tool leaves in the
folder are removed by the next break by age if they are older than the broken lock.

### Settled points from the review of #1001

- **Held for the transaction**, from `VaultTransaction::new` (before the operation's first read) to
  the drop after `commit`.
- **No heartbeat thread**: the refresh rides on the ownership check, on the holder's own thread.
- **Always on** for every holder that writes notes: a process cannot tell whether a second kernel
  exists. The `test-support` seam `without_shared_lock` models an older cdno in tests.
- **The comparison stays.** It is the only protection against a writer that takes no lock (an
  editor, a sync tool, an older cdno), and it narrows the window left to a broken holder.
- **Raw writes** (`save_config_raw`, template eject and save, `templates sync`, `write_note_raw`)
  take neither lock, as before; this change does not cover them.
- **Readers never take it**, including a read command whose opening reconciliation indexes a changed
  note. `cdno now` and shell completion take no lock at all (their tests check `.cuaderno/` is not
  touched).

### Holders and how long they hold

| Holder | Lock | Holds across | Worst case, as far as known |
|---|---|---|---|
| Every domain verb (`VaultTransaction::new`) | both | its reads, its commit | milliseconds; about 10 ms per written file on a macOS disk, where each write is a full device flush |
| `rename_note`, the method migration | both | the whole-vault scan, one commit rewriting every touched note | about 10 ms per rewritten note: seconds for hundreds, tens of seconds for thousands, refreshed at every file op |
| The git checkpoint | both, only when `git status` (run first, unlocked) finds the tree dirty | `git add -A`, `git commit` | a second or two; it checks ownership before each step and skips the tick if the lock was lost |
| Reconciliation, link re-resolution, snapshot backfill, rebuild's locked pass, set-asides, reopen, `reindex`'s set-aside and rebind | `flock` only | index work | not seen by another kernel |

A long scan before a commit (a rename's read of the whole vault) is not refreshed, since no file op
runs during it: keep scans short. The checkpoint's `git` calls are not refreshed either; its checks
before staging and committing catch a lock lost meanwhile.

### What it costs

Measured in process on a macOS laptop SSD (APFS, debug build), the same binary with the lock file
on and off (`without_shared_lock`), alternating:

| | With | Without | Added |
|---|---|---|---|
| Lock and release alone, median of 300, three runs | 0.37 ms | 0.015 ms | 0.36 ms |
| A transaction appending a line to a note, median of 300, three runs | 12.5 ms | 9.5 to 9.6 ms | 2.9 to 3.0 ms |
| Opening a vault of 500 new notes into an empty index, median of 5 | 39.1 ms | 38.7 ms | none (reconciliation takes the `flock` alone) |

A transaction pays the lock and release, and two ownership checks per file op (one open and read
each). The larger part of the added time is the note's own full device flush, which now also
flushes the lock's metadata. Over a share each step is a round trip. Run them with
`cargo test -p cdno-core --test unit -- --ignored measure_what_the_lock_file --nocapture` and
`cargo test -p cdno-domain --test unit -- --ignored measure_what_the_lock_file --nocapture`.

**A waiter behind a side that takes the lock back to back.** The design review's emulation
(`fair.py`), re-run against the real code
(`cargo test -p cdno-core --test unit -- --ignored measure_how_long --nocapture`): one "kernel"
re-acquires in a loop with the given hold and gap, the other arrives 30 times at random moments and
waits with the production 5 s timeout. Same machine; over a share both scale with the round trip.

| Hold, gap | Waiter's median | p90 | max | Refused at 5 s | The review's emulation of the earlier code: median, refused |
|---|---|---|---|---|---|
| 0 ms, none | 0.5 ms | 6.9 ms | 8.2 ms | 0 of 30 | 0.4 ms, 0 |
| 1 ms, none | 6.0 ms | 47 ms | 222 ms | 0 of 30 | 118 ms, 0 |
| 5 ms, none | 54 ms | 595 ms | 810 ms | 0 of 30 | 0.9 s, 4 of 30 |
| 12 ms, none | 109 ms | 548 ms | 851 ms | 0 of 30 | 2.4 s, 6 of 30 |
| 12 ms, 1 ms | 16 ms | 178 ms | 462 ms | 0 of 30 | 39 ms, 0 |

There is no fairness protocol: a side that writes continuously can still delay the other, but the
earlier refusals are gone.

### Ignore rules

Reconciliation (through `classify`) and `cdno watch` skip `.cuaderno/` entirely, and `cdno lint`
reads the index and walks only `portfolios/`, so none of them sees the locks folder. The HTTP
server's git checkpoint adds `/.cuaderno/locks/` to `.git/info/exclude`, since it commits while
holding the lock. The repository's own `.gitignore` has the same rule, and the user guide's
`.gitignore` lines for a vault in git include it. `cdno init` writes no `.gitignore`, so an existing
vault in git needs the line added by hand (the changelog's upgrade step).

### Across two kernels

The lead's first run: a server in a container and the CLI on the host, one vault over the VM share,
400 appends a side paced by process start-up (about 25 writes a second a side): 0 acknowledged and
missing in about 4,000; with the server's index private, 800 of 800 and no refusals; the server's
write median 41 ms against 43 ms without the lock file; no lock files left.

To fill in before #1012 is closed (the rows the design review lists):

| Run | Lost / refused by kind, per side | Latency per write (median, p95, max), lock on / off | Notes |
|---|---|---|---|
| Paced appends (above) | 0 lost of ~4,000 | 41 ms / 43 ms (server median) | |
| Unpaced appends, one side in a tight loop | | | |
| Uncontended writes over the share | — | | |
| Create tier used on each side (debug line) | — | — | |
| Inode of the lock through the holder's descriptor and through the name, each side | — | — | |
| Clock source: a lock's mtime and a probe's, read from each side, against each side's clock (and after the host slept) | — | — | |
| `kill -9` of one side mid-commit, repeated: time to the other side's first write, failures and messages, breaks, leftovers | | | |
| Same-kernel crash: `kill -9` a host CLI mid-commit, the next host write | | | |
| Suspend the holder past 30 s mid-transaction while the other writes, then resume | | | |
| A host breaking a guest holder (share cache mode recorded) | | | |
| A long hold: a rename over enough notes to run for seconds, with the other side writing and the checkpoint ticking | | | |
| Cold index on one side while the other writes | | | |
| Leftovers after a clean stop and after each kill; nothing staged in git | | | |
| Mixed versions: one side on 0.46.0 | 1 line lost (the lead's first measurement) | | |

### Tests

- `cdno-core/tests/unit/shared_lock_tests.rs`, over two stores with independent `flock`s and boot
  identifiers:
  - two writers exclude each other; the lock holds its record from the moment it exists; a store
    built `without_shared_lock` takes only the `flock`; an index lock takes only the `flock`; an
    index-only transaction refuses a file op;
  - a stale lock is broken by exactly one of two contenders (both judge it stale and look again
    before either renames it; the rendezvous times out instead of hanging);
  - a young lock is never broken (fresh, and five seconds short of stale), and the busy error names
    the lock, its age and its owner;
  - a crashed holder's old leftovers go with its lock, and a live contender's fresh temp and probe
    files stay;
  - a dead holder on this kernel is broken at once; a young lock from another boot waits for its
    age; a lock naming this very process is never taken for a dead one;
  - the break race: a live lock moved aside by mistake is put back and its holder, checking in the
    gap, commits; a put-back never replaces a third writer's lock, and the moved lock's holder
    refuses;
  - a broken holder refuses to commit and leaves the successor's lock; a lock broken after the temp
    file is synced stops the rename; one broken between two file ops rolls the first back;
  - the ownership check refreshes a long-held lock, so another kernel finds it young;
  - release happens on commit, on a refused commit and on drop.
- `cdno-domain/tests/unit/unshared_lock_tests.rs`: two stores with independent `flock`s logging to
  one daily note through `log_to_daily_note`, interleaving, lose no line.
- `cdno-mcp`: the lock folder is never staged by the checkpoint; `vault_busy` and a clean
  `LockLost` are rejections, a part-written `LockLost` is not. `cdno-cli/tests/exit.rs`: both exit
  75, with their `--json` objects. `cdno-cli/tests/lint.rs`: lint does not report the lock files.
  `now.rs` and `completions.rs`: `cdno now --line` and completion leave `.cuaderno/` untouched.
- The `#1001` tests in `cdno-core/tests/unit/unshared_lock_tests.rs` build their writers
  `without_shared_lock`, as an older cdno writes, since the comparison is what they test.
