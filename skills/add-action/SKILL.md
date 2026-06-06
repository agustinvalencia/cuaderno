---
name: add-action
description: Add a next action to a project. Low friction — defaults to an inline next-action bullet on the project map. Use when the user wants to add a task or todo, says "add an action", "next action", "I need to", or describes something to do on a project.
metadata:
  author: cuaderno
  version: "1.0"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Add Action

Add a next action to a project's `## Next Actions`. In the Research Logbook Method most actions are lightweight inline bullets; the heavier action-note form is the exception, not the default.

**Principles**: Low friction · Reduce decision fatigue · One thing ([full guide](../references/ADHD-PRINCIPLES.md))
**Linking**: Use `[[wikilinks]]` for any project/note the action references ([rules](../references/LINKING-RULES.md))

## Surface notes (read before editing this skill)

- `add_action(project, title, energy, with_note?)` appends an inline `- [ ]` bullet to the project's Next Actions. With `with_note: true` it also creates an action note (design §5.11) and wikilinks the bullet — the heavier form, for multi-day / multi-evidence investigation work only.
- `energy` is required by the tool: one of `deep`, `medium`, `light`.
- **No stored "focus".** There's no focus marker to default the project to. If the user doesn't name a project, resolve it from `get_orientation.projects` (use the single active project if there's exactly one; otherwise ask which).

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `add_action` | cdno-mcp | Append the next-action bullet (optionally with an action note) |
| `get_orientation` | cdno-mcp | Resolve the project when the user didn't name one |

## Steps

### 1. Resolve the project (low friction)

- If the user named a project, use it.
- If not, call `get_orientation`. One active project → use it silently. Several → ask once, listing only the active slugs: "Which project — [a], [b], or [c]?"
- No active projects → offer `/create-project` first.

### 2. Title and energy

- The action title is whatever the user described — keep it short and verb-first ("Draft the intro", "Reply to the open review thread").
- Energy: infer if the description makes it obvious (deep = focused/creative/complex; light = quick/admin; medium otherwise). If genuinely unclear, ask one word — don't interrogate. Default to `medium` if the user waves it off.

### 3. Add it (inline by default)

```
add_action(project: "<slug>", title: "<title>", energy: "<deep|medium|light>")
```

Only set `with_note: true` when the action is clearly investigation-style work that will accrete notes/evidence over days (design §5.11) — and say why ("This one looks like it'll grow, so I'll give it an action note"). Default is the inline bullet.

### 4. Confirm in one line

```
Added to [project]: → <title> (<energy>)
```

Offer nothing further unless asked. The point is to capture the action and get back to work.

## What NOT to do

- Don't default to `with_note: true` — the inline bullet is the norm; the note is the exception.
- Don't ask a string of questions. Project (if ambiguous) and, at most, energy. Nothing else.
- Don't invent a project — if it's unclear and there's no active one, route to `/create-project`.
- Don't reference a "focus" project as if stored — resolve from active projects.

## Multiple actions

If the user lists several actions for the same project, add each with its own `add_action` call, then one confirmation: "Added [N] actions to [project]."
