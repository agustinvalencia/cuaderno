---
name: quick-capture
description: Instantly capture a thought, idea, or todo before it disappears. Zero friction — one line to the daily log, no categorisation, no decisions. Use when the user says "capture", "quick note", "note to self", "jot this down", or mentions something they don't want to forget.
metadata:
  author: cuaderno
  version: "1.0"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Quick Capture

The lowest-friction skill in the set. Goal: get the thought out of the user's head and into the vault in one move, with no questions asked. Initiation friction is the enemy.

**Principles**: Low friction · External memory · One thing ([full guide](../references/ADHD-PRINCIPLES.md))

## Surface notes (read before editing this skill)

- Capture lands in **today's daily log** via `append_to_log(text)` — a single timestamped line under `## Logs`. This is the RLM chronological-capture surface; it's the right home for a fleeting thought.
- The vault's `inbox/` folder is **not exposed via MCP** — there's no inbox-capture tool. Don't reference one. Everything captured here goes to the daily log. (If a true uncategorised inbox capture is ever needed, that's a future MCP tool, not this skill's job.)

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `append_to_log` | cdno-mcp | Append the captured line to today's daily log |

## Steps

### 1. Capture immediately (no questions)

The moment the user gives you something to capture, write it. Don't ask which project, don't categorise, don't confirm first.

```
append_to_log(text: "<the thing, lightly cleaned up>")
```

Light cleanup only: fix an obvious typo, expand an ambiguous pronoun if trivial. Preserve the user's words and intent. If the thought references a known project or note, wrap it as a `[[wikilink]]` — but never stall to look one up; a bare mention is fine.

### 2. Confirm in one line

Acknowledge it landed, so the user can let go of it. Keep it to a single line.

```
Captured. → "<the thing>"
```

That's it. Don't offer to do more, don't ask follow-ups. The whole value is that capture cost nothing.

## What NOT to do

- Don't ask "which project / category / tag?" — zero decisions at capture time.
- Don't confirm before writing — write first, acknowledge after.
- Don't turn it into a task, a project action, or a commitment — that's triage's job, later.
- Don't lose the input: if `append_to_log` errors, repeat the exact text back so the user can capture it another way ("Couldn't write it — here it is to grab: …").

## Multiple things at once

If the user dumps several thoughts, append each as its own line (one `append_to_log` per item) so each is independently scannable later. Then a single confirmation: "Captured all [N]."
