---
name: weekly-review
description: An ADHD-friendly weekly review — celebrate the week's wins first, scan stewardship habits, review project movement, look at the next two weeks of commitments, and pick ONE main goal for next week. No shame, no overwhelm. Use when the user wants to do their weekly review, reflect on the week, or says "weekly review".
metadata:
  author: cuaderno
  version: "1.1"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Weekly Review

A 5–10 minute reflective pass on the week. Wins first, always. Surface what moved, gently flag what stalled, look just far enough ahead, and leave with ONE goal — not a sprawling plan.

**Principles**: Celebrate wins first · One thing at a time · Compassionate accountability · No shame ([guide](../references/ADHD-PRINCIPLES.md))
**Linking**: Use `[[wikilinks]]` for projects/actions referenced in logged content ([rules](../references/LINKING-RULES.md))
**Logging**: Log checkpoints to the daily as they happen — atomic, not batched.

## Surface notes (read before editing this skill)

- `get_weekly_context` returns the week's picture: `week_of` (the ISO Monday), `logs` (daily-log lines), `completed_actions` (the wins list), `state_changes` (project states that moved this week), and `commitments` (next two weeks).
- The **stewardship scan** uses `get_orientation.lapsed_habits` (which stewardships have lapsed) — optionally `get_stewardship_tracking(stewardship, activity, period)` for detail.
- **The review persists to the weekly note.** `upsert_weekly_section(section, content, date?, append?)` writes a section of the ISO-week note for `date` (design §5.2). Each week's anchoring goal lives in **that week's own note** under `This Week's Goal`, so the review splits its writes: `Wins`, `Challenges`, and `One Improvement` record **this** week's note (omit `date`), while the ONE forward goal lands in **next** week's note's `This Week's Goal` — pass a `date` in next week (the Monday after `week_of`, i.e. `week_of` + 7 days). It becomes that week's anchor, ready for weekly-planning to refine. `append: false` (default) composes the section. `read_weekly_note(date?)` checks whether a week already has a note before you start. (The former `Next Week's Focus` is a deprecated alias that maps to `This Week's Goal` — use the canonical name.) Still drop a one-line recap to **today's daily** via `append_to_log`, and apply any decided changes via `update_project_state` / `park_project` / `activate_project`. The conversation is the review; the note is its record.

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `get_weekly_context` | cdno-mcp | The week: logs, completed actions (wins), state changes, 2-week commitments |
| `get_orientation` | cdno-mcp | Lapsed stewardship habits + active project slate |
| `get_stewardship_tracking` | cdno-mcp | Optional detail on a specific stewardship |
| `read_weekly_note` | cdno-mcp | Check whether the week already has a note before composing |
| `upsert_weekly_section` | cdno-mcp | Persist Wins / Challenges / One Improvement (this week) + next week's goal to `This Week's Goal` |
| `append_to_log` | cdno-mcp | Log a one-line review recap to today's daily |
| `update_project_state` | cdno-mcp | Apply a state change decided during review |
| `park_project` / `activate_project` | cdno-mcp | Apply a parking decision made during review |

## Steps

### 1. Gather (silent)

`get_weekly_context` and `get_orientation` in parallel. Hold the wins, state changes, commitments, lapsed habits, and project slate.

### 2. Celebrate the week first

Open with what got done — `completed_actions` and notable `state_changes`. Be specific and generous:
```
Solid week — you closed [N] actions ([[a]], [[b]]) and moved [project] from blocked to running. Nice.
```
Quiet week? Find honest motion ("you kept the daily log going", "you showed up"). Never open with what didn't happen.

### 3. Project movement

Walk the active slate briefly: which moved (`state_changes`), which sat still. Flag a stalled project compassionately — "X didn't move this week; still the priority, or should it wait?" — as a question, not a verdict.

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

## What NOT to do

- Don't open with problems or overdue items — wins first, always.
- Don't produce a long plan — ONE goal.
- Don't shame a stalled project or a lapsed habit — frame as a choice.
- Don't pad the note with a todo list — actions belong on their projects; the weekly note holds the review and the one goal.
