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

## Keeping the index on each machine

The index is a per-machine cache, so it can be kept outside the vault folder. Set
`CUADERNO_INDEX_PATH` to the absolute path of the database file and every binary uses that file
instead of `.cuaderno/index.db`: the `cdno` CLI, the stdio `cdno-mcp` server, and
`cdno-mcp-server`, which also takes it as [`--index-path`](../reference/mcp/http-server.md#flags)
(the flag wins over the variable).

```bash
export CUADERNO_INDEX_PATH="$HOME/.cache/cuaderno/surrogate-model.db"
```

**Set it when one vault folder is reachable from two machines at once**: a vault bind-mounted into
a container (through a VM file share such as virtiofs), or a vault on a network share. SQLite's file
locks and its shared-memory file do not cross that boundary, so two machines opening one index
file each act as if alone and damage it. Give each machine its own value, with the file on that
machine's own storage, outside the shared folder. **Two machines must never share one index
file**, whether it sits at the default location or at an override.

- **Leave it unset** for a vault used from one machine: the index stays at `.cuaderno/index.db`,
  exactly as before.
- **The value must be absolute.** A relative value is refused with an error rather than resolved
  against the vault or the working directory. `cdno` does not expand `~`, so write `$HOME` (or an
  unquoted `~`, which the shell expands first). An empty value counts as unset, and a value naming
  a directory is refused.
- **The siblings follow the file.** SQLite's `-wal` and `-shm` files, and any file a rebuild writes
  beside the index, sit next to the database file wherever it is. A missing parent folder is
  created.
- **It is not a `config.toml` key**, on purpose: the config travels with the vault, and this
  setting has to differ from one machine to the next.
- Moving the index needs no migration. The new location starts empty and the next open rebuilds it
  from the Markdown; an old `.cuaderno/index.db` is no longer read and can be deleted.

[`cdno reindex`](../reference/cli/reindex.md) rebuilds the file the variable names, and the HTTP
server's git checkpoints never commit an index that the override places inside the vault.

Next: [Business rules](business-rules.md).
