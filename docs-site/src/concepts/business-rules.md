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

At most **five** projects count at once. A project counts when it is **active and names no one
else as its holder**: parked projects don't count, closed ones don't, and neither does an active
project that someone else is moving ([below](#handed-off-projects)). Creating a project at the cap
creates it parked; the cap is checked whenever a project that did not count begins to: on
activation, so activating a sixth stops you — you must [park](../reference/cli/project.md) one
first — and when you take an active project back from its holder. Parked projects live in
`projects/_parked/`. Closed ones are not open at all: a project that is
finished is [completed](../reference/cli/project.md#cdno-project-complete), and one that is not
going to happen is [dropped](../reference/cli/project.md#cdno-project-drop), both into
`projects/_done/<year>/`. Closing never needs a slot, and a parked project can be closed directly. The limit is configurable via `config.toml`
(see [Configuration](configuration.md)); five is the default because it's the point past which
"active" stops meaning anything.

## Parked projects are complete maps

Parking changes two things only: the project does not count, and you read it at the monthly
review rather than daily. A parked map takes every plan write an active one does: next actions,
Current State, milestones, waiting-on items, the core question and a holder, from the CLI and over
MCP alike, so you never activate a project just to write on it. Its hard milestones and action
due dates stay in the [commitments register](#commitments-are-aggregated-not-stored-in-one-place).

Only **starting, switching or resuming work, and parking** need an active project
([`cdno action start`](../reference/cli/action.md), `switch`, `resume`, and
[`cdno project park`](../reference/cli/project.md#cdno-project-park)): a project you start work on is
a project you are moving, so it counts. Activate it first, parking another if the cap is full.

## Handed-off projects

An open project may name **who else is moving it** in a `held_by:` line at the top of its map:
`held_by: "the harbour office"`. [`cdno project hand-off`](../reference/cli/project.md#cdno-project-hand-off)
writes it and [`take-back`](../reference/cli/project.md#cdno-project-take-back) clears it, each
logging a `holder on [[<slug>]]` entry in today's daily note. An active project that names a holder
stays where it is, keeps its actions, its log and its dates in the register, and does not count
toward the cap: the cap counts your attention, not someone else's.

Three guards keep it honest. If you cannot name who has it, it is yours. A handed-off map says what
you are waiting for in its Waiting On. And handing off is never the answer to a full cap: name a
holder only when someone else really has the project. To make room, park, complete or drop one.

**The deny-list.** A `held_by:` value that names you or nobody (`me`, `tbd`, `someone`, `?`, …)
reads as no holder, so the map stays yours and counts, and the hand-off verb refuses it. Those
values are a config key, [`[projects] holder_sentinels`](../reference/configuration.md#projects),
with a default list you can replace, or set to `[]` to accept any name. Its stated limit: **the tool
cannot know your own name**, so `held_by: "<your name>"` is read as someone else unless you add your
name to the list. A value that is not a plain name (an unquoted wikilink, a number, a date) also
reads as no holder; [`cdno lint`](../reference/cli/lint.md) reports it so you can quote it.

## The method version

The vault records which version of the [Research Logbook Method](rlm.md) it follows, as
`[vault] method_version` in `.cuaderno/config.toml`. That is the method's version, not cdno's:
`cdno init` writes the version this cdno implements, and only
[`cdno migrate`](../reference/cli/migrate.md) changes it. A vault with no key is read as RLM 1.0. A
cdno refuses to write to a vault on another **major** version (an older one: migrate it; a newer
one: upgrade cdno), while reads keep working; a minor difference only prints a notice.

## Append-only notes

`daily`, `weekly`, `evidence`, and `tracking` notes are **append-only** — Cuaderno only ever grows
them, never overwrites. They are the historical record. (Projects are the deliberate exception; see
next.)

## Project state history is preserved

A project's **Current State** is the one piece of freely-mutable prose in the vault. To keep history
intact, every time you update it (via [`cdno project state`](../reference/cli/project.md) or the MCP
`update_project_state` tool) the *previous* state is auto-logged to today's daily note **before** the
new text overwrites it. You get a clean current view and a full audit trail in the journal. A
hand-off or take-back logs the same way, as `holder on [[<slug>]]` with `was:` and `now:` lines.

## Commitments are aggregated, not stored in one place

The [commitments view](../tutorials/commitments.md) is *computed* from four sources, so a promise is
counted wherever it naturally lives:

1. **Project milestones** marked with a hard deadline (`--hard`), of an open (active or parked) project.
2. **Stewardship periodic commitments** (the recurring lines on a stewardship dashboard).
3. **Standalone commitment notes** in `commitments/`.
4. **Action notes** carrying a self-imposed `due:` that isn't pinned to a milestone, of an open (active or parked) project.

A project's own dates (sources 1 and 4) count while the project is open, active or parked: a hard date on a parked map is still a promise. Completing or dropping a project takes its milestones and action due dates out of the view until it is activated again. A standalone commitment that names a parked or closed project keeps surfacing, since it is a promise to someone else.

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
