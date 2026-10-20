# Weekly review

The weekly review is the one slightly-longer ritual in the method — a short retrospective plus a
single goal for the week ahead. It's where you celebrate progress, notice what's stuck, and set
direction without micromanaging.

## The weekly note

Each ISO week has a `weekly` note with four sections:

- **Wins** — what moved (actions completed, evidence filed, projects advanced).
- **Challenges** — what got in the way.
- **One Improvement** — a single thing to change next week.
- **This Week's Goal** — the week's anchor: one goal everything else orbits.

View the current week's note (or any week) any time:

```bash
cdno weekly                       # this ISO week
cdno weekly --date 2026-04-20     # the week containing that date
```

## The guided ritual

[`cdno review weekly`](../reference/cli/review.md) walks you through it. Interactively, it prompts for
each retrospective section and writes them into **this** week's note, then asks for next week's goal
and sets it as the **This Week's Goal** of **next** week's note — so when the new week starts, its
anchor is already there.

```bash
cdno review weekly
```

Run non-interactively (or with `--no-interactive`), it reads the current note rather than prompting —
handy for scripts or a quick scan.

## A weekly cadence that works

A simple routine, ~15 minutes:

1. `cdno status` — skim active projects and their top actions.
2. `cdno commitments --weeks 2` — what's due in the next fortnight?
3. `cdno stewardship list` — any habit looking stale?
4. `cdno review weekly` — capture Wins / Challenges / One Improvement, set next week's goal.

The point isn't a perfect record; it's a regular moment to look up from the work, acknowledge what
you did, and choose one direction. Parking, completing or dropping a project here, or retiring a
question, is a *good* outcome, not a failure.

## A question per relationship

Not every open project is asked the same thing, because not every project is yours to move. Walk
the active projects (`cdno project list` shows which name a holder) and ask each the question its
relationship implies:

- **Yours** (no holder): *did it move?* If it sat still, is it still the priority, or should it
  wait? Parking it is a fine answer.
- **Handed off, and its map changed this week**: *what came back, and what is waiting on my
  decision?* Not "did it move?": the moving is someone else's.
- **Handed off over an empty Waiting On**: a name over an empty Waiting On is a parked map wearing a
  name. Park it, or take it back.
- **Handed off, but you worked on it yourself on two or more days this week**: doing a thing on a
  handed-off map does not make it yours, doing most of its work does. Take it back, or change the
  name to whoever really has it.
- **Handed off, and stuck for weeks** (at the monthly review, from its list of stuck projects):
  chase the holder, or take it back.

A handed-off project that did not change this week needs no question at all. The questions are
prompts, never verdicts, and a hand-off is never an answer to a full cap: to make room, park,
complete or drop a project. See [Managing projects](projects.md#hand-a-project-off).

## With Claude

The MCP tools `get_weekly_context`, `read_weekly_note`, and `upsert_weekly_section` let an assistant
run the same review conversationally — reading your week back to you and writing the sections as you
talk. `get_weekly_context` carries, per active project, `counts`, `held_by`, `changed_this_week`,
`waiting_on_empty` and `focus_days_while_held`, so an assistant can ask each project the question
above (the stuck-project question reads `get_monthly_context`'s `stuck_projects`, not these fields); the [`weekly-review`](../reference/mcp/with-claude-skills.md) example skill shows how. See [Connect to Claude](../getting-started/connect-to-claude.md).

Next: [Inbox and triage](inbox-and-triage.md).
