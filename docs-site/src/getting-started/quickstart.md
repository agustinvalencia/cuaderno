# Quickstart: the daily loop

This is Cuaderno in five commands. It assumes you've [installed](installation.md) `cdno` and
[created a vault](initialise-a-vault.md). Run these from inside the vault (or with `--vault`).

```bash
# 1. Start a project (capped at 5 active).
cdno project create --title "Surrogate model" --context work

# 2. Give it a next action, tagged by the energy it needs.
cdno action add --project surrogate-model \
                --title "Run feature set B on the full mesh" \
                --energy deep

# 3. Record a promise with a deadline.
cdno commit create --title "Pay rent" --due 2026-06-01 --context personal

# 4. Each morning: see what's due, your active projects, and a suggested start.
cdno orient --energy deep

# 5. Mark the action done when it's finished (substring match on the bullet).
cdno action complete --project surrogate-model --query "feature set B"
```

Throughout the day, the two verbs you'll reach for most:

```bash
cdno log "scaled the mesh to 2M cells; runtime 4x, still stable"   # append to today's log
cdno capture "check whether Chen 2025 used the same preconditioner" # drop a thought into the inbox
```

## Interactive vs. scripted

Every write command follows the same convention (see
[CLI overview](../reference/cli/overview.md)):

- **In a terminal**, omit a required flag and `cdno` *prompts* you for it, then asks you to confirm
  before writing.
- **In a script, pipe, or with `--no-interactive`**, a missing required flag is an error instead —
  so automation never blocks on a prompt.

```bash
# Interactive: cdno asks for the title and context.
cdno project create

# Scripted: everything supplied, no prompts.
cdno project create --title "Surrogate model" --context work --no-interactive
```

## Machine-readable output

Add `--json` to any read or write verb to get structured output instead of a formatted table.
Read verbs emit their listing/detail; write verbs emit a `{ "path": ..., "message": ... }` result and
run non-interactively. Great for scripts and for piping into `jq`.

```bash
cdno project list --json | jq '.[].slug'
cdno project create --title "Surrogate model" --context work --json
# -> { "path": "projects/surrogate-model.md", "message": "Created projects/surrogate-model.md" }
```

See [JSON output](../reference/json-output.md) for the full shapes.

## Where to go next

- Understand the model: [The Research Logbook Method](../concepts/rlm.md).
- Work through each workflow: [Tutorials](../tutorials/daily-loop.md).
- Let Claude drive: [Connect to Claude](connect-to-claude.md).
