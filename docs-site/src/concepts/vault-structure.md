# Vault structure

A vault is a directory tree of Markdown files plus a `.cuaderno/` config folder. `cdno init` creates
the whole layout; here's what each part holds.

```text
vault/
├── journal/
│   └── 2026/                      # partitioned by (ISO) year
│       ├── daily/
│       │   └── 2026-04-25.md      # type: daily (append-only)
│       └── weekly/
│           └── 2026-W17.md       # type: weekly (append-only)
│
├── projects/
│   ├── surrogate-model.md        # type: project (mutable)
│   ├── _parked/                  # parked projects: complete maps that don't count
│   │   └── bayesian-opt.md
│   └── _done/
│       └── 2025/                 # closed projects: completed or dropped
│           └── thesis-chapter-2.md
│
├── actions/
│   ├── characterise-sampler.md   # type: action (manifest form)
│   └── _done/
│       └── 2026/                 # completed actions, partitioned by year
│           └── run-ablation.md
│
├── portfolios/
│   └── sparse-vs-dense-ood/
│       ├── _index.md             # type: portfolio
│       ├── 2026-03-15-chen-2025.md   # type: evidence (append-only)
│       └── 2026-04-01-ablation-b.md
│
├── stewardships/
│   ├── finances.md               # type: stewardship (flat)
│   └── health/                   # expanded variant
│       ├── _index.md             # type: stewardship
│       ├── tracking/             # type: tracking entries (append-only)
│       │   └── 2026-04-06-gym.md
│       └── routines/             # reference docs, not logs
│           └── upper-body-a.md
│
├── commitments/
│   ├── pay-rent.md               # type: commitment
│   └── _done/
│       └── 2026/                 # fulfilled commitments
│
├── questions/
│   ├── research/
│   │   └── surrogate-cost.md     # type: question (domain: research)
│   └── life/
│       └── apartment-as-home.md  # type: question (domain: life)
│
├── concepts/                     # the concept library (a custom type `cdno init` declares)
│   ├── woodbury-identity.md      # type: concept (mutable, refined in place)
│   └── linear-algebra.md         # a hub note: mostly links
│
├── inbox/                        # raw captures awaiting triage
│
└── .cuaderno/
    ├── config.toml               # vault configuration
    ├── index.db                  # SQLite index cache (auto-created, rebuildable)
    ├── .lock, locks/             # the vault write lock (locks/write.lock only while a write runs)
    └── templates/                # note templates (override the built-ins)
```

## Conventions worth knowing

- **`_parked/`** (projects) and **`_done/`** (projects, actions, commitments) prefix folders hold
  inactive or finished notes. The underscore keeps them sorted out of the way and signals "not the active set."
  `_done/` is partitioned by year so the active folders stay scannable.
- **`projects/_parked/`** holds complete maps that are set aside: they take every plan write
  (actions, state, milestones, waiting-on), keep their hard dates in the commitments register, and
  do not count toward the cap. You read them at the monthly review. A handed-off project
  (`held_by:` in its frontmatter) stays in whichever folder it was in; only the name changes. See
  [Business rules](business-rules.md#parked-projects-are-complete-maps).
- **`_index.md`** is the identity note of a folder — the `portfolio` note inside a portfolio folder,
  the `stewardship` note inside an expanded stewardship folder.
- **`tracking/`** inside a stewardship holds time-series entries; **`routines/`** holds prescriptive
  reference documents (a workout plan, a checklist) — those are *not* logs.
- **`concepts/`** is flat by design: tags and links, including hand-written hub notes, give it
  structure. See [The concept library](concept-library.md).
- **Stewardships have two shapes:** a flat `stewardships/<slug>.md`, or an expanded
  `stewardships/<slug>/` folder. Only expanded ones can hold tracking entries.

## `.cuaderno/`

- **`config.toml`** — vault settings: the method version the vault follows
  (`[vault] method_version`), the project cap, the holder deny-list (`[projects] holder_sentinels`),
  ignore globs, template behaviour, schema extensions, variables. See [Configuration](configuration.md).
- **`index.db`** — a SQLite cache of the vault, used for fast search, linting, and link queries. It
  is rebuilt automatically when it's stale (see [Business rules](business-rules.md)); deleting it is
  safe. **The Markdown files are always the source of truth.** It can live outside the vault
  instead; see [Keeping the index on each machine](#keeping-the-index-on-each-machine).
- **`templates/`** — the note templates `cdno` fills when scaffolding a note. The built-ins are
  written here at `init` so you can edit them; pure variable substitution, no logic.
- **`.lock`** and **`locks/`** — the vault write lock. Every write takes both, one after the
  other, and holds them while it reads and rewrites notes. `.lock` stays (an empty file the
  operating system locks), and only processes on the same machine see that lock. `locks/write.lock`
  exists only while a write holds it, and processes on another machine or in a container that share
  the folder see it too (see [Sharing a vault between machines](#sharing-a-vault-between-machines)).
  Other files in `locks/` live for an instant while a write takes or breaks the lock. None of them
  is a note; the folder keeps itself out of git (its own `.gitignore`), and should be kept out of
  a sync tool (below).

## Sharing a vault between machines

When one vault folder is reachable from two operating systems **at the same time** (a host and a
container that mounts it, or two computers on a network share), the writes of both are kept apart by
`.cuaderno/locks/write.lock`, a lock file both sides see: a write on one side waits for a write on
the other, so neither replaces a note the other has just changed. What still matters:

- **Upgrade every side together.** 0.46.0 and older take neither this lock nor the check that a note
  did not change under a write, so with one side on such a version lines are still lost, silently.
  An older `cdno` or `cdno-mcp-server` is no more excluded than an editor is.
- **Writers that take no lock** (an editor, a sync tool) are not excluded by it. A write that finds
  the note changed under it refuses, writing nothing (see
  [Troubleshooting](../reference/troubleshooting.md#a-write-fails-with-changed-on-disk-while-this-write-was-being-prepared)).
- **A writer that crashes holding the lock** leaves `write.lock` behind. The next write on the same
  machine (or a container of the same kernel reaching the folder through the same mount, a
  restarted one included) takes it over at once. One on the other machine waits until the lock is
  30 seconds old, failing as busy until then, and then takes it over by itself
  (see [Troubleshooting](../reference/troubleshooting.md#a-write-fails-with-timed-out--waiting-for-the-vault-write-lock)).
- **A writer paused while holding it** (a laptop asleep, a paused container) has it taken over once
  it is 30 seconds old; when it wakes, its write stops and undoes itself. One that stops in the very
  last instant of a write, between its last check and replacing the note, can still overwrite a
  line the other side wrote meanwhile: the lock narrows that to one step, and nothing short of the
  file server itself can close it.
- **The config and the templates are not covered.** Editing `config.toml` or a template through
  cdno (`cdno config` edits, `cdno templates` eject, save and sync) takes no lock, on one machine
  or two; do that from one side at a time.
- **Give each machine its own index**, next section: the lock keeps the notes safe, not a SQLite file
  shared between two kernels.
- **Not on an object-store mount.** A bucket mounted as a folder cannot create a file exclusively
  across two machines, so the lock excludes nothing there.

**Synced copies are a different case.** A tool that keeps two copies of the vault in step (git,
Syncthing, Dropbox, iCloud Drive) is not two machines writing one folder: each machine writes its own
copy, its own lock keeps its own writers apart, and the tool resolves conflicts between the copies.
The lock file does nothing across copies; a copy of it that the tool carries over can only make the
other machine wait (up to 30 seconds), or have a write there refused (and retried, for the commands
that add to a note) when the tool carries a break back onto a newer lock, never lose a line. So
keep the folder out of the sync, and let the tool leave `.cuaderno/.lock` alone too (replacing it
under a writer splits the lock):

- git: nothing to do; the folder carries its own `.gitignore`. If a copy of it was ever committed,
  `git rm -r --cached .cuaderno/locks`.
- Syncthing: a line `.cuaderno/locks` in `.stignore`.
- Dropbox: mark `.cuaderno/locks` as ignored.
- A tool with no ignore rules (iCloud Drive): expect an occasional wait of up to 30 seconds on the
  other machine after a write here.

## Keeping the index on each machine

The index is a per-machine cache, so it can be kept outside the vault folder. Set
`CUADERNO_INDEX_PATH` to the absolute path of a database file and that process uses it instead of
`.cuaderno/index.db`. All three binaries honour it: the `cdno` CLI, the stdio `cdno-mcp` server, and
`cdno-mcp-server`, which also takes it as [`--index-path`](../reference/mcp/http-server.md#flags)
(the flag wins over the variable). The CLI deliberately has no flag for it: the setting belongs to a
machine, not to one command.

**When to set it.** Set it when one vault folder is reachable from two machines at once: a vault
mounted into a container, or a vault on a network share. SQLite's file locks and its shared-memory
file do not cross that boundary, so two machines opening one index file each act as if alone and
damage it. **Two machines must never share one index file**, whether it sits at the default
location or at an override. Leave it unset for a vault used from one machine: the index stays at
`.cuaderno/index.db`, exactly as before.

**Set it for one process, not your whole shell.** The value names one file, and every vault the
process opens would use that file. Put it in the environment of the process that serves the shared
vault: the server's own environment, or the `env` block of an MCP client's server entry. Do not
`export` it from a shell profile if you use more than one vault (a scratch vault from `cdno init`
counts). For example, for the HTTP server:

```bash
CUADERNO_VAULT_PATH=/srv/vaults/surrogate-model \
CUADERNO_INDEX_PATH=/var/lib/cuaderno/surrogate-model.db \
  cdno-mcp-server
```

- **One file per vault.** An overridden index records the vault it belongs to, and opening it for a
  different vault is refused with an error that names both; so is a plain `cdno reindex` there. To
  give the file to another vault on purpose, first stop whatever serves the vault it belongs to,
  then run [`cdno reindex --rebind`](../reference/cli/reindex.md) in the other vault: it deletes
  the old vault's index, archival snapshots included, builds a new file for that vault, and says
  so. Where only the server is installed, delete the file instead. (An ordinary `cdno reindex` in
  the vault the file belongs to rebuilds it in place and keeps the snapshots.)
- **The binding is the vault's path.** Move or rename the vault folder and the index reads as
  another vault's: every open is refused, naming the old path. The way out is the same
  `cdno reindex --rebind`, run in the vault at its new path, and it costs that vault's **own**
  archival snapshots, since the file is rebuilt from nothing (see
  [Rebinding](../reference/cli/reindex.md#rebinding)). Deleting the file costs the same.
- **Persistent storage, in a directory the process owns.** SQLite's `-wal` and `-shm` files, and any
  file a rebuild writes, are created beside the database file, so its directory must be writable
  and must not be a single file mounted on its own. A missing parent directory is created. An index
  on storage that is wiped (a container's writable layer, say) is rebuilt from nothing each time.
- **The servers repair it where it is.** An MCP server checks and repairs an overridden index like
  one at the default location (see
  [The index looks after itself](../reference/mcp/overview.md#the-index-looks-after-itself)): the
  fresh file a rebuild builds, and the `.previous` copy it keeps of a damaged file, sit beside the
  override's database file, nothing is written under `.cuaderno/`, and the rebuilt index stays bound
  to its vault. A file the override refuses (not cuaderno's index, or another vault's) is refused at
  start as above, never repaired or moved aside. When the file is deleted or replaced under a
  running server (a `--rebind`, a restore), the server reopens it on its next sweep through the same
  guards: a file now bound to another vault is refused, and the server says so in `index_status`
  rather than writing into it.
- **The symptom of a shared index, without the override.** A process on another kernel that cannot
  see the server's locks deletes the index's `-wal` and `-shm` each time it exits, taking itself for
  the last user of the file. A server notices on its next sweep and reopens the index, logging each
  reopen; three within an hour are reported as `repair: reopen_recurring` with
  `last_reopen.kind: sidecars_moved` (the server keeps reopening). That is the cue to set
  `CUADERNO_INDEX_PATH` on one side (see
  [Troubleshooting](../reference/troubleshooting.md#the-index-files-keep-moving-under-the-server)).
- **Keep it off the shared folder.** Use the machine's own storage. An override inside the vault
  works, but the file stays on the share, a file-sync tool picks it up, and only `cdno-mcp-server`'s
  git checkpoints exclude it: the CLI and the stdio server add no ignore rule, so your own
  `git add -A` would commit it.
- **The value must be absolute.** A relative value is refused rather than resolved against the vault
  or the working directory. `cdno` does not expand `~`, so write `$HOME` (or an unquoted `~`, which
  the shell expands first). An empty value counts as unset. A value naming a directory (or ending
  in `/`) and one naming a note (`.md`, `.typ`) are refused. A file already at the path that
  cuaderno did not create as its index (any other file, another application's SQLite database
  included) is never opened for writing or deleted: cuaderno refuses instead. The one exception is a
  file with SQLite's header that SQLite itself cannot read (it reports it damaged, or not a
  database): that is taken for a damaged index and rebuilt, so a corrupt index still heals.
- **It is not a `config.toml` key**, on purpose: the config travels with the vault, and this
  setting has to differ from one machine to the next.

**What it does not do.** The override stops the damage, not the staleness. Each machine's index
learns of the other machine's writes only when it reconciles: the CLI at every command except [`cdno now`](../reference/cli/now.md) and [shell completion](../reference/cli/completions.md#what-a-tab-press-reads), which never open the index, the HTTP
and stdio servers at start and then on every sweep, every `CDNO_MCP_RECONCILE_INTERVAL_SECS`
seconds (300 by default; `--reconcile-interval-secs` on the HTTP server; see
[Keeping up with changes made elsewhere](../reference/mcp/overview.md#keeping-up-with-changes-made-elsewhere)). In that
window search and backlinks can miss the other side's latest notes, and the active-project cap and
the check that a new note's name is unique are made against that stale view, so the cap can be
exceeded or a name taken twice until someone reconciles. The override separates the index only.
For the notes themselves, the two machines' writes are kept apart by the lock file both see
([Sharing a vault between machines](#sharing-a-vault-between-machines)), and a write still refuses
to replace a note that something else changed while it was being prepared
([a write fails with "changed on disk …"](../reference/troubleshooting.md#a-write-fails-with-changed-on-disk-while-this-write-was-being-prepared)).

**Moving an index.** No migration is needed: the new file starts empty and the next open rebuilds
it from the Markdown. A new file starts without the archival snapshots (only a rebuild of an
existing index carries them across), so [`cdno lint`](../reference/cli/lint.md) loses its
frozen-prefix check on archived actions (see
[`cdno reindex`](../reference/cli/reindex.md#archival-snapshots)). Do **not** delete the old `.cuaderno/index.db` while
another machine still uses the default location for the same vault; it is that machine's index.

Next: [Business rules](business-rules.md).
