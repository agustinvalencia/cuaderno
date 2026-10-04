---
name: quick-capture
description: Instantly capture a thought, idea, or todo before it disappears. Zero friction — one line to the daily log, no categorisation, no decisions. Use when the user says "capture", "quick note", "note to self", "jot this down", or mentions something they don't want to forget.
metadata:
  author: cuaderno
  version: "1.1"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured.
---

# Quick Capture

The lowest-friction skill in the set. Goal: get the thought out of the user's head and into the vault in one move, with no questions asked. Initiation friction is the enemy.

**Principles**: Low friction · External memory · One thing ([full guide](../references/ADHD-PRINCIPLES.md))

## Surface notes (read before editing this skill)

- Capture lands in **today's daily log** via `append_to_log(text)` — a single timestamped line under `## Logs`. This is the RLM chronological-capture surface; it's the right home for a fleeting thought.
- `capture(text)` exists: it writes the text verbatim as an item under `inbox/` for later triage. It stays the exception here — `append_to_log` is the default, and the right home for a fleeting thought. Reach for `capture` only when the person says "inbox" or "for triage", or the thing is plainly a to-sort item rather than a moment in the day.
- **Focus tagging is the server's job.** When a focus is open, `append_to_log` and `capture` record it themselves (an indented `during: [[slug]]` line under the log entry, or `captured_during:` on the inbox item). Never write a `during:` or `captured_during:` tag by hand, and don't mention focus in the captured text.
- The write results of `append_to_log`, `capture` and `note_to_daily` carry `focus` — the same value `current_focus` returns, or `null` when nothing is open. Read it from the result; no second call.

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `append_to_log` | cdno-mcp | Append the captured line to today's daily log (default) |
| `capture` | cdno-mcp | Drop a raw line into the inbox, when the person asks for the inbox |

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

### 3. Give the return cue (only when a focus is open)

If the write result's `focus` is not `null`, add one short line that points back at it, so the thought can stay parked and the person can step back in. Use the focus's readable title (drop the energy suffix) and, if you already know where they were — a `next:` you read back from `resume_action`, or what you saw them do — say it:

```
Captured. → "<the thing>"
Back to [title] — you were at "[where they were]".
```

With nothing known about where they were, just "Back to [title]." Never invent the place. If `focus` is `null`, stop at the confirmation; no cue. The cue is a statement, not a question: don't ask whether they want to move over, and don't repeat it for a second capture in the same breath (one cue after a batch).

## What NOT to do

- Don't ask "which project / category / tag?" — zero decisions at capture time.
- Don't confirm before writing — write first, acknowledge after.
- Don't turn it into a task, a project action, or a commitment — that's triage's job, later.
- Don't lose the input: if `append_to_log` errors, repeat the exact text back so the user can capture it another way ("Couldn't write it — here it is to grab: …").

## Multiple things at once

If the user dumps several thoughts, append each as its own line (one `append_to_log` per item) so each is independently scannable later. Then a single confirmation: "Captured all [N]." (plus the one return cue, if a focus is open).
