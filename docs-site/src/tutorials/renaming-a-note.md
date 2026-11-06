# Renaming a note

A note's slug is its file name, and every `[[wikilink]]` and every `project:` field types it.
Sometimes the slug you got is not the one you want to live with. This tutorial renames a project
whose derived slug stopped mid-title, follows what the rename changes, and covers what to do on a
second machine. The reference is [`cdno note rename`](../reference/cli/note.md#cdno-note-rename-note).

## How you get a bad slug

The slug is derived from the title, and a long title is cut at 50 characters:

```bash
cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs" --context work
# -> projects/calibrate-the-surrogate-model-against-the.md
```

Every later link now has to say `[[calibrate-the-surrogate-model-against-the]]`. The way to avoid
this is to name the note when you create it:

```bash
cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs" \
  --context work --slug surrogate-calibration
```

Every create verb takes `--slug` (see [the slug rule](../reference/cli/overview.md#naming-a-note---slug)),
and an agent creating a note over MCP is told to propose a short one. For a note that already
exists, rename it.

## Preview first

Give the project an action note and start it, so that the journal and the action note both point at
the project:

```bash
cdno action add --project calibrate-the-surrogate-model-against-the --title "Run the baseline sweep" --note --energy deep
cdno action start --project calibrate-the-surrogate-model-against-the --query baseline
```

Then preview the rename:

```bash
cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration --dry-run
```

```text
Dry run: nothing written.
  move     projects/calibrate-the-surrogate-model-against-the.md -> projects/surrogate-calibration.md
  rewrite  actions/run-the-baseline-sweep.md                      1 field (project:)
  rewrite  journal/2026/daily/2026-11-06.md                       3 links
  alias    projects/surrogate-calibration.md gains `calibrate-the-surrogate-model-against-the`
  log      journal/2026/daily/2026-11-06.md
             renamed [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
               was: calibrate-the-surrogate-model-against-the
  4 references in 2 notes · plan 2109…
```

Your plan hash will differ: the log lines carry their time stamps.

Each line is one change: the file moves, two other notes have references corrected (three links in
the journal, from the lines for the project's creation, the action and the start; the `project:`
field of the action note), the project gains an alias, and today's log gets
one entry. Nothing is written. If the preview is refused, it is refused exactly as the write would
be, so a dry run is a real check.

## Write it

```bash
cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration \
  --reason "the derived slug stopped mid-title"
```

With the note and `--to` given, the command writes as soon as it has checked the plan, without a
confirm, in a terminal or not; the dry run was your look, and it is advice, not a lock. Leave out
`--to` in a terminal and you are asked for it, shown the preview and asked to confirm. That confirmed
write is tied to the preview: if a note it would rewrite changed while you decided, it refuses with
`stale_plan` and writes nothing. Over MCP the guard applies when the agent passes the preview's `plan_hash` as `expected_plan`.

## What changed

The project map has a new `aliases:` entry for the old name:

```yaml
aliases: [calibrate-the-surrogate-model-against-the]
```

and the old name still works. `cdno open calibrate-the-surrogate-model-against-the` finds the
project, and says it was found by its former name. `cdno project show` takes it too, and prints the
current slug in its header.

Today's log gains one entry:

```markdown
- **09:44**: renamed [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
  was: calibrate-the-surrogate-model-against-the
  reason: the derived slug stopped mid-title
```

`was:` is plain text on purpose. It records what the note was called, and a link there would be
rewritten by the next rename. Every earlier `state on [[…]]` and `started [[…]]` line in the journal
now names `surrogate-calibration`, so the project's history is still one search, and a link clicked
in an editor lands on the note.

This is the one place the vault edits the past. The log is append-only: what was recorded and when
never changes. A reference is an address, not content, and correcting an address changes neither
(see [the method page](../concepts/rlm.md)). `cdno lint` accepts exactly that and nothing else in an
archived note.

## Renaming back

Rename `old` to `new` and back to `old` and you get `aliases: [new]`. The first `renamed` line's
link is rewritten to `[[old]]` like any other link to the note, and its `was: old` stays as
written; the second reads `renamed [[old]] … was: new`.

## When it is refused

Nothing is written on a refusal, and each has a code (the [table](../reference/cli/note.md#refusals)
says what to do for each). The common ones: the new slug is invalid or taken, the note is a kind that
cannot be renamed (a daily note, an archived action; see
[which notes can be renamed](../concepts/note-types.md#which-notes-can-be-renamed)), or the vault is
behind its upstream.

## The other machine

If the vault is a git repository synced between machines, the other one does nothing special: the
renamed files, the corrected links and the alias arrive through the sync like any edit.

- Rename on a machine that is up to date. A rename refuses with `behind_upstream` while the vault is
  behind, and `--skip-sync-check` overrides that only when you know nothing unpushed on the other
  side touches these notes.
- If the sync stops on a conflict, resolve it, then run `cdno lint`. A link that came back with the
  old name still works through the alias, and lint warns that `[[old]]` names a renamed note, now
  `new`. Leaving it is harmless; `cdno note rename new --relink` rewrites every such link to the new
  name in one go ([relinking the stragglers](../reference/cli/note.md#relinking-the-stragglers---relink)),
  and editing one by hand is allowed too, even in a past log entry.
- Run any `cdno` command in the vault once on every machine, on a cdno that has `note rename`, before
  renaming anything an archived action references;
  [the reference](../reference/cli/note.md#on-the-other-machine) says why and what `cdno lint` says
  otherwise.

## Over MCP

An agent previews with `rename_note { dry_run: true }`, shows you the plan, and writes with the
plan's `plan_hash` as `expected_plan`. See [Renaming a note](../reference/mcp/writes.md#renaming-a-note).
