# Context-gathering tools

Read-only tools an assistant uses to understand your vault before acting. None of these mutate
anything. Inputs marked optional may be omitted.

| Tool | Inputs | Returns |
|------|--------|---------|
| `get_orientation` | `energy?` (`deep`\|`medium`\|`light`) | Commitments due soon (an active project's milestones and action dates, standalone commitments, stewardship lines), active projects, lapsed stewardship habits, and a suggested starting point. Also `focus` (the [`current_focus`](#focus-fields) shape, or `null`) and, on each project, `last_paused` (its most recent unresolved pause, or `null`) beside `top_action`. The MCP form of [`cdno orient`](../cli/orient.md), which does not render those two. |
| `get_project_context` | `project` (slug) | A project's state, next actions, milestones, waiting-on items, and links. Resolves active, parked and closed (`_done/<year>/`) projects. Also `last_paused`, as on `get_orientation`, so the re-entry hint is there when the project is opened and not only in the morning. |
| `get_portfolio_contents` | `portfolio` (slug) | Portfolio metadata plus its evidence inventory. |
| `get_weekly_context` | `date?` (any day in the week) | The weekly note's sections (Wins, Challenges, One Improvement, This Week's Goal), plus the week's logs, `completed_actions`, the projects completed or dropped that week (`closed_projects`) and project state changes. |
| `get_monthly_context` | `date?` | Monthly context for a strategic scan, including the past 30 days' `completed_actions` as wins patterns, the projects completed or dropped in those 30 days (`closed_projects`), and every parked project (`parked_projects`). |
| `get_stewardship_tracking` | `stewardship`, `activity`, `period?` (e.g. `30d`, `6m`) | Tracking entries for a stewardship/activity over a window, plus the activity's declared contract in `spec` (record key, group field, and each metric's type, unit, aggregate, and `derived` expression when it is computed rather than recorded — write that metric's operands, never a field of its own name) when the vault declares one under [`[tracking.<activity>]`](../configuration.md#tracking). `spec` is null for an undeclared activity. Also returns `series`: this activity's numeric trends, one per `(group, metric)` it declares, each point already reduced by that metric's own aggregate, scoped and windowed by the same `activity` and `period` as the entries. |
| `get_active_questions` | `domain?` (`research`\|`life`) | Active question notes, optionally filtered by domain. |
| `get_commitments` | `lookahead_weeks?` (default 2) | The aggregated commitments view; overdue always included. A project's milestones and action due dates appear only while it is active; a standalone commitment appears whatever its project's status. |
| `current_focus` | — | The action most recently started or resumed, unless it has since been completed, dropped or paused (focus is one slot), or `null`. Returns `project`, `action`, `started`, `date`, `carried` and `origin` ([below](#focus-fields)). Replayed from the `## Logs` of today and the `[focus] carry_over_days` days before it (default 1), so a start made from the CLI counts too, as does one written by hand in the log's full shape (`- **HH:MM**: started [[slug]] — text` — stamp and em dash U+2014 both required; see [`cdno now`](../cli/now.md)); a completion, a drop or a pause clears it, and a `promote_action` in between renames it to the new note, keeping the start time. The MCP form of [`cdno now`](../cli/now.md). |
| `list_projects` | `include_closed?` (default false) | Active and parked projects with their frontmatter, plus the slot budget; with `include_closed: true`, completed and dropped projects too, under `closed`. |
| `list_note_types` | — | Every note type — the built-ins plus any config-defined `[note_types.*]` custom type — with its folder, required/optional fields, typed `[schemas.*]` field specs, template, and supplied placeholders. Call before `create_custom_note` to discover a vault's custom types. |
| `read_daily_note` | `date?` (default today) | The daily log for a date. |
| `read_weekly_note` | `date?` (default this week) | The weekly note for an ISO week. |
| `read_monthly_note` | `date?` (default this month) | The monthly note for a calendar month. |
| `read_note` | `note` (path, slug, `type:slug` or journal date) | Any note, whole: `path`, `note_type`, `frontmatter`, `body` (uncapped), `content_hash`, `backlinks` and `headings`. Takes the references [`cdno open`](../cli/open.md) takes, plus a path without its `.md`. |
| `search_notes` | `query`, `note_type?`, `from?`, `to?`, `portfolio?`, `limit?` (default 20) | Ranked full-text hits. The MCP form of [`cdno search`](../cli/search.md). |
| `lint` | — | Vault-wide problems: frontmatter, broken wikilinks, attachment pairing, a project whose status disagrees with its folder, a custom template that lacks a key its built-in has gained, and lines the canonical parsers silently skip — malformed stewardship-dashboard bullets and daily-log focus markers (`started`, `paused`, `resumed`, `action promoted on`) [`cdno now`](../cli/now.md) will not read back. |
| `triage_inbox` | — | Pending inbox captures awaiting triage. |

## Focus fields

`current_focus`, the `focus` field of `get_orientation` and the `focus` field every
[focus-carrying write result](writes.md#focus-on-write-results) share one shape:

```json
{"project": "surrogate-model", "action": "Draft the methods section (deep)",
 "started": "08:50", "date": "2026-10-04", "carried": false,
 "origin": {"started_at": "2026-10-03T14:05"}}
```

| Field | Meaning |
|-------|---------|
| `project` | Project slug. |
| `action` | The bullet text exactly as logged, energy suffix and all: the string `complete_action` expects back. After a `promote_action` it is the new note's link, `[[actions/<slug>]] (energy)`. |
| `started` | `HH:MM` of the open marker (the `started`, or the `resumed` that re-anchored it). |
| `date` | The day of that marker, `YYYY-MM-DD`. |
| `carried` | `true` when `date` is not today: the focus was left open on an earlier day. |
| `origin` | `{started_at}` of the earlier start a `resume` continued, else `null`. A resume after a pause has none. |

`null` means nothing is open, wherever it appears.

`last_paused`, on `get_orientation`'s projects and on `get_project_context`:

```json
{"project": "surrogate-model", "action": "Draft the methods section (deep)",
 "title": "Draft the methods section", "at": "2026-10-04T11:13", "date": "2026-10-04",
 "next": "pick up at \"Prior approaches\"", "reason": "lunch"}
```

It is the project's most recent pause that no later `started`, `resumed`, completion, drop or
promotion of the same action followed, looked for in the last
[`paused_lookback_days`](../configuration.md#focus) days; `next` and `reason` are `null` when the pause
had none. Quote `next` back to the person when they pick the project up. The server instructions
tell an agent how to use the focus; see [Write tools](writes.md#focus-tools).

## Notes

- **Dates** are `YYYY-MM-DD`. Week-scoped tools accept *any* day within the target ISO week;
  month-scoped tools accept *any* day within the target calendar month.
- **`search_notes`** returns the same hit shape as the CLI: `path`, `note_type`, `title`, `snippet`,
  `score`. See [JSON output](../json-output.md).
- **`completed_actions`** (weekly and monthly) covers both forms an action takes. One with its own
  note carries `slug` and `path` alongside `source: "note"`; an inline bullet — the *default* form,
  and so the common case — carries `source: "bullet"` with both null. **A client must expect null
  there.** A completion that has both a note and a log line is listed once, and dropped actions
  never appear. The list is read from the daily notes directly rather than from the capped `logs`
  field, so a completion early in a busy week is not lost to that cap.
- **`read_note`** reads any note, of any type. `content_hash` is the hash of the bytes just read,
  the value to hand back as `expected_hash` when revising the note, so a change made in between is
  detected rather than overwritten. `backlinks` comes from the index, which every cuaderno write
  updates; a link made by editing a file elsewhere appears once the index next reconciles. A slug several notes share is
  refused with code `ambiguous_slug` and the candidates' paths in `details.candidates`; a
  reference matching nothing is refused with code `not_found`. `headings` lists every heading in
  the body with inline markup stripped, the form `revise_note`'s `section` takes. To follow a
  heading link such as a concept's `origin` (`[[journal/2026/daily/2026-09-02#Woodbury
  identity]]`), pass the part before the `#` and find the entry under that heading: the whole link,
  anchor included, is refused as `not_found`.
- These pair naturally with the [write tools](writes.md): read context, propose an action, then
  write it.
