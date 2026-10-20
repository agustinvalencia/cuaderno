---
name: weekly-review
description: An ADHD-friendly weekly review — celebrate the week's wins first, scan stewardship habits, review project movement, look at the next two weeks of commitments, and pick ONE main goal for next week. No shame, no overwhelm. Use when the user wants to do their weekly review, reflect on the week, or says "weekly review".
metadata:
  author: cuaderno
  version: "1.2"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Weekly Review

A 5–10 minute reflective pass on the week. Wins first, always. Surface what moved, gently flag what stalled, look just far enough ahead, and leave with ONE goal — not a sprawling plan.

**Principles**: Celebrate wins first · One thing at a time · Compassionate accountability · No shame ([guide](../references/ADHD-PRINCIPLES.md))
**Linking**: Use `[[wikilinks]]` for projects/actions referenced in logged content ([rules](../references/LINKING-RULES.md))
**Logging**: Log checkpoints to the daily as they happen — atomic, not batched.

## Surface notes (read before editing this skill)

- `get_weekly_context` returns the week's picture: `week_of` (the ISO Monday), `logs` (daily-log lines), `completed_actions` (the wins list), `state_changes` (project states that moved this week), `commitments` (next two weeks), and `projects`: every active project, handed-off ones included.
- **Each `projects[]` entry says which question to ask it.** `{ slug, held_by, counts, waiting_on_empty, core_question, changed_this_week, focus_days_while_held }`. `counts` is true for the person's own project (it holds a slot) and false for a handed-off one; branch on it, never on the name. `held_by` is null unless the map names someone else. `changed_this_week` is true when the map file was modified this ISO week (it can over-report: a tool that touches the file counts). `focus_days_while_held` is, for a handed-off map, the number of days this week with work started or resumed on it, and 0 otherwise. No log line records who did the work, so it is evidence for the person to weigh, never a verdict.
- **The cap is read, never remembered.** `get_orientation.slots` is `{ active, counted, cap }`. If taking a project back or activating one comes up, quote `slots.counted` and `slots.cap`. `hand_off_project(project, take_back: true)` on an active map is refused with `project_cap_reached` when the counted projects are at the cap; the refusal lists the projects to choose from, and the choice is the person's.
- The **stewardship scan** uses `get_orientation.lapsed_habits` (which stewardships have lapsed) — optionally `get_stewardship_tracking(stewardship, activity, period)` for detail.
- **The review persists to the weekly note.** `upsert_weekly_section(section, content, date?, append?)` writes a section of the ISO-week note for `date` (design §5.2). Each week's anchoring goal lives in **that week's own note** under `This Week's Goal`, so the review splits its writes: `Wins`, `Challenges`, and `One Improvement` record **this** week's note (omit `date`), while the ONE forward goal lands in **next** week's note's `This Week's Goal` — pass a `date` in next week (the Monday after `week_of`, i.e. `week_of` + 7 days). It becomes that week's anchor, ready for weekly-planning to refine. `append: false` (default) composes the section. `read_weekly_note(date?)` checks whether a week already has a note before you start. (The former `Next Week's Focus` is a deprecated alias that maps to `This Week's Goal` — use the canonical name.) Still drop a one-line recap to **today's daily** via `append_to_log`, and apply any decided changes via `update_project_state` / `park_project` / `activate_project` / `hand_off_project`. The conversation is the review; the note is its record.

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `get_weekly_context` | cdno-mcp | The week: logs, completed actions (wins), state changes, 2-week commitments |
| `get_orientation` | cdno-mcp | Lapsed stewardship habits, active project slate, `slots` |
| `get_stewardship_tracking` | cdno-mcp | Optional detail on a specific stewardship |
| `read_weekly_note` | cdno-mcp | Check whether the week already has a note before composing |
| `upsert_weekly_section` | cdno-mcp | Persist Wins / Challenges / One Improvement (this week) + next week's goal to `This Week's Goal` |
| `append_to_log` | cdno-mcp | Log a one-line review recap to today's daily |
| `update_project_state` | cdno-mcp | Apply a state change decided during review |
| `park_project` / `activate_project` | cdno-mcp | Apply a parking decision made during review |
| `hand_off_project` | cdno-mcp | Take a project back (`take_back: true`) or change the name to who really has it (`holder`), on the person's word |
| `add_waiting_on` | cdno-mcp | Record what is expected back on a handed-off project, when the person says |

## Steps

### 1. Gather (silent)

`get_weekly_context` and `get_orientation` in parallel. Hold the wins, state changes, commitments, lapsed habits, the review's `projects`, and `slots`.

### 2. Celebrate the week first

Open with what got done — `completed_actions` and notable `state_changes`. Be specific and generous:
```
Solid week — you closed [N] actions ([[a]], [[b]]) and moved [project] from blocked to running. Nice.
```
Quiet week? Find honest motion ("you kept the daily log going", "you showed up"). Never open with what didn't happen.

### 3. Project movement

Walk `get_weekly_context.projects` briefly. Each project gets the questions its relationship to the person implies, and no others. **Ask; never conclude.** The fields decide which question to ask, not what the answer is.

**The person's own** (`counts: true`): *did it move?* Offer what the week shows (its `state_changes` entry, if any) and ask:

```
Did [project] move this week? I see its state went from blocked to drafting.
```

If it sat still, ask compassionately — "still the priority, or should it wait?" — never as a verdict.

**Handed off** (`counts: false`). Never ask whether it moved: someone else is moving it. Ask whichever of these apply, in this order (`[holder]` is the project's `held_by`, said as the name):

- **A name over an empty Waiting On** (`waiting_on_empty: true`):
  ```
  [project] is with [holder], but nothing is listed under what you're waiting for.
  A name over an empty Waiting On is a parked map wearing a name: park it, or take it back?
  ```
  If the person instead names what they are waiting for, that is their answer too: record it with `add_waiting_on`.
- **It changed this week** (`changed_this_week: true`):
  ```
  [project] changed this week while it's with [holder]. What came back, and what is waiting on your decision?
  ```
- **Work started on it on two or more days** (`focus_days_while_held` of 2 or more). Offer it as what the log shows, and let the person decide:
  ```
  The log shows work started on [project] on [N] days this week, while it's with [holder].
  That may be fine. Does it still sit with them? If it's really yours again, you could take it back,
  or change the name to whoever really has it.
  ```
  One afternoon on a handed-off map does not make it the person's; doing most of its work does, and only they can say which this was.

A handed-off project that none of these apply to gets no question this week; it is fine to name it in passing ("[project] is with [holder]").

Apply only what the person decides, on their word: `park_project` to park; `hand_off_project(project, take_back: true)` to take it back; `hand_off_project(project, holder: "<the name they give>")` to change the name. Taking back an active project makes it count again; if the counted projects are at `slots.cap`, the refusal lists the projects, and the person chooses what to park or finish. Never suggest handing a project off to make room.

Keep it short: at most three projects get a question out loud; offer the rest in a line ("two more are below, if you want them").

### 4. Stewardship scan

From `get_orientation.lapsed_habits`: name any lapsed habit gently ("the [habit] streak slipped this week — happens"). No guilt; just visibility. Offer to look closer with `get_stewardship_tracking` only if they want.

### 5. Look two weeks ahead

From `get_weekly_context.commitments`: surface anything due in the next two weeks, flagging overdue first. Cap at the few that matter.

### 6. Pick ONE goal for next week

The whole review funnels to this. Not a todo list — one sentence:
```
What's the ONE thing that would make next week a win?
```
`[[wikilink]]` the project it serves.

### 7. Record it in the weekly note

Persist the review (design §5.2) — one `upsert_weekly_section` call per section that has content. The three reflection sections record **this** week's note; the ONE forward goal lands in **next** week's note as its `This Week's Goal`, so it carries the date of the coming week:
```
# This week's note (the week you just reviewed) — omit date:
upsert_weekly_section(section: "Wins",            content: "<the wins, as a short list>")
upsert_weekly_section(section: "Challenges",      content: "<what stalled or was hard>")
upsert_weekly_section(section: "One Improvement", content: "<the single change to try>")
# Next week's note — pass a date in that week (the Monday after week_of,
# i.e. week_of + 7 days). It becomes next week's anchoring goal:
upsert_weekly_section(section: "This Week's Goal", date: "<week_of + 7 days>", content: "<the ONE goal, with [[project]]>")
```
Skip a section the user had nothing for — an empty one is fine. Then a one-line daily recap so the day shows it:
```
append_to_log(text: "Weekly review (week of [week_of]): [wins recap]. Next week's goal: [goal].")
```

### 8. Close gently

```
That's your week — [goal] is the anchor. Rest of it can flex.
```

## The monthly note (for a monthly review)

There is no monthly-review skill in this repository; a monthly skill built on `get_monthly_context` follows these questions, in the same spirit (ask, never conclude):

- **Parked projects, by name, once a month: is it time?** Read `get_monthly_context.parked_projects` and name each one by its `slug`, read as words, without opening it: "[project] has been parked. Is it time?" A parked project with a `held_by` name is someone else's to move ("someday" reads apart from "someone has it"); show it with its holder and ask nothing of it unless the person raises it. If the person wants one back, `activate_project` checks the cap: quote `slots.counted` and `slots.cap` from the same context, and if `project_cap_reached` comes back, the choice of what to park or finish is theirs. Never offer handing a project off to make room.
- **A handed-off project in the stuck list: chase, or take back?** `get_monthly_context.stuck_projects` lists active projects whose map has not changed in two weeks; for one with `counts: false`, ask "[project] has been with [holder] for a while with nothing changing. Chase, or take back?" For the person's own stuck projects, the usual "still wanted, or should it wait?" applies.

## What NOT to do

- Don't open with problems or overdue items — wins first, always.
- Don't produce a long plan — ONE goal.
- Don't shame a stalled project or a lapsed habit — frame as a choice.
- Don't ask whether a handed-off project moved. Don't conclude anything for the person from `changed_this_week` or `focus_days_while_held`: ask, and let them decide.
- Don't suggest handing a project off to make room under the cap, and don't write the cap as a number from memory; quote `slots.cap`.
- Don't pad the note with a todo list — actions belong on their projects; the weekly note holds the review and the one goal.
