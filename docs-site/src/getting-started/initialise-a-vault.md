# Initialise a vault

A **vault** is just a directory of Markdown files with a `.cuaderno/` config folder at its root.
Create one with `cdno init`:

```bash
cdno init ~/notebook
cd ~/notebook
```

This scaffolds the full folder tree, writes a default `.cuaderno/config.toml`, and drops the
built-in note templates into `.cuaderno/templates/` (so you can customise them later). What you get:

```text
~/notebook/
├── journal/          # daily + weekly notes, partitioned by year
│   └── 2026/         #   e.g. journal/2026/daily/2026-04-25.md, journal/2026/weekly/2026-W17.md
├── projects/         # project maps (max 5 active)
│   └── _parked/      # inactive projects
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
