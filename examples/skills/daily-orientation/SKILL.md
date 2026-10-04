---
name: daily-orientation
description: Start the day with a low-friction, calendar-aware orientation. Surfaces commitments due soon, suggests ONE project to start with, reality-checks the day against your actual free time, and persists a standup, intention, and agenda to the daily note. ADHD-friendly — minimal overwhelm, maximum momentum. Use when the user says good morning, wants to start their day, asks what's on the agenda, or says "orient me", "standup", or "daily standup".
metadata:
  author: cuaderno
  version: "1.2"
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
- **Focus is read from the log, never stored.** `get_orientation.focus` is the same value `current_focus` returns: `{ project, action, started, date, carried, origin }` or `null`. `carried: true` means it was left open on an earlier day. It is changed only by the focus tools (`start_action`, `resume_action`, `pause_action`, `switch_action`), each on the person's word, and each writes its own log line. Never write a focus line with `append_to_log`.
- **The server never refuses a write for being outside the focus**, and it refuses a second `start_action` while one is open (`focus_open`). On `focus_open`, don't retry: follow the refusal's `remedy` (`already_focused`: carry on, ask nothing; `resume_action` or `switch_action`: ask the person first).
- **Calendar is a separate MCP** (`apple-calendar`). If unavailable, skip the schedule and ask once — never block.

## MCP Tools Used

| Tool | Server | Purpose |
|------|--------|---------|
| `get_orientation` | cdno-mcp | Commitments due soon, active projects with their top action, lapsed stewardship habits |
| `get_weekly_context` | cdno-mcp | Recently completed actions (for the wins line + standup) |
| `read_daily_note` | cdno-mcp | Check for pre-planned intention/agenda before writing |
| `upsert_daily_section` | cdno-mcp | Persist the Standup / Intention / Agenda sections |
| `resume_action` | cdno-mcp | Pick a carried focus (or, when nothing is open, a project's latest pause) up again, on the person's yes |
| `start_action` | cdno-mcp | Start the action the person picked, when nothing is open |
| `start_unplanned_action` | cdno-mcp | Same, for work not on the project map yet (title + energy) |
| `switch_action` | cdno-mcp | Move from the open focus to a different action the person named or accepted |
| `switch_unplanned_action` | cdno-mcp | Same, for work not on the map yet |
| `pause_action` | cdno-mcp | Set the open focus aside, on the person's yes |
| `today_schedule` | apple-calendar | Today's meetings and events |
| `find_free_slots` | apple-calendar | Available deep-work windows |

## Steps

### 1. Gather context (silent)

Call in parallel; don't dump raw output on the user:

- `get_orientation` — `{ commitments, projects, lapsed_habits, focus }`.
  - `commitments[]`: `{ date, title, source: { kind, slug }, is_overdue }`. `kind` ∈ `project_milestone | stewardship | standalone_commitment | action_note`.
  - `projects[]`: `{ slug, status, state_snippet, top_action: { text, energy } | null, last_paused: { project, action, title, at, date, next, reason } | null }`. `energy` ∈ `deep | medium | light` or absent. `last_paused` is that project's latest pause not yet picked up again; `next` is the re-entry hint it left, or null.
  - `focus`: `{ project, action, started, date, carried, origin } | null`. `action` is the logged text with its energy suffix, e.g. `Fit baseline (deep)`.
  - `lapsed_habits[]`: `{ stewardship, detail }`.
- `get_weekly_context` — read `completed_actions[]` (`{ slug, project, title, completed, path, source }`); keep those completed yesterday/today for the wins line + standup. `source` is `bullet` or `note`. **A bullet carries `slug: null` and `path: null`, and the bullet is the DEFAULT form of an action — so null is the common case, not the exception.** Every entry has `title` and `project`.
- `read_daily_note` (today) — if `exists`, scan `markdown` for an existing `## Intention` and `## Agenda`. Store what's there; it changes steps 7–8 (acknowledge, don't re-ask or overwrite).
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

- "Yesterday you closed [[ACTION-slug|action title]] on [project] — nice." — **only when `slug` is non-null.** When `slug` is null, say the plain title instead: "Yesterday you closed *action title* on [project] — nice." Never invent a slug to fill the link (`references/LINKING-RULES.md`).
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
**Yesterday** — [N] action(s) done: [[ACTION-slug|title]] when `slug` is non-null, otherwise the bare title, …  (or "light day, no tracked completions")
**Today** — [one neutral line: the day's shape or due-soon; the pick is not decided yet — step 9 records it through the focus tools, and you may re-upsert this line afterwards]
**Due soon** — [commitment titles, or "none"]
```

```
upsert_daily_section(section: "Standup", content: "<standup markdown>")
```

Adapt for sparse days without judgement — just state the facts.

### 6. Ask energy, suggest ONE pick

Recommend, don't open-question. This comes AFTER the greeting, the wins and what's due (steps 2–4) — never lead with the focus. Ask energy first (one word):

```
How's your energy — deep, medium, or light?
```

Then surface exactly ONE pick, in this order. Key on whether a focus is OPEN (`get_orientation.focus` non-null), then on whether it was carried:

**Open today** (`focus.carried: false`). No offer: name it ("You're already on [title] since [started]") and carry on to step 7. No offer needed; a start or resume of this same action would be refused (`already_focused`). The person can still pause it or switch to other work (step 9).

**a. Carried** (`focus.carried: true`) is the recommended pick, framed as continuity, with pausing as the alternative. Use the action's readable title (drop the energy suffix and link syntax; for a promoted action written `[[actions/<slug>]] (energy)`, use the last segment of the slug, never "actions/<slug>"), the day it was started (`date`, as "yesterday" or the weekday) and `started`:

```
Yesterday you were mid-way through [title] on [project] (since [started]).
I suggest picking it up there (Recommended), or pausing it with a note on where you got to.
```

If the focus is deep (`action` ends `(deep)`) and they said light, lead with the pause instead: "That's a deep one and today sounds light — want to pause it with a note on where you got to, and start something lighter? Or pick it up anyway." Their call; offer, don't push.

**b. Nothing open, and the project you'd recommend has a `last_paused`.** (With a focus open, `resume_action(project)` is refused, so this applies only when `focus` is null.) Offer to pick that up, and quote `last_paused.next` when it is present (never invent one):

```
You paused [title] on [project] [yesterday / on date]. You left yourself: "[next]".
I suggest picking it up there (Recommended).
```

**c. Otherwise** (nothing open, no pause to offer) match energy to a project whose `top_action.energy` fits (deep top-action for deep energy, etc.; fall back to any active project with a top action):

```
I'd start with:
→ [project] — [top_action.text]
  (current state: [state_snippet])
```

Let them pick another, but offer the one — don't list all.

### 7. Reality-check the calendar, then persist the agenda

Use the calendar data to show the day's true shape and match the picked action to a real free block.

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

### 9. Act on their answer

What the person said in step 6 decides the call. Each of these writes its own `started` / `resumed` / `paused` line, so there is no separate day-start line to write.

- **"Yes" to picking up a carried focus (6a), or naming that same action ("let's do methods")** → `resume_action`. **"Yes" to a pause offered under 6b** → `resume_action(project: "<slug>")`. Read `resumed_from.kind` and `resumed.action` back, since a carried focus wins over a pause, and read `resumed_from.next` to them when it is set: "Picked up [title] — you left yourself: '[next]'."
- **A plain "yes" to the 6c pick, or to the pick offered after "pause it"** → `start_action(project, query)` with the project's slug and a distinctive substring of its `top_action.text`; `switch_action` with the same arguments if a focus is open.
- **The person names other work** ("let's work on Y", "start Y") → `start_action(project, query)` when nothing is open; `switch_action(project, query)` when a focus is open (it pauses the old one first). Pass `next` to `switch_action` only if you know where the old one stood — never invent it.
- **"Pause it"** → `pause_action`, with `next` only if they say where they got to (ask once, in a clause; no answer means no `next`), then offer the one pick from step 6c.
- **A pick that isn't on the map yet** → `start_unplanned_action` when nothing is open, `switch_unplanned_action` when a focus is open, with a title and an energy.
- **No answer, or "not yet"** → write nothing; an open focus stays as it is.

Never write a prose `append_to_log` line about a start, a pick-up or a focus; the focus tools write the real marker. On `focus_open`, don't retry: follow the refusal's `remedy`. `already_focused` means carry on and ask nothing; for a `resume_action` or `switch_action` remedy, ask the person first.

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
- Don't call `resume_action` or `pause_action` without the person's yes, and `switch_action` only on their word, and don't open with the focus: greeting, wins and due-soon come first.
- Don't write a day-start or focus line with `append_to_log` — the focus tools write the log line.
- Don't build a rigid minute-by-minute timeline. Don't silently overschedule.
- Don't shame a quiet yesterday. Don't manufacture fake wins.
- Don't block on a missing calendar — degrade to asking once.

## Edge cases

### Starting later in the day
"Late" is by the clock, not a judgement. Greet by time of day: morning before 12:00, afternoon 12:00–17:00, evening after 17:00. When the first orientation of the day lands in the afternoon or later there's less runway to plan, so compress: greet for the time of day ("Hey — it's [Day] afternoon, let's orient quick"), show only calendar time from now onward, and shrink the ask to one action. Still write the standup and suggest one action. No shame for a later start — just less day left.

### Nothing active
No active projects: offer `/create-project`. No commitments and no actions: "Clean slate — what's one thing worth a dent today?"

### Quick mode
If the user seems rushed or says "quick"/"fast": greeting + the single most time-sensitive thing (overdue commitment, else the suggested action) + write the standup silently. With a carried focus, make the one suggestion the pick-up, in a line. Skip energy, agenda, intention. Movement over process.

## Greeting variations

Vary the opener: "Good morning! It's [Day], [Date]." · "Morning — happy [Day]." · "Rise and shine, it's [Day]." Keep it warm and brief.
