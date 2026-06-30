# Write tools

Tools that mutate the vault. Each returns a result describing what was written. The same business
rules as the CLI apply (append-only notes, auto-logged project state, the project cap).

## Logging, capture, triage

| Tool | Inputs | Effect |
|------|--------|--------|
| `append_to_log` | `text` | Append a line to today's daily note. ([`cdno log`](../cli/log.md)) |
| `capture` | `text` | Drop a raw note into `inbox/`. ([`cdno capture`](../cli/capture.md)) |
| `discard_inbox_item` | `slug` | Clear a triaged capture (slug from `triage_inbox`). |

## Evidence

| Tool | Inputs | Effect |
|------|--------|--------|
| `file_to_portfolio` | `portfolio`, `source`, `origin`, `content?`, `attach?`, `vars?` | File evidence into a portfolio; `attach` is a server-side path to a non-Markdown artefact (`vars` is ignored on the attach path). ([`cdno file`](../cli/file.md)) |

## Projects, actions, milestones, waiting-on

| Tool | Inputs | Effect |
|------|--------|--------|
| `update_project_state` | `project`, `new_state` | Rewrite the Current State (auto-logs the previous). |
| `add_action` | `project`, `title`, `energy`, `with_note?`, `vars?` | Append a next action; `with_note` also scaffolds a manifest note (`vars` applies only then). |
| `promote_action` | `project`, `query`, `vars?` | Promote a bullet to a manifest note (substring match). |
| `complete_action` | `project`, `query` | Complete an action; archives its note if any. |
| `add_milestone` | `project`, `title`, `target_date`, `hard?` | Add a milestone; `hard` counts it in commitments. |
| `complete_milestone` | `project`, `query` | Complete a milestone (substring match). |
| `add_waiting_on` | `project`, `description` | Add a waiting-on blocker. |
| `resolve_waiting_on` | `project`, `query` | Resolve a waiting-on item (substring match). |

## Commitments and tracking

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_commitment` | `title`, `due`, `context`, `project?`, `stewardship?`, `vars?` | Create a standalone commitment note. |
| `complete_commitment` | `commitment` (slug) | Mark a commitment done and archive it. |
| `create_tracking_entry` | `stewardship`, `activity`, `routine?`, `content?`, `vars?` | File a tracking note under an expanded stewardship. |

## Daily and weekly sections

| Tool | Inputs | Effect |
|------|--------|--------|
| `upsert_daily_section` | `section` (`Standup`\|`Intention`\|`Agenda`\|`Meeting`), `content?`, `date?`, `append?` | Write or append a daily-note section. |
| `upsert_weekly_section` | `section` (`Wins`\|`Challenges`\|`One Improvement`\|`This Week's Goal`), `content?`, `date?`, `append?` | Write or append a weekly-note section. |

## Notes

- `append?` defaults to replacing the section; set it `true` to append instead.
- Dates are `YYYY-MM-DD`; week-scoped tools accept any day in the target week.
- `vars?` is an optional `name -> value` map supplying values for a custom template's
  [`[variables.prompt]`](../../tutorials/templates-and-frontmatter.md) placeholders — the MCP analogue
  of the CLI's repeatable `--var name=value`. Omitting a required prompted variable fails with an
  "unresolved prompts" error. See [Creation and lifecycle tools](creation-and-lifecycle.md) for the
  full list of templated tools that accept it.
- See also: [Creation and lifecycle tools](creation-and-lifecycle.md), [JSON output](../json-output.md).
