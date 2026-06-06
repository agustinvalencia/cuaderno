---
name: complete-action
description: Mark a next action as done, with a proper dopamine hit. Celebrates the win, archives any attached action note, and prompts for what's next. Use when the user says they finished something, "done", "completed", "tick off", or "mark [X] done".
metadata:
  author: cuaderno
  version: "1.0"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Complete Action

Close a next action with closure that feels good. For ADHD brains the completion signal matters as much as the work — name the win before moving on.

**Principles**: Celebrate wins first · Compassionate accountability · One thing ([full guide](../references/ADHD-PRINCIPLES.md))

## Surface notes (read before editing this skill)

- `complete_action(project, query)` matches a Next Actions bullet by **substring** `query`, removes it, logs the completion to today's daily, and — if the bullet had an attached action note — flips its status to completed and archives it to `actions/_done/<year>/`. One atomic move.
- It errors on an **ambiguous** match (more than one bullet contains `query`). Handle that by asking for a more specific phrase rather than guessing.
- **No stored "focus".** If the user doesn't name the project, resolve it from `get_orientation.projects` (single active → use it; otherwise ask).

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `complete_action` | cdno-mcp | Remove the bullet, log completion, archive any attached note |
| `get_orientation` | cdno-mcp | Resolve the project / see the next action to suggest |
| `add_action` | cdno-mcp | Optionally add the replacement next action |

## Steps

### 1. Resolve project + which action

- Project: named by the user, else resolve from `get_orientation` (single active → silent; several → ask once).
- `query`: a distinctive phrase from the action the user finished. Keep it short but specific enough to match one bullet.

### 2. Complete it

```
complete_action(project: "<slug>", query: "<distinctive phrase>")
```

**If it errors as ambiguous**, don't guess — ask: "A couple of actions match '[query]' — which one: [a] or [b]?" and retry with a sharper phrase.

### 3. Celebrate the win (first, always)

Name the specific thing, warmly and briefly. This is the point of the skill.

```
Done — [action title]. Nice one.
```

Vary it ("That's off the list.", "Closed.", "One down."). If it cleared a milestone or unblocked something, say so. Never skip straight to "what's next".

### 4. Prompt the next action (low pressure)

Glance at the project's remaining actions (from `get_orientation.projects[].top_action`, or by re-reading after completion) and offer ONE:

```
Next on [project]: → [top action]. Want to pick that up, or take a breather?
```

If the user describes a new follow-up action, offer to add it:
```
add_action(project: "<slug>", title: "<follow-up>", energy: "<...>")
```

If the project has no actions left, say so plainly and offer it as a small prompt, not a demand: "[project] has no next action queued — want to add one, or leave it for now?"

## What NOT to do

- Don't skip the celebration or bury it under next-steps — the completion signal is the dopamine.
- Don't guess on an ambiguous match — ask.
- Don't pile on "and now do X, Y, Z" — surface ONE next action, optionally.
- Don't shame leftover work on the project ("you still have 6 todo") — one forward step, no scoreboard.

## Multiple completions

If the user finished several actions, complete each (own `complete_action` call), celebrate them together ("Three done — [a], [b], [c]. Strong session."), then one forward prompt.
