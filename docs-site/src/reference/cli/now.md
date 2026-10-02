# `cdno now`

What you are in the middle of: the action most recently started, unless it has since been completed,
dropped or paused. Focus is one slot: starting something new displaces the old start, which never
comes back.

```text
cdno now [OPTIONS]
```

There is no state behind this. It replays the `## Logs` of today and of the
`[focus] carry_over_days` days before it (default 1, so a start left open yesterday is still your
focus this morning), so a start made from
[`cdno action start`](action.md#cdno-action-start), from an agent over MCP, or typed into the daily
note by hand all count — and a completion, a drop or a pause clears it. Nothing to keep in sync, and
nothing to go stale.

[`cdno action promote`](action.md#cdno-action-promote) *rewrites* the bullet it matched, and the
focus follows it: the `action promoted on` line it logs renames the open start to the new note, with
the original start time, so `action complete` and `action drop` still close it.

A line you type yourself has to match the shape the writers emit:

```text
- **09:30**: started [[surrogate-model]] — Draft the methods section (deep)
```

Both halves are required. The `- **HH:MM**: ` stamp is what makes the line a log entry at all, and
the separator is an **em dash** (U+2014), not a hyphen. The parser requires that exact codepoint, so
that ordinary prose beginning "started something" is never mistaken for a focus. That strictness
means a line missing the stamp, or typed with `-`, is simply not picked up here — but
[`cdno lint`](lint.md) reports it, naming the line and the likely cause, so a near-miss does not stay
invisible.

```bash
$ cdno now
On surrogate-model — Draft the methods section (deep), since 09:30 (1h 30m).
```

A start carried over from an earlier day names it (`since Monday 14:05 (18h 55m)`), and one
re-anchored with `action resume` reads `picked up 08:50 today (started Monday 14:05)`. With nothing
open it says so, and shows the most recent open pause:

```bash
$ cdno now
Nothing started.
Last paused: surrogate-model — Draft the methods section (10:40), next: pick up at "Prior approaches"
```

`--line` prints one sanitised line of at most 160 characters for a prompt segment or hook —
`Focus: surrogate-model — Draft the methods section (since 09:30)`, or `Focus: none (last paused:
…)`. It prints nothing and exits 0 when no vault is found or anything goes wrong.

`--json` emits `project`, `action`, `title`, `note`, `energy`, `started`, `started_at`, `date`,
`carried`, `origin`, `elapsed_minutes` and `last_paused`. Every key is always there and is `null` when
it has no value (all of them but `last_paused` when nothing is open), so a caller can test one field
without first branching on the shape of the document.

The `action` field is the bullet text exactly as logged, energy suffix and all. That is the same
string [`cdno action complete`](action.md#cdno-action-complete) matches, which is why the pairing
works.

## Related MCP tools

[`current_focus`](../mcp/reads.md) — the same read, for an agent.

## See also

- [`action start`](action.md#cdno-action-start) — what puts something here.
- [`orient`](orient.md) — what to begin when nothing is open.
