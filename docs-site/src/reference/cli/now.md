# `cdno now`

What you are in the middle of: the most recent action started and not yet closed.

```text
cdno now [OPTIONS]
```

There is no state behind this. It replays today's `## Logs`, so a start made from
[`cdno action start`](action.md#cdno-action-start), from an agent over MCP, or typed into the daily
note by hand all count — and a completion or a drop clears it. Nothing to keep in sync, and
nothing to go stale.

One verb breaks the pairing: [`cdno action promote`](action.md#cdno-action-promote) *rewrites* the
bullet it matched, so promoting between a start and its close leaves `cdno now` naming the old text
for the rest of the day, and `action complete` and `action drop` then match nothing.

A line you type yourself has to match the shape the writers emit:

```text
- **09:30**: started [[surrogate-model]] — Draft the methods section (deep)
```

Both halves are required. The `- **HH:MM**: ` stamp is what makes the line a log entry at all, and
the separator is an **em dash** (U+2014), not a hyphen. The parser requires that exact codepoint, so
that ordinary prose beginning "started something" is never mistaken for a focus — which also means a
line missing the stamp, or typed with `-`, is silently not picked up, and `cdno lint` will not flag
either.

```bash
$ cdno now
surrogate-model since 09:30 · 1h 30m
  Draft the methods section (deep)
```

With nothing open it says so rather than printing an empty frame:

```bash
$ cdno now
Nothing started yet. `cdno orient` suggests one thing to begin.
```

`--json` emits `{project, action, started}`, all three `null` when nothing is open — so a caller can
test one field without first branching on the shape of the document.

The `action` field is the bullet text exactly as logged, energy suffix and all. That is the same
string [`cdno action complete`](action.md#cdno-action-complete) matches, which is why the pairing
works.

## Related MCP tools

[`current_focus`](../mcp/reads.md) — the same read, for an agent.

## See also

- [`action start`](action.md#cdno-action-start) — what puts something here.
- [`orient`](orient.md) — what to begin when nothing is open.
