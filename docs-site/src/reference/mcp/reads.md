# Context-gathering tools

Read-only tools an assistant uses to understand your vault before acting. None of these mutate
anything. Inputs marked optional may be omitted.

| Tool | Inputs | Returns |
|------|--------|---------|
| `get_orientation` | `energy?` (`deep`\|`medium`\|`light`) | Commitments due soon, active projects, lapsed stewardship habits, and a suggested starting point. The MCP form of [`cdno orient`](../cli/orient.md). |
| `get_project_context` | `project` (slug) | A project's state, next actions, milestones, waiting-on items, and links. |
| `get_portfolio_contents` | `portfolio` (slug) | Portfolio metadata plus its evidence inventory. |
| `get_weekly_context` | `date?` (any day in the week) | The weekly note's sections (Wins, Challenges, One Improvement, This Week's Goal), plus the week's logs, `completed_actions` and project state changes. |
| `get_monthly_context` | `date?` | Monthly context for a strategic scan, including the past 30 days' `completed_actions` as wins patterns. |
| `get_stewardship_tracking` | `stewardship`, `activity`, `period?` (e.g. `30d`, `6m`) | Tracking entries for a stewardship/activity over a window, plus the activity's declared contract in `spec` (record key, group field, and each metric's type, unit, aggregate, and `derived` expression when it is computed rather than recorded — write that metric's operands, never a field of its own name) when the vault declares one under [`[tracking.<activity>]`](../configuration.md#tracking). `spec` is null for an undeclared activity. Also returns `series`: this activity's numeric trends, one per `(group, metric)` it declares, each point already reduced by that metric's own aggregate, scoped and windowed by the same `activity` and `period` as the entries. |
| `get_active_questions` | `domain?` (`research`\|`life`) | Active question notes, optionally filtered by domain. |
| `get_commitments` | `lookahead_weeks?` (default 2) | The aggregated commitments view; overdue always included. |
| `current_focus` | — | The action started and not yet closed, or `null`. Replayed from today's `## Logs`, so a start made from the CLI counts too, as does one written by hand in the log's full shape (`- **HH:MM**: started [[slug]] — text` — stamp and em dash U+2014 both required; see [`cdno now`](../cli/now.md)); a completion or a drop clears it, but a `promote_action` in between strands it. The MCP form of [`cdno now`](../cli/now.md). |
| `list_projects` | — | All projects (active + parked) with summaries. |
| `list_note_types` | — | Every note type — the built-ins plus any config-defined `[note_types.*]` custom type — with its folder, required/optional fields, typed `[schemas.*]` field specs, template, and supplied placeholders. Call before `create_custom_note` to discover a vault's custom types. |
| `read_daily_note` | `date?` (default today) | The daily log for a date. |
| `read_weekly_note` | `date?` (default this week) | The weekly note for an ISO week. |
| `read_monthly_note` | `date?` (default this month) | The monthly note for a calendar month. |
| `search_notes` | `query`, `note_type?`, `from?`, `to?`, `portfolio?`, `limit?` (default 20) | Ranked full-text hits. The MCP form of [`cdno search`](../cli/search.md). |
| `lint` | — | Vault-wide problems: frontmatter, broken wikilinks, attachment pairing, and lines the canonical parsers silently skip — malformed stewardship-dashboard bullets and daily-log focus markers [`cdno now`](../cli/now.md) will not read back. |
| `triage_inbox` | — | Pending inbox captures awaiting triage. |

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
- These pair naturally with the [write tools](writes.md): read context, propose an action, then
  write it.
