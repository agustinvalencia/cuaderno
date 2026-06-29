# Vault structure

A vault is a directory tree of Markdown files plus a `.cuaderno/` config folder. `cdno init` creates
the whole layout; here's what each part holds.

```text
vault/
├── journal/
│   ├── daily/
│   │   └── 2026/
│   │       └── 2026-04-25.md      # type: daily (append-only)
│   └── weekly/
│       └── 2026-W17.md           # type: weekly (append-only)
│
├── projects/
│   ├── surrogate-model.md        # type: project (mutable)
│   └── _parked/                  # inactive projects (don't count toward the cap)
│       └── bayesian-opt.md
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
├── inbox/                        # raw captures awaiting triage
│
└── .cuaderno/
    ├── config.toml               # vault configuration
    ├── index.db                  # SQLite index cache (auto-created, rebuildable)
    └── templates/                # note templates (override the built-ins)
```

## Conventions worth knowing

- **`_parked/`** (projects) and **`_done/`** (actions, commitments) prefix folders hold inactive or
  finished notes. The underscore keeps them sorted out of the way and signals "not the active set."
  `_done/` is partitioned by year so the active folders stay scannable.
- **`_index.md`** is the identity note of a folder — the `portfolio` note inside a portfolio folder,
  the `stewardship` note inside an expanded stewardship folder.
- **`tracking/`** inside a stewardship holds time-series entries; **`routines/`** holds prescriptive
  reference documents (a workout plan, a checklist) — those are *not* logs.
- **Stewardships have two shapes:** a flat `stewardships/<slug>.md`, or an expanded
  `stewardships/<slug>/` folder. Only expanded ones can hold tracking entries.

## `.cuaderno/`

- **`config.toml`** — vault settings: the project cap, ignore globs, template behaviour, schema
  extensions, variables. See [Configuration](configuration.md).
- **`index.db`** — a SQLite cache of the vault, used for fast search, linting, and link queries. It
  is rebuilt automatically when it's stale (see [Business rules](business-rules.md)); deleting it is
  safe. **The Markdown files are always the source of truth.**
- **`templates/`** — the note templates `cdno` fills when scaffolding a note. The built-ins are
  written here at `init` so you can edit them; pure variable substitution, no logic.

Next: [Business rules](business-rules.md).
