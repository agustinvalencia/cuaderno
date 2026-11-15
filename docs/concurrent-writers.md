# Writers the vault lock does not exclude

Design note for #1001 (part of the epic on vaults shared between two kernels, #1005). It says what
a commit checks so that a second writer's change is not silently discarded, how that check reaches
every write, and which windows remain open.

## The problem

Every domain write is read, modify, write-by-rename. Writers serialise on an advisory `flock` on
`.cuaderno/.lock`, held for the life of a `VaultTransaction` (#196). `flock` is enforced by one
kernel. When one vault folder is written from two kernels at once (a host and a container that
bind-mounts it through a VM file share, or a network share that does not forward locks), both sides
take "the" lock at the same moment. Two read-modify-writes then interleave, and the second rename
replaces the first writer's file with content composed before that writer's change. Each file stays
well formed, so nothing looks wrong afterwards. Measured before this change: two writers appending
80 lines each to one daily note lost 11 of 160 lines across two kernels, and none within one.

A program that does not take the lock at all (an editor, a sync tool writing the working tree) is
the same case seen from another side.

## The design: compare before rename

A commit knows what the operation read of every file it is about to replace, append to or delete,
re-checks the file on disk just before writing it, and on a mismatch writes nothing and fails with a
distinct error.

### Where "what I read" lives: a read journal the store fills

Domain operations read through `Vault::store` directly and stage writes on the transaction; the two
do not meet. Changing the dozens of verbs to read through the transaction would be invasive, and a
verb that forgot would silently skip the check. So the store records instead:

- `VaultTransaction::new` takes the lock and then opens a `ReadJournal` on the store
  (`cdno-core/src/read_journal.rs`). While it is open, `FsVaultStore` and `MemoryVaultStore` record
  the **first** observation of every path they are asked about: `read_file` and `read_bytes` record
  the content's xxh3 fingerprint and length, a `NotFound` records "absent", and an `exists` that
  answers false records "absent". An `exists` that answers true records nothing (it saw no content).
- A path staged for a write, an append or a delete with nothing recorded (a blind overwrite, a
  delete decided from a listing) is observed when it is staged. A path that could not be observed
  then is observed at the start of the commit. **No staged path goes unchecked**; a verb that never
  read the file is checked from a later moment, not skipped.
- A store that records nothing (a test double that does not delegate `read_journals`) is covered by
  the same staging observation.

The journal is per store instance and closes when the commit starts, so the commit's own reads are
never mistaken for the operation's.

**Why the first observation wins.** A store is shared by every thread of a process, so while a
transaction is open, a read-only call on another thread records into its journal too. The
operation's own read always comes after the journal opened, so the recorded fingerprint is either the
one the operation read or an older one. An older one can only make the check fire when it need not
(a spurious conflict, cleared by a retry); it can never let a change through. Recording the last
observation instead would be unsafe.

Rejected: the transaction recording reads made through a new `tx.read_file` (every verb would have
to opt in, and one that forgot would skip the check); a versioned read returned by the store (same
problem, and it changes every read call site); size and mtime instead of a content fingerprint
(coarse on some shared filesystems, and a same-size rewrite within one tick is invisible).

### What the commit checks

1. **Before any file op** (`precheck` in `transaction.rs`), every path whose first touch in the
   commit is a write, an append or a delete is compared with its first observation. Every mismatch
   is collected, and the commit returns `TransactionError::Conflict { paths }` having written
   nothing. Later ops on a path the commit already touched are compared with what the earlier op
   left, not with the read.
2. **As each op applies**, the read that takes its undo snapshot is compared too (no extra read).
3. **In the store, right before the rename** (`write_file_expecting`, `append_to_file_expecting`):
   `FsVaultStore` writes and fsyncs the temp file, reads the target again, compares it, checks with
   one `stat` that the target is still the same file (inode, size, times), and only then renames. A
   file the commit expects to be absent is created with a no-clobber rename (`persist_noclobber`),
   which the kernel refuses if anything is there, so a create has no window at all. A filesystem
   that supports neither a no-clobber rename nor hard links falls back to checking for absence and
   renaming, with the same gap as a replace (window 1 below) rather than failing every creation.
   `MemoryVaultStore` compares and writes under one mutex.

A mismatch found in step 2 or 3 rolls back what was applied, in reverse order. The rollback is
itself conditional: each undo carries what its op left on disk, and a path that no longer holds that
(another writer changed it after this commit wrote it) is left alone and reported as a rollback
failure rather than overwritten. A clean rollback is reported as `Conflict`; one that could not
undo everything is `FileWrite { source: Changed, rollback_failures }`, because the vault may then be
part-written.

**Moves are not compared.** A move carries the file along whatever it holds, so it cannot discard a
change, and comparing would mean reading an attachment's bytes, which a move never did before (a
rename test pins that). Notes are never moved this way: a project park, a closure or a rename writes
the new path and deletes the old one, and both of those are compared. A move's destination, like an
import's, is created by a store call that refuses to overwrite.

### What happens on a mismatch

The transaction cannot re-apply a change: that means re-running the domain operation that composed
it. Two layers:

- **Every verb** fails with the typed error: `TransactionError::Conflict`, wrapped as
  `DomainError::Transaction`, "… changed on disk while this write was being prepared, most likely by
  another writer; nothing was written, run the operation again". `DomainError::is_concurrent_change`
  names it. Because nothing was written, running the verb again is safe.
- **`Vault::log_to_daily_note`** (`cdno log`, `append_to_log`) retries itself up to four attempts in
  all, with a short growing pause (`vault/conflict_retry.rs`), before returning the conflict. Its
  every input is its arguments and the vault, so re-running it is exactly the append asked for, and
  appends are the case that was measured. Verbs whose content a caller may have composed from its own
  earlier read (a section replacement, a revision, a state update) are not retried by the domain:
  only the caller knows whether to re-read and recompose. Extending the retry to other verbs that
  compose everything server-side (capture, `note_to_daily`, action starts) is safe and is left for a
  follow-up.

The interfaces surface the error so a caller can act on it:

- **MCP**: a tool result with code `concurrent_change` and `details.paths`, not a protocol error,
  because the caller's remedy is to call again; the message says to re-read first if the content was
  composed from an earlier read.
- **CLI**: the usual `Error: …` report and exit status **75** (`EX_TEMPFAIL`), so a script can retry.

## The windows that remain

The check narrows the window in which a change can be lost from the whole read-modify-write to the
gap between the last comparison and the rename. It does not close it.

1. **Between the last `stat` and the rename of a replaced file.** No rename can be made conditional
   on what it replaces (an exchange-and-verify with `RENAME_EXCHANGE` / `RENAME_SWAP` was
   considered: it briefly publishes the conflicting content, which a third read can build on, and
   its support across VM file shares is unknown). A writer that renames inside this gap is still
   overwritten. Measured on a laptop SSD with two writers in a tight loop, where every commit
   contends: 0 to 2 lines lost of 300, against about 150 of 300 with the check disabled
   (`on_a_real_folder_only_the_gap_before_the_rename_remains` in `cdno-core`). With writers at a
   human pace the gap is microseconds wide against minutes between writes.
2. **Between the comparison and the unlink of a deleted file.** A delete compares the file it read
   and then removes it; a change landing between the two is removed with it.
3. **A read made before the transaction opened** is no baseline: the journal only records while the
   lock is held. That is the existing #196 rule (open the transaction before reading); a verb that
   breaks it is compared from the moment it stages the write.
4. **Writers that do not use this protocol** (an editor, a sync tool) are protected against only as
   far as the comparison sees their change before the rename; nothing makes them check ours.
5. **A move** carries whatever is at its source; it is not compared (see above), by design.
6. **The raw writes** that bypass the transaction and the lock (`write_note_raw`, template eject and
   save, `save_config_raw`) bypass this check too. They were left as they are and must not be
   widened.
7. **The index.** The check is about the Markdown; a long-lived process's index can still lag
   another kernel's writes until it reconciles (#1002), and SQLite's own locking is not shared across
   kernels (#998).

Closing the first two windows needs a lock both kernels see, for example an exclusive-create lock
file beside the `flock` (option 2 of #1001), which can be added on top of this later.

## Interactions

- **The index and `IndexStale`.** A conflicted commit returns before any index op, so the index is
  untouched. A commit whose files land behaves as before: an index failure after them is still
  `IndexStale`.
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
  `fsync`s dominate either way.

## Tests

- `cdno-core/tests/unit/unshared_lock_tests.rs`: two stores over one folder with independent lock
  files (`FsVaultStore::with_lock_file`, a test-only seam) interleave read-modify-writes by hand
  (second commit refused, first intact; creates, deletes, appends, blind writes; multi-file
  atomicity; a change landing mid-commit rolls back; a rollback does not overwrite a later change);
  two threads race appends over a shared in-memory store (exact: no line lost) and over a real
  folder (a measurement of window 1, `#[ignore]`d because its outcome depends on scheduling; run it
  with `cargo test -p cdno-core --test unit -- --ignored on_a_real_folder --nocapture`);
  single-machine commits are unchanged.
- `cdno-domain/tests/unit/unshared_lock_tests.rs`: two `Vault`s logging to one daily note lose no
  line; a stale read is retried onto the fresh note; a log that never converges returns the conflict
  and writes nothing; a section write from a stale read is refused, not retried.
- `cdno-mcp` (`rejection.rs`): the `concurrent_change` code and payload; `cdno-cli/tests/exit.rs`:
  exit status 75.
