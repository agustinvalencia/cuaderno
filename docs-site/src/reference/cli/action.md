# `cdno action`

Manage a project's next actions: add (optionally as a manifest note), promote a bullet to a note,
start, pause, switch, resume, complete, drop, and list.

```text
cdno action [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`add`](#cdno-action-add) | Append a next action to a project |
| [`start`](#cdno-action-start) | Log that work on an action is starting |
| [`pause`](#cdno-action-pause) | Set the action in focus aside without finishing it |
| [`switch`](#cdno-action-switch) | Pause the action in focus and start another, in one step |
| [`resume`](#cdno-action-resume) | Pick a carried or paused action up again |
| [`promote`](#cdno-action-promote) | Promote a plain bullet to a wikilinked manifest note |
| [`complete`](#cdno-action-complete) | Mark an action done by substring match |
| [`drop`](#cdno-action-drop) | Close an action **without** recording it as done |
| [`list`](#cdno-action-list) | List a project's open actions |

Write subcommands honour `--json` (`{path, message}`, non-interactive); `list` emits its data under
`--json`. A refused `start`, `pause`, `resume` or `switch` prints a rejection object instead; see
[When `start` is refused](#when-start-is-refused).

`start`, `pause`, `switch` and `resume` are the *focus verbs*: they move the one slot that
[`cdno now`](now.md) reads. See [Focus](../../concepts/contexts-and-energy.md#focus) for the idea
and [the log markers they write](#what-the-focus-verbs-write) below.

---

## `cdno action add`

Append a next action to a project. `--note` also scaffolds a manifest note and wikilinks the bullet.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--title <TITLE>` | Action title. |
| `--energy <ENERGY>` | `deep`, `medium`, or `light`. |
| `--note` | Also create a manifest note alongside the bullet and wikilink it. |
| `--var <NAME=VALUE>` | Value for a custom action-note template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Only applies with `--note` (a plain bullet isn't templated). See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno action add --project surrogate-model --title "Profile the assembly step" --energy medium
cdno action add --project surrogate-model --title "Characterise sample efficiency" --energy deep --note
```

## `cdno action promote`

Promote an existing plain bullet to a wikilinked manifest note. Substring-matches the bullet text;
energy is inherited.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Case-insensitive substring of the bullet text. |
| `--var <NAME=VALUE>` | Value for a custom action-note template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Promotion scaffolds an action note, so it gathers the same prompts as `add --note`. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno action promote --project surrogate-model --query "profile the assembly"
```

## `cdno action start`

Log that work on an action is starting: writes `started [[slug]] — <bullet>` to today's daily note,
which is what [`cdno now`](now.md) reads back.

The action must already be on the map. A start *names a bullet*, so that the later completion logs
matching text and the focus clears — a start naming nothing could never be closed. What gets logged
is the **resolved** bullet text, not your query, so `--query "draft methods"` logs
`Draft methods (deep)`.

[`cdno action promote`](#cdno-action-promote) *rewrites* the bullet it matched, and the focus
follows it: [`cdno now`](now.md) switches to the new note (`[[actions/<slug>]] (deep)`), keeps the
original start time, and `complete` and `drop` still pair with the start.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Substring of an existing bullet. Conflicts with `--unplanned`. |
| `--unplanned` | Start work that is on no map yet: adds the bullet, then starts it. |
| `--title <TEXT>` | Title for the new bullet. Requires `--unplanned`. |
| `--energy <LEVEL>` | `deep`, `medium` or `light`. Requires `--unplanned`. |

### When `start` is refused

Focus is one slot, so `start` while another action is in focus is refused, naming the open one and
the way forward:

```text
$ cdno action start --project other-project --query "CI"
Error: Draft the methods section is already in focus on surrogate-model (since 09:30).
To move on to this instead: cdno action switch --project other-project --query CI
Or complete or pause it first.
```

You can always move on; you just say so. In a terminal the refusal is followed by `Switch to it
instead?` (default no); `y` runs the switch without asking for a hint or a reason. Piped, with
`--no-interactive` or with `--json`, it fails fast instead.

Starting the action that *is* the focus is refused too. If it was started today there is nothing to
do. If it was carried over from an earlier day the message points at
[`cdno action resume`](#cdno-action-resume). A typo or an ambiguous `--query` is reported as
such, not as a refusal, and a refused `--unplanned` start adds no bullet.

With `--json` the refusal is the same object the MCP server returns, on stdout, with a non-zero exit:

```json
{
  "code": "focus_open",
  "message": "An action is already in focus. Ask the person before switching; do not retry.",
  "details": {
    "focus": {"project": "surrogate-model", "action": "Draft the methods section (deep)",
              "started": "09:30", "date": "2026-10-04", "carried": false},
    "attempted": {"project": "other-project", "query": "CI"},
    "same_action": false,
    "remedy": "switch_action"
  }
}
```

`remedy` is `switch_action` when something else is in focus, `already_focused` for the same action
started today, and `resume_action` for the same action carried over. `attempted` is
`{project, query}`, or `{project, title}` for an `--unplanned` start.

An ambiguous `--query` is a question rather than a dead end: in a terminal you get a picker over the
candidates, and non-interactively they are listed one per line. The same holds for
[`complete`](#cdno-action-complete), [`drop`](#cdno-action-drop) and
[`promote`](#cdno-action-promote), which resolve through the same matcher.

One case no picker can settle: two open bullets whose text differs only in case, or not at all.
The domain's whole-bullet tiebreak compares case-insensitively, so it sees two exact matches,
declines, and picking either re-ambiguates. Edit one of the bullets to tell them apart.

```bash
# start something already planned
cdno action start --project surrogate-model --query "feature set B"

# start something that was never planned — adds the bullet and starts it
cdno action start --project surrogate-model --unplanned \
    --title "Fix the CI badge" --energy light
```

`--unplanned` is deliberately explicit rather than a fallback when `--query` matches nothing: a
fallback would turn every typo into a new action, silently. `--title` and `--energy` require it, so
passing them alone is a parse error naming `--unplanned` rather than a start on some other bullet.

## `cdno action pause`

Set the action in focus aside without finishing it. It acts on the **current focus** and takes no
project or query: there is exactly one focus, and it is the one paused. Nothing is looked up on the
project map, so a focus on a project you have since parked can still be paused, and the bullet stays
where it is.

| Flag | Description |
|------|-------------|
| `--next <TEXT>` | Where to pick up again: the re-entry hint `resume` and `cdno now` read back. In a terminal, asked for once when absent (`Enter` skips); no confirmation follows. |
| `--reason <TEXT>` | Why the work was set aside. Never prompted for. |

```bash
cdno action pause --next 'pick up at "Prior approaches"' --reason "CI is red"
```

With nothing in focus it says `Nothing started — nothing to pause.` and exits non-zero. A pause is
not a drop: a paused action stays open on the map, and can be completed or dropped later as usual.

## `cdno action switch`

Pause the action in focus and start another, in one commit. It takes the same target flags as
[`start`](#cdno-action-start), plus the hint and reason for the action being set aside.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project of the action to switch **to**. |
| `--query <QUERY>` | Substring of the open bullet to switch to. Conflicts with `--unplanned`. |
| `--unplanned` | Switch to work on no map yet: adds the bullet, then starts it. |
| `--title <TEXT>` | Title for the new bullet. Requires `--unplanned`. |
| `--energy <LEVEL>` | `deep`, `medium` or `light`. Requires `--unplanned`. |
| `--next <TEXT>` | Re-entry hint for the action being paused. Asked for once in a terminal when absent (`Enter` skips). |
| `--reason <TEXT>` | Why the focus moved. Never prompted for. |

```bash
cdno action switch --project other-project --query "CI" \
    --next "related-work paragraph half done" --reason "a collaborator is blocked on it"
```

A switch is a pause followed by a start in the log; there is no separate marker. If the target does
not resolve, nothing is written, not even the pause. With nothing in focus it is a plain start, and a
`--next` it had nothing to attach to is reported rather than dropped. A prompted switch shows `pause
<X>, start <Y>` and asks to confirm, and the picker leaves out the bullet that is already in focus.

## `cdno action resume`

Pick work up again. It takes **no query or title**: the text comes from the log, not from the map.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Restrict to this project: its carried focus, else its most recent pause. |

```bash
$ cdno action resume
Resumed on surrogate-model, logged to journal/2026/daily/2026-10-04.md
next: pick up at "Prior approaches"
```

Without `--project` it resumes the focus carried over from an earlier day if there is one, else the
most recent pause within [`paused_lookback_days`](../configuration.md#focus) (default 14). It writes
`resumed [[slug]] — <text>`, which re-anchors the focus to now, so a Tuesday start you keep working on
does not expire on Thursday, and prints the pause's `next:` hint when it has one. A pause that a later
start, resume, completion, drop or promotion of the same action followed is no longer offered.

With nothing to pick up it says `Nothing to resume — nothing is carried over or paused.` (`Nothing to
resume on <project> — …` with `--project`) and exits non-zero. It is refused with a `focus_open` refusal (the same object as above, without `attempted`) when a different action is in focus (pause it or `switch`
instead; resume never switches for you) or when the same action was already started today.
With `--json` a success carries `path`, `message` and `resumed_from`: `kind` (`carried` or
`paused`), `date`, `next` and `reason`.

## What the focus verbs write

Each verb is one entry in today's `## Logs`. `next:` and `reason:` are indented continuation lines,
written only when given:

```text
- **10:40**: action paused on [[surrogate-model]] — Draft the methods section (deep)
  next: pick up at "Prior approaches"
  reason: a collaborator is blocked on CI
- **10:40**: started [[other-project]] — Fix red CI on main (medium)
- **14:05**: resumed [[surrogate-model]] — Draft the methods section (deep)
```

`paused` closes the focus the way `action done on` and `action dropped on` do; `resumed` opens it again.
`complete` and `drop` are unchanged, including on a paused action. When a focus is open, a log line
written by [`cdno log`](log.md#focus-tag), [`cdno capture`](capture.md) or the triage verbs carries a
`during:` tag naming its project.

## `cdno action complete`

Mark a next action completed by case-insensitive substring match. A wikilinked bullet also archives
its note to `actions/_done/<year>/`.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Substring of the bullet text. |

```bash
cdno action complete --project surrogate-model --query "feature set B"
```

## `cdno action drop`

Close a next action **without recording it as done** — for work that was superseded, abandoned or
reprioritised. Matches the bullet exactly as `complete` does; a wikilinked bullet also archives its
note to `actions/_done/<year>/`, stamped `status: dropped` with no completion date.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Substring of the bullet text. |
| `--reason <TEXT>` | Optional: why it was dropped. |

```bash
cdno action drop --project surrogate-model --query "demo proposal" \
    --reason "superseded by the demo-planning action"
```

Use this rather than `complete` whenever the work was not actually performed. `complete` writes
`action done on [[…]]` into the daily log, which is the record the weekly review, the monthly scan
and every later verdict read back from — so completing something that never happened leaves the
vault asserting work nobody did, and the only repair is a correction line written by hand.

`--reason` is optional but worth giving: "superseded by X" and "no longer wanted" are different
facts, and only one of them tells a later reader to go looking for the replacement. It is recorded
on a continuation line under the log entry.

A dropped action does **not** appear in the completed-actions views that the weekly and monthly
reviews build, because it carries no completion date.

## `cdno action list`

List a project's open action bullets, with attached-note status (active / blocked / completed / dropped) inline
when present. Honours `--json`.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |

```bash
cdno action list --project surrogate-model
cdno action list --project surrogate-model --json
```

## Related MCP tools

[`add_action`](../mcp/writes.md), [`promote_action`](../mcp/writes.md),
[`start_action`](../mcp/writes.md), [`start_unplanned_action`](../mcp/writes.md),
[`pause_action`](../mcp/writes.md#focus-tools), [`switch_action`](../mcp/writes.md#focus-tools),
[`switch_unplanned_action`](../mcp/writes.md#focus-tools), [`resume_action`](../mcp/writes.md#focus-tools),
[`complete_action`](../mcp/writes.md), [`drop_action`](../mcp/writes.md). (Open actions are also visible via
[`get_project_context`](../mcp/reads.md); what is currently started via
[`current_focus`](../mcp/reads.md).)

## See also

- [Actions](../../tutorials/actions.md).
- [`now`](now.md) — what is currently started.
- [`project`](project.md).
