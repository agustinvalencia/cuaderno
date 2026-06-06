---
name: daily-orientation
description: Start the day with a low-friction, calendar-aware orientation. Surfaces commitments due soon, suggests ONE project to start with, reality-checks the day against your actual free time, and persists a standup, intention, and agenda to the daily note. ADHD-friendly — minimal overwhelm, maximum momentum. Use when the user says good morning, wants to start their day, asks what's on the agenda, or says "orient me", "standup", or "daily standup".
metadata:
  author: cuaderno
  version: "1.1"
compatibility: Requires the cuaderno MCP server (cdno-mcp) with a vault configured. Calendar awareness additionally uses the apple-calendar MCP server; the skill degrades gracefully without it.
---

# Daily Orientation

ADHD-friendly morning routine for the Research Logbook Method. Goal: get the user moving with ONE clear action, anchored to a realistic picture of the day.

**Principles**: One thing at a time · Wins first · No shame · Low friction · The vault remembers ([full guide](../references/ADHD-PRINCIPLES.md))
**Linking**: Use `[[wikilinks]]` when written content references a project, question, or other note ([rules](../references/LINKING-RULES.md))
**Calendars**: The user may keep several calendars by context (work, personal, shared) and may mark shared-calendar events with an ownership prefix; honour whatever convention theirs uses ([details](../references/CALENDAR-CONVENTIONS.md))

## Surface notes (read before editing this skill)

What the cuaderno MCP can and can't do here, so steps stay bound to real tools:

- **Planning sections persist.** `upsert_daily_section(section, content)` writes the daily note's `Standup`, `Intention`, or `Agenda` section (create-or-replace). Any other section name — including the append-only `Logs`/`Notes` — is rejected. Use it to persist the standup, intention, and agenda.
- **Pre-planned content is readable.** `read_daily_note(date?)` returns the day's markdown (or `exists: false` when none yet). Scan it for an already-written `## Intention` or `## Agenda` (from a prior session, weekly-planning, or close-day) before writing — don't clobber the user's earlier thinking.
- **History is append-only.** `## Logs` only grows, via `append_to_log(text)` (single timestamped lines). Never try to write `Logs`/`Notes` through `upsert_daily_section`.
- **No stored "focus".** The skill *suggests* one project from `get_orientation`; it does not read or write a focus marker.
- **Calendar is a separate MCP** (`apple-calendar`). If unavailable, skip the schedule and ask once — never block.

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `get_orientation` | cdno-mcp | Commitments due soon, active projects with their top action, lapsed stewardship habits |
| `get_weekly_context` | cdno-mcp | Recently completed actions (for the wins line + standup) |
| `read_daily_note` | cdno-mcp | Check for pre-planned intention/agenda before writing |
| `upsert_daily_section` | cdno-mcp | Persist the Standup / Intention / Agenda sections |
| `append_to_log` | cdno-mcp | Log a short "day started" line to the daily log |
| `today_schedule` | apple-calendar | Today's meetings and events |
| `find_free_slots` | apple-calendar | Available deep-work windows |

## Steps

### 1. Gather context (silent)

Call in parallel; don't dump raw output on the user:

- `get_orientation` — `{ commitments, projects, lapsed_habits }`.
  - `commitments[]`: `{ date, title, source: { kind, slug }, is_overdue }`. `kind` ∈ `project_milestone | stewardship | standalone_commitment | action_note`.
  - `projects[]`: `{ slug, status, state_snippet, top_action: { text, energy } | null }`. `energy` ∈ `deep | medium | light` or absent.
  - `lapsed_habits[]`: `{ stewardship, detail }`.
- `get_weekly_context` — read `completed_actions[]` (`{ slug, project, title, completed, path }`); keep those completed yesterday/today for the wins line + standup.
- `read_daily_note` (today) — if `exists`, scan `markdown` for an existing `## Intention` and `## Agenda`. Store what's there; it changes steps 6–8 (acknowledge, don't re-ask or overwrite).
- `today_schedule` (apple-calendar) — today's events. On error, note calendar unavailable and continue.
- `find_free_slots` for today (apple-calendar) — free windows. Same graceful-degrade rule.

Note the day of week and date now (combats time blindness).

### 2. Warm greeting with orientation

Brief — establish time, then the shape of the day:

```
Good morning! It's [Day], [Date].
[N] active projects on the go, [M] commitment(s) due soon.
```

No active projects: "Clean slate — no active projects. Want to start one?"

### 3. Celebrate before problems

If `completed_actions` shows anything finished yesterday/today, lead with it:

- "Yesterday you closed [[ACTION-slug|action title]] on [project] — nice."
- "Two actions done this week already — good momentum."

If the daily note already had a pre-planned intention/agenda (step 1), acknowledge that planning effort — for ADHD brains, planning ahead is itself a win. If nothing completed, find something honest and small ("You're here and oriented — that counts"). No manufactured praise, no shame.

### 4. Surface time-sensitive commitments

From `get_orientation.commitments`, lead with `is_overdue: true`, then nearest upcoming. Cap at 3; summarise if more.

```
Due soon:
→ [title] ([relative date], [source kind]) [· overdue]
```

Relative time ("tomorrow", "in 2 days"), not raw dates.

### 5. Write the standup (silent)

Compose a short standup from the gathered context and persist it. Don't ask — just write, then mention you did.

```markdown
**Yesterday** — [N] action(s) done: [[ACTION-slug|title]], …  (or "light day, no tracked completions")
**Today** — starting [[project-slug]]: [top action]
**Due soon** — [commitment titles, or "none"]
```

```
upsert_daily_section(section: "Standup", content: "<standup markdown>")
```

Adapt for sparse days without judgement — just state the facts.

### 6. Ask energy, suggest ONE project

Recommend, don't open-question. Ask energy first (one word), then bias the suggestion:

```
How's your energy — deep, medium, or light?
```

Match energy to a project whose `top_action.energy` fits (deep top-action for deep energy, etc.; fall back to any active project with a top action). Surface exactly ONE:

```
I'd start with:
→ [project] — [top_action.text]
  (current state: [state_snippet])
```

Let them pick another, but offer the one — don't list all.

### 7. Reality-check the calendar, then persist the agenda

Use the calendar data to show the day's true shape and match the suggested action to a real free block.

**If events + free slots are available:**
```
Today's shape:
- [time] [event] [whose, if a shared-calendar prefix indicates it]
Free for deep work:
- [start]–[end] ([duration])

That [duration] block fits [project]'s next action — realistic for today is one solid pass, not three half-starts.
```

**If the day is packed:** say so and shrink the ask ("Calendar's tight — maybe [Xh] free; aim for just the one action, or a 15-minute start").

**If `read_daily_note` already had an `## Agenda`:** merge — confirm what matches, only update what changed; don't blow away a pre-filled agenda.

**If calendar is unavailable:** skip the schedule, ask once ("Anything booked today I should plan around?"), and proceed.

Once the shape is agreed, **persist it** — this is the realistic-expectations record:

```
upsert_daily_section(section: "Agenda", content: "<schedule + free blocks + the realistic call>")
```

Keep it a loose shape, not a minute-by-minute plan (ADHD brains rebel against over-structure). Don't silently overschedule — if the work exceeds the time, say so. A mermaid gantt is optional; only add one if the user likes the visual.

### 8. Set the intention

**If `read_daily_note` already had a `## Intention`:** acknowledge it, don't re-ask.
```
Your intention for today: "[existing intention]" — still feels right, or adjust?
```
Only rewrite if they want to change it.

**If none exists:** ask for one sentence (optional — don't push).
```
One thing that would make today feel successful? (your north star)
```

Persist whatever they give:
```
upsert_daily_section(section: "Intention", content: "<intention text>")
```

If they skip it, that's fine — leave the section unwritten.

### 9. Log the start (silent)

A single timestamped line in the daily log, so the day's start is in the history:
```
append_to_log(text: "Started the day — focus [[<project-slug>]]: <top action>.")
```

### 10. Launch with momentum

Reduce initiation friction to the smallest first step, then get out of the way:
```
You're set — [project], starting with: [smallest first step, e.g. "open the file" / "write one sentence"].
Go get it. I'm here if you need me.
```

## What NOT to do

- Don't list every active project or commitment — one project, commitments capped at 3.
- Don't ask open-ended "what do you want to do?" — recommend.
- Don't write `Logs`/`Notes` via `upsert_daily_section` — they're append-only; the call is rejected. Log lines go through `append_to_log`.
- Don't overwrite a pre-filled Intention or Agenda — acknowledge or merge (you read them in step 1).
- Don't reference a stored "focus" — cuaderno has none; you're suggesting.
- Don't build a rigid minute-by-minute timeline. Don't silently overschedule.
- Don't shame a quiet yesterday. Don't manufacture fake wins.
- Don't block on a missing calendar — degrade to asking once.

## Edge cases

### Late start
Adapt the greeting ("Hey — it's [Day] afternoon, let's get oriented quick"), only show calendar time from now onward, still write the standup + suggest one action + log a line.

### Nothing active
No active projects: offer `/create-project`. No commitments and no actions: "Clean slate — what's one thing worth a dent today?"

### Quick mode
If the user seems rushed or says "quick"/"fast": greeting + the single most time-sensitive thing (overdue commitment, else the suggested action) + write the standup silently + log a line. Skip energy, agenda, intention. Movement over process.

## Greeting variations

Vary the opener: "Good morning! It's [Day], [Date]." · "Morning — happy [Day]." · "Rise and shine, it's [Day]." Keep it warm and brief.
