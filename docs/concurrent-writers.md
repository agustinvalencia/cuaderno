# Writers the vault lock does not exclude

Design note for #1001 (part of the epic on vaults shared between two kernels, #1005). It says what
a commit checks so that a second writer's change is not silently discarded, how that check reaches
every write, how much it helps, and which windows remain open. It narrows the loss; it does not
remove it. The lock both kernels see, which closes it, is #1012.

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
zero. The line that was lost was acknowledged to its writer: window 1 below.

**Why a one-machine figure is not representative.** Two writers in a tight loop on one Mac lose 0 to
2 lines of 300 with the check and about 150 without it (the ignored test
`on_a_real_folder_only_the_gap_before_the_rename_remains`). That low figure is an artefact: `sync_all`
on macOS is a full device flush (`F_FULLFSYNC`), which serialises the two writers into strict
alternation and acts as an accidental shared lock. A reviewer reproduced the protocol standalone:
with a plain `fsync` the same check lost 38 to 52 of 300, and with a short delay between the last
check and the rename nearly every contended line. What decides the rate is the width of the gap
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
  opens its transaction fails its tests; a release build keeps the staging-time comparison.
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
- **The append-shaped verbs retry** (`Vault::retry_on_conflict`, `vault/conflict_retry.rs`): four
  attempts in all, with a short growing pause, before the conflict is returned. A refused commit
  wrote nothing, every verb writes in one transaction, and the comparison is against the
  transaction's own reads, so a retry is exactly as safe as the verb's no-conflict path. Nearly every
  mutating verb appends a history line to today's daily note, so that note is where conflicts arise
  whichever verb is called. Retried: `log_to_daily_note`, `note_to_daily`, `upsert_daily_section`
  with `append`, `capture_to_inbox`, `discard_inbox_item`, the action verbs (`add`, `start`,
  `start_unplanned`, `switch`, `switch_unplanned`, `pause`, `resume`, `complete`, `drop`), the
  milestone verbs (`add`, `complete`, `drop`), the waiting-on verbs (`add`, `resolve`), and
  `add_tracking_entry`.
- **Not retried, and why.** The verbs that replace content a caller may have composed from what it
  read earlier (`upsert_*_section` without `append`, `update_project_state`, `set_frontmatter`,
  `revise_note`), and `rename_note`, which writes a plan the caller may have approved by its hash.
  Retrying them would be as safe as their no-conflict path, but a conflict says the note is being
  written right now, which is the moment to let the caller look again. Whether the caller's own
  earlier read is stale is a different question that this does not answer (#563). The create and
  lifecycle verbs (projects, questions, portfolios, commitments, stewardships, closures, migration)
  do not retry yet either; they are as safe to retry and are left out only to keep this change
  reviewable.

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
2. **Between the comparison and the unlink of a deleted file.** A delete compares the file it read
   and then removes it; a change landing between the two is removed with it.
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
with a stale-break rule, tracked in #1012 (option 2 of #1001). With it, cdno writers exclude each
other everywhere, and this comparison remains the only protection against writers that take no
lock. The two are complements.

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
  - a log that never converges returns the conflict after exactly four attempts and writes nothing;
  - a section replace from a stale read is refused, not retried;
  - a capture and a stewardship never replace one made at the same path meanwhile;
  - a discard logs the text it deletes.
- `cdno-mcp`: the `concurrent_change` code, payload and message (`rejection.rs`), and the server
  instructions sentence (`tests/server.rs`). `cdno-cli/tests/exit.rs`: exit status 75 and the
  `--json` object.
