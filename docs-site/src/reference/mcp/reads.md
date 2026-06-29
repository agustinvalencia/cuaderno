# Context-gathering tools

Read-only tools an assistant uses to understand your vault before acting. None of these mutate
anything. Inputs marked optional may be omitted.

| Tool | Inputs | Returns |
|------|--------|---------|
| `get_orientation` | `energy?` (`deep`\|`medium`\|`light`) | Commitments due soon, active projects, and a suggested starting point. The MCP form of [`cdno orient`](../cli/orient.md). |
| `get_project_context` | `project` (slug) | A project's state, next actions, milestones, waiting-on items, and links. |
| `get_portfolio_contents` | `portfolio` (slug) | Portfolio metadata plus its evidence inventory. |
| `get_weekly_context` | `date?` (any day in the week) | The weekly note's sections (Wins, Challenges, One Improvement, This Week's Goal). |
| `get_monthly_context` | `date?` | Monthly context for a strategic scan. |
| `get_stewardship_tracking` | `stewardship`, `activity`, `period?` (e.g. `30d`, `6m`) | Tracking entries for a stewardship/activity over a window. |
| `get_active_questions` | `domain?` (`research`\|`life`) | Active question notes, optionally filtered by domain. |
| `get_commitments` | `lookahead_weeks?` (default 2) | The aggregated commitments view; overdue always included. |
| `list_projects` | — | All projects (active + parked) with summaries. |
| `read_daily_note` | `date?` (default today) | The daily log for a date. |
| `read_weekly_note` | `date?` (default this week) | The weekly note for an ISO week. |
| `search_notes` | `query`, `note_type?`, `from?`, `to?`, `portfolio?`, `limit?` (default 20) | Ranked full-text hits. The MCP form of [`cdno search`](../cli/search.md). |
| `lint` | — | Frontmatter problems across the vault. |
| `triage_inbox` | — | Pending inbox captures awaiting triage. |

## Notes

- **Dates** are `YYYY-MM-DD`. Week-scoped tools accept *any* day within the target ISO week.
- **`search_notes`** returns the same hit shape as the CLI: `path`, `note_type`, `title`, `snippet`,
  `score`. See [JSON output](../json-output.md).
- These pair naturally with the [write tools](writes.md): read context, propose an action, then
  write it.
