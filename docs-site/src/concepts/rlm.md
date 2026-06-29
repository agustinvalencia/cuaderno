# The Research Logbook Method

The Research Logbook Method (RLM) is the practice Cuaderno implements. It distils habits common to
prolific researchers — Faraday's notebooks, Darwin's dated entries, Hamming's "important problems,"
Knuth's and Tao's working logs — into six concrete pillars. Each pillar maps onto one or more
[note types](note-types.md) the tool manages.

## The six pillars

1. **A chronological log.** A dated, append-only record of what you did and thought — the single
   source of truth. In Cuaderno this is the daily and weekly journal.
2. **Evidence portfolios.** A dossier per important question that accumulates evidence — papers,
   experiment results, conversation notes — over months and years.
3. **Important questions.** Hamming's discipline, made first-class: name the questions that matter,
   keep them visible, re-read them often.
4. **Project maps.** Lightweight overviews of active work — a current state, next actions, and
   milestones. Not a Gantt chart.
5. **Stewardships.** Small, bounded, perpetual responsibilities (health, finances, a recurring
   service) — long-haul, low-drama, optionally with habit tracking.
6. **A commitments register.** Promises to others (and dated promises to yourself), with deadlines —
   distinct from a to-do list.

## Actions, not tasks

The RLM speaks of **actions**, not "tasks," on purpose. The default form of an action is a single
inline bullet on a project map — not a heavyweight per-item note. You only "promote" an action to
its own note when it grows into an investigation spanning multiple days and evidence artefacts. This
keeps the friction of capturing the next step near zero. See [Actions](../tutorials/actions.md).

## Designed for ADHD working styles

The method is deliberately shaped to be sustainable when executive function is unreliable:

- **Leads with what is there**, not what is missing — no guilt engine, no red overdue counts.
- **Permission to park or drop.** Projects park, questions retire, commitments get fulfilled or
  dropped — all first-class, all reversible.
- **Minimal maintenance.** If keeping the system running costs more than a few minutes a day outside
  the weekly review, something is wrong.
- **One obvious next step.** `cdno orient` answers "what should I do now?" with a single suggestion,
  biased to your current energy.

## The rhythm

The method runs on a few interlocking loops:

- **Daily** — orient in the morning, log and act through the day, a light close at the end.
  See [The daily loop](../tutorials/daily-loop.md).
- **Weekly** — a short retrospective (wins, challenges, one improvement) and a single goal for next
  week. See [Weekly review](../tutorials/weekly-review.md).
- **Occasional** — file evidence as you find it, triage the inbox, prune questions and projects.

## How it maps to the tool

| Pillar | Note type(s) | Primary commands |
|--------|--------------|------------------|
| Chronological log | `daily`, `weekly` | [`log`](../reference/cli/log.md), [`orient`](../reference/cli/orient.md), [`review`](../reference/cli/review.md) |
| Evidence portfolios | `portfolio`, `evidence` | [`portfolio`](../reference/cli/portfolio.md), [`file`](../reference/cli/file.md) |
| Important questions | `question` | [`question`](../reference/cli/question.md), [`questions`](../reference/cli/questions.md) |
| Project maps | `project`, `action` | [`project`](../reference/cli/project.md), [`action`](../reference/cli/action.md) |
| Stewardships | `stewardship`, `tracking` | [`stewardship`](../reference/cli/stewardship.md), [`track`](../reference/cli/track.md) |
| Commitments register | `commitment` (+ computed) | [`commit`](../reference/cli/commit.md), [`commitments`](../reference/cli/commitments.md) |

Read on: [Note types](note-types.md).
