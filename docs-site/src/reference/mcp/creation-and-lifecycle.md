# Creation and lifecycle tools

Tools that create new notes or move existing ones through their lifecycle.

## Creation

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_project` | `title`, `context`, `core_question?` | Create a project (parked if at the active cap). ([`cdno project create`](../cli/project.md)) |
| `create_portfolio` | `question`, `project?` | Create an evidence portfolio. |
| `create_question` | `domain` (`research`\|`life`), `text` | Create a question note. |
| `create_stewardship` | `name`, `context`, `expanded?` | Create a stewardship; `expanded` adds a `tracking/` folder. |
| `link_portfolio_to_question` | `portfolio`, `question` | Retrofit a portfolio→question link (backlinks both ways). |
| `link_portfolio_to_project` | `portfolio`, `project` | Retrofit a portfolio→project link (sets `project:` and appends to the project's Links). |

## Lifecycle

| Tool | Inputs | Effect |
|------|--------|--------|
| `park_project` | `project` | Move an active project to `_parked/`. |
| `activate_project` | `project` | Bring a parked project back (enforces the five-project cap). |
| `set_question_status` | `question`, `status` (`active`\|`parked`\|`answered`\|`retired`) | Transition a question's status. |
| `add_periodic_commitment` | `stewardship`, `title`, `recurrence`, `next_date` | Append a periodic commitment to a stewardship dashboard. |

## Notes

- `context` is one of the fixed [life domains](../../concepts/contexts-and-energy.md).
- `recurrence` follows the [recurrence syntax](../recurrence.md): `daily`, `weekly`, `monthly`,
  `yearly`, or `every N months`.
- `activate_project` enforces the cap — if activating would exceed five active projects, the call
  fails and the assistant must park one first.
- See also: [Write tools](writes.md), [Context-gathering tools](reads.md).
