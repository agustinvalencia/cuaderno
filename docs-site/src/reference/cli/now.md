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

`cdno now` has three shapes.

**Something is in focus.** The line above. A start carried over from an earlier day names it
(`since Saturday 14:05 (21h 7m)`), and the elapsed time counts across midnight.

**A carried focus that was picked up again.** A focus carried over from an earlier day and
re-anchored with [`action resume`](action.md#cdno-action-resume) reads the new time first and keeps the original:

```bash
$ cdno now
On surrogate-model — Draft the methods section (deep), picked up 08:50 today (started Saturday 14:05).
```

Resuming a *pause* shows the plain `since HH:MM` shape, because the pause had already closed the
earlier start.

A day more than six days back is shown as a date (`2026-09-22 14:05`), since a weekday name would be
ambiguous.

**Nothing in focus.** It says so, and shows the most recent pause that nothing has picked up since,
with its re-entry hint:

```bash
$ cdno now
Nothing started.
Last paused: surrogate-model — Draft the methods section (10:40), next: pick up at "Prior approaches"
```

The pause is looked for in the last [`paused_lookback_days`](../configuration.md#focus) days
(default 14), so a Friday pause is still there on Monday. Pick it up with
[`cdno action resume`](action.md#cdno-action-resume).

### `--line`

`--line` prints one sanitised line of at most 160 characters for a prompt segment or hook:

```text
Focus: surrogate-model — Draft the methods section (since 09:30)
Focus: none (last paused: surrogate-model — Draft the methods section, next: pick up at "Prior approaches")
```

Control characters are stripped and an over-long line is cut. It prints nothing and exits 0 when no
vault is found or anything goes wrong, so it can sit in a prompt without ever breaking it. With
`--json` as well, `--line` wins. The [Claude Code hook example](https://github.com/agustinvalencia/cuaderno/tree/main/examples/hooks/claude-code)
runs it before every prompt.

### `--json`

| Key | Meaning |
|-----|---------|
| `project`, `action` | The focus's project slug and its bullet text exactly as logged (energy suffix and all). |
| `title` | The bullet text without link and energy suffix, for display. For an attached note it is the last segment of the note's slug. |
| `note` | The attached action note's path (`actions/<slug>`), or `null` for an inline bullet. |
| `energy` | `deep`, `medium` or `light`. |
| `started` | `HH:MM` of the open marker. Kept for compatibility. |
| `started_at`, `date` | The marker's full datetime (`2026-10-04T09:30`) and its day. |
| `carried` | `true` when `date` is not today. |
| `origin` | `{started_at}` of the earlier start a `resume` continues, else `null`. |
| `elapsed_minutes` | Minutes since `started_at`, across midnight. |
| `last_paused` | The most recent unresolved pause: `project`, `action`, `title`, `at`, `next`, `reason`. Present whether or not something is in focus; `null` when there is none. |

Every key is always there and is `null` when it has no value (all of them but `last_paused` when
nothing is in focus), so a caller can test one field without first branching on the shape of the
document.

The `action` field is the bullet text exactly as logged, energy suffix and all. That is the same
string [`cdno action complete`](action.md#cdno-action-complete) matches, which is why the pairing
works.

## Related MCP tools

[`current_focus`](../mcp/reads.md) — the same read, for an agent.

## See also

- [Focus](../../concepts/contexts-and-energy.md#focus) — the idea behind the slot and the window.
- [`action start`](action.md#cdno-action-start) — what puts something here;
  [`pause`](action.md#cdno-action-pause), [`switch`](action.md#cdno-action-switch) and
  [`resume`](action.md#cdno-action-resume) move it.
- [`orient`](orient.md) — what to begin when nothing is open.
