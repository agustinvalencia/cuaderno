# Creation and lifecycle tools

Tools that create new notes or move existing ones through their lifecycle.

## Creation

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_project` | `title`, `context`, `core_question?`, `vars?` | Create a project (parked if at the active cap). ([`cdno project create`](../cli/project.md)) |
| `create_portfolio` | `question`, `project?`, `vars?` | Create an evidence portfolio. |
| `create_question` | `domain` (`research`\|`life`), `text`, `vars?` | Create a question note. |
| `create_stewardship` | `name`, `context`, `expanded?`, `vars?` | Create a stewardship; `expanded` adds a `tracking/` folder. |
| `create_custom_note` | `type_name`, `title`, `fields?`, `vars?`, `body?`, `origin?` | Create a note of a **config-defined** custom type (`[note_types.<name>]`); built-in types have their own dedicated create tools, on this page and under [Write tools](writes.md). `fields` is a name → value map of the type's declared frontmatter fields — every `required` one must be present, and each key must be declared. `body` excludes the title heading (the engine writes the H1) and fills the template's `{{body}}` placeholder, or is inserted after the H1; `origin` is one string of wikilinks to where the note came from, always stored as a frontmatter string, and is refused unless the type declares an `origin` field. Creation is logged to today's daily note as `<type> created [[<folder>/<slug>]] — <title>`; a promotion (creation with `origin`) needs no other log line. A second note with the same title is not refused but created as `<slug>-2`, so for a `concept`, search the library (`search_notes` with `note_type: concept`) and refine an existing note with [`revise_note`](writes.md#revising-a-note) instead. Call [`list_note_types`](reads.md) first to discover a vault's types and their fields. ([`cdno note create`](../cli/note.md)) |
| `link_portfolio_to_question` | `portfolio`, `question` | Retrofit a portfolio→question link (backlinks both ways). |
| `link_portfolio_to_project` | `portfolio`, `project` | Retrofit a portfolio→project link (sets `project:` and appends to the project's Links). |

## Lifecycle

| Tool | Inputs | Effect |
|------|--------|--------|
| `park_project` | `project` | Move an active project to `_parked/`. |
| `complete_project` | `project` | Close an active or parked project whose work is done: move it to `_done/<year>/` with `status: completed` and `closed:`. Refused while any action or milestone is open. ([`cdno project complete`](../cli/project.md#cdno-project-complete)) |
| `drop_project` | `project`, `reason?`, `open_items?` (`refuse`\|`drop`), `expected_open_items?` | Close an active or parked project that is not going to happen: `status: dropped`, `closed:`, moved to `_done/<year>/`. Refused with the open items by default; `open_items: "drop"` with the refusal's hash drops them too. ([`cdno project drop`](../cli/project.md#cdno-project-drop)) |
| `activate_project` | `project` | Bring a parked or closed project back, clearing `closed:` (enforces the five-project cap). |
| `set_question_status` | `question`, `status` (`active`\|`parked`\|`answered`\|`retired`) | Transition a question's status. |
| `add_periodic_commitment` | `stewardship`, `title`, `recurrence`, `next_date` | Append a periodic commitment to a stewardship dashboard. |

## Notes

- `context` is one of the fixed [life domains](../../concepts/contexts-and-energy.md).
- `recurrence` follows the [recurrence syntax](../recurrence.md): `daily`, `weekly`, `monthly`,
  `yearly`, or `every N months`.
- `activate_project` enforces the cap — if activating would exceed five active projects, the call
  fails and the assistant must park one first.
- Closing a project never needs a slot, and works on a parked project directly. While an action or
  milestone is open, both closing tools return a `project_has_open_items` rejection whose
  `details` list the `actions`, the `milestones` (with `date` and `hard`), the
  `untouched_commitments` (linked standalone commitments, which no close ever touches) and an
  `open_items_hash`. `complete_project` stays refused until each item is completed or dropped: a
  completion is a claim that the work was done. `drop_project` with `open_items: "drop"` and
  `expected_open_items` set to that hash drops every listed item with the project, each logged with
  `reason: project dropped (<reason>)` (plain `reason: project dropped` when no `reason` was given); if the list has changed since, it is refused again with the
  new list and hash. The result names `dropped_actions`, `dropped_milestones` and
  `untouched_commitments`.
- A closed project can take the other outcome later (`complete_project` on a dropped one, and the
  reverse), re-filed under this year's folder with a new `closed:` date. The same outcome again is a
  `project_not_active` rejection whose `details` carry `status` and `closed`.
- `vars?` is an optional `name -> value` map supplying values for a custom template's
  [`[variables.prompt]`](../../tutorials/templates-and-frontmatter.md) placeholders — the MCP analogue
  of the CLI's repeatable `--var name=value`. Supply an entry for each prompted variable the note's
  template uses that has no static `[variables]` default; otherwise creation fails with an
  "unresolved prompts" error (MCP has no interactive prompt to fall back on).
- See also: [Write tools](writes.md), [Context-gathering tools](reads.md).
