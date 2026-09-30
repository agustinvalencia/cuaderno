# Initialise a vault

A **vault** is just a directory of Markdown files with a `.cuaderno/` config folder at its root.
Create one with `cdno init`:

```bash
cdno init ~/notebook
cd ~/notebook
```

This scaffolds the full folder tree, writes a default `.cuaderno/config.toml` (which declares the
[`concept`](../concepts/concept-library.md) type), and writes two starter templates into
`.cuaderno/templates/`: `daily.md` and `concept.md`. Every other note type renders from its built-in
template until you add a file for it there. What you get:

```text
~/notebook/
├── journal/          # daily + weekly notes, partitioned by year
│   └── 2026/         #   e.g. journal/2026/daily/2026-04-25.md, journal/2026/weekly/2026-W17.md
├── projects/         # project maps (max 5 active)
│   ├── _parked/      # inactive projects
│   └── _done/        # closed projects, partitioned by year
├── actions/          # manifest action notes (the heavy form)
│   └── _done/        # completed actions, partitioned by year
├── portfolios/       # evidence dossiers, one folder per question
├── stewardships/     # long-haul responsibilities (flat file or folder)
├── commitments/      # standalone promises with deadlines
│   └── _done/        # fulfilled commitments
├── questions/
│   ├── research/
│   └── life/
├── inbox/            # quick captures awaiting triage
├── concepts/         # the concept library (a custom type init declares)
└── .cuaderno/
    ├── config.toml   # vault configuration
    └── templates/    # note templates (override the built-ins here)
```

See [Vault structure](../concepts/vault-structure.md) for what each folder holds.

## Running `cdno` from anywhere

You rarely need to be at the vault root. `cdno` finds the vault by walking **up** from your current
directory until it sees a `.cuaderno/` folder — so commands work from any subdirectory.

When you're *outside* any vault, two fallbacks apply, in order:

1. The `--vault <PATH>` flag (highest priority — overrides everything).
2. The `CUADERNO_VAULT_PATH` environment variable.

```bash
# From anywhere, target a specific vault:
cdno --vault ~/notebook log "spotted a bug in the sampler"

# Or set it once for the shell session:
export CUADERNO_VAULT_PATH=~/notebook
cdno log "spotted a bug in the sampler"
```

> If you're standing inside vault A while `CUADERNO_VAULT_PATH` points at vault B, the directory you
> are in wins — writes land in A. The env var is only a fallback for when discovery finds nothing.

## Upgrading an older vault

`cdno init` refuses to run on a vault that already exists: re-initialising is destructive, so it is
never done for you. When a newer `cdno` ships a note type that `init` now declares, add it to your
existing vault instead:

```bash
cdno config note-type install --list             # what ships, and whether this vault has it
cdno config note-type install --name concept
```

It writes the type's template, its folder and its declaration, each only when absent. A
declaration you already have, or a template you have edited, is kept and reported, never
overwritten, so the command is safe to re-run; it ends with `concept: already installed, nothing to
do` when there is nothing left to write. Add `--dry-run` to see exactly what it would write first.
See [Installing a bundled note type](../reference/cli/config.md#installing-a-bundled-note-type).

## Back up your vault

A vault is just Markdown files (the SQLite index in `.cuaderno/index.db` is a rebuildable cache — see
[Business rules](../concepts/business-rules.md)). The simplest, most durable backup is **version
control**: `git init` the vault and commit as you go, or keep it in a synced folder.

```bash
cd ~/notebook
git init && git add . && git commit -m "Initial vault"
# The index is regenerated on demand, so it's safe to ignore:
echo ".cuaderno/index.db" >> .gitignore
```

Because the Markdown is the source of truth, your history is just your commits — nothing is locked
inside a proprietary store.

## Next step

Run your first daily loop: [Quickstart](quickstart.md).
