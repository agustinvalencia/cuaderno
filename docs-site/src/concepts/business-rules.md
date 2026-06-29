# Business rules

Cuaderno enforces a small set of rules that keep the method honest. Knowing them explains why some
commands behave the way they do.

## Markdown is the source of truth

The files on disk are canonical. The SQLite index in `.cuaderno/index.db` is a **cache** — it makes
search, linting, and link lookups fast, but it can always be rebuilt from the Markdown. A stale
index is recoverable; a stale *file* would be data loss, so the tool never lets the cache override
the files. You can delete `index.db` at any time, or rebuild it explicitly with
[`cdno reindex`](../reference/cli/reindex.md).

## Startup reconciliation

On every CLI invocation, MCP session, and app launch, Cuaderno reconciles the index against the
filesystem — comparing modification times and content hashes — and quietly repairs anything stale
before doing your command. This is why edits you make to notes by hand (in your editor, via sync)
are picked up without a manual reindex.

## The five-project cap

At most **five** projects are active at once. Try to create or activate a sixth and the command
stops you — you must [park](../reference/cli/project.md) one first. Parked projects live in
`projects/_parked/` and don't count toward the cap. The limit is configurable via `config.toml`
(see [Configuration](configuration.md)); five is the default because it's the point past which
"active" stops meaning anything.

## Append-only notes

`daily`, `weekly`, `evidence`, and `tracking` notes are **append-only** — Cuaderno only ever grows
them, never overwrites. They are the historical record. (Projects are the deliberate exception; see
next.)

## Project state history is preserved

A project's **Current State** is the one piece of freely-mutable prose in the vault. To keep history
intact, every time you update it (via [`cdno project state`](../reference/cli/project.md) or the MCP
`update_project_state` tool) the *previous* state is auto-logged to today's daily note **before** the
new text overwrites it. You get a clean current view and a full audit trail in the journal.

## Commitments are aggregated, not stored in one place

The [commitments view](../tutorials/commitments.md) is *computed* from four sources, so a promise is
counted wherever it naturally lives:

1. **Project milestones** marked with a hard deadline (`--hard`).
2. **Stewardship periodic commitments** (the recurring lines on a stewardship dashboard).
3. **Standalone commitment notes** in `commitments/`.
4. **Action notes** carrying a self-imposed `due:` that isn't pinned to a milestone.

[`cdno commitments`](../reference/cli/commitments.md) merges and sorts all four by date, with overdue
items flagged.

## Atomicity and its limits

Each write is captured as a transaction (file writes + index updates) that commits as a batch and
rolls back on failure — *while the process is alive*. Startup reconciliation catches index staleness
afterward, but a crash midway through a rare multi-file operation can still leave partial state on
disk. In practice this is vanishingly rare; the takeaway is simply that the Markdown is authoritative
and [`reindex`](../reference/cli/reindex.md) + [`lint`](../reference/cli/lint.md) will surface
anything odd.

Next: [Contexts and energy](contexts-and-energy.md).
