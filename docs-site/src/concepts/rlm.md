# The Research Logbook Method

The Research Logbook Method (RLM) is the practice Cuaderno implements. It distils habits common to
prolific researchers — Faraday's notebooks, Darwin's dated entries, Hamming's "important problems,"
Knuth's and Tao's working logs — into seven concrete practices across two tracks. Each maps onto one or more
[note types](note-types.md) the tool manages.

## Two tracks

Research is two jobs at once. **Inquiry** is open-ended, driven by questions and evidence.
**Operations** is delivery — deadlines, collaborators, promises. Most systems handle one and force
the other into its shape. The RLM keeps them separate and lets them share a substrate: the daily log,
the weekly review, and one set of projects that bridges the two.

### Track 1 — Inquiry: how you investigate

1. **A chronological log** (Faraday). A single append-only stream of what you did, observed and
   thought, in the order it happened. Past entries are never edited; a change of mind is a new entry
   referencing the old one. It removes the "where do I put this?" decision, preserves the *reasoning*
   and not just the result, and keeps you honest against hindsight. Unified across contexts — the
   regularisation weight and the plumber go in the same day.
2. **Evidence portfolios** (Darwin). One folder per active question, accumulating whatever is
   relevant. The log interleaves many questions; the portfolio aggregates one. Name the folder
   practically, but phrase the *question* at the top of its index: "sparse models" names a topic,
   "does the sparse variant outperform the dense baseline out of distribution?" tells you when you
   are done. Don't organise inside — accumulate, and file as things arise, while your reaction is
   still the most valuable part.
3. **Important questions** (Hamming). The few questions that would genuinely change your situation if
   answered. Not tasks — questions. They sit *above* projects: one question may spawn several over
   time and persists as they finish or get shelved. Two short lists, research and life, reviewed
   monthly: are these still the right questions?

### Track 2 — Operations: how you deliver

A question and a project are not the same thing. A question is open-ended and may branch or turn out
to be the wrong question. A project is bounded — a deliverable, a deadline, people counting on you.
One question can span several projects; a project can draw on several portfolios; either can exist
without the other. Keeping them apart stops deadline pressure polluting open-ended inquiry, and stops
open-ended inquiry dissolving operational commitments.

4. **Project maps** (Knuth). A mutable one-pager per piece of finite work — the page you open after a
   two-week gap. It answers: where was I, what do I believe and what would change my mind, what are
   my next actions, what is time-sensitive, where is everything. Tasks live here, embedded in the
   project that gives them meaning. **Maximum five active projects, at most three next actions each**
   — fifteen tasks as a ceiling, not a backlog. A project map is not a history (the log is), not a
   long checkbox list (eighteen unchecked boxes reads as failure), and not a timeline.
5. **Stewardships.** Dashboards for perpetual responsibilities — health, finances, household — that
   never finish. The critical distinction: **projects end, stewardships do not.** If health and
   finances occupy project slots you permanently lose two of five to things that cannot complete.
   They run in the background with periodic commitments and get a scan at the weekly review, without
   competing for slots.
6. **A commitments register.** One flat, date-sorted list of everything with a hard external
   deadline, whichever project or stewardship it came from. This is *not* a to-do list, and the
   distinction matters: a to-do is something you decided to do, a commitment is something someone
   else is counting on. Mixed together, the commitments drown and you break promises you meant to
   keep.
7. **Energy-matched scheduling** (Tao, Wolfram). Tag work *deep*, *medium* or *light* and match it to
   your current cognitive state. Forcing deep work when your brain wants something mechanical is not
   discipline, it is waste — the alternative to deep work on a low day is usually paralysis, not
   virtue. The exception: if you *never* have deep days, that is environmental, and the fix is
   protecting blocks rather than trying harder.

### The bridge

The log feeds both tracks: an observation becomes evidence in a portfolio, a realisation that
something must be rerun before a deadline becomes a next action on a project map. The weekly review
governs both — retrospective and forward scan. Projects reference the portfolios they draw on, and
commitments decompose into milestones that surface as next actions at the right time.

The daily orientation is where the tracks meet: check what is urgent, glance at what is important,
check your energy, and pick *one* thing.

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

| Practice | Note type(s) | Primary commands |
|--------|--------------|------------------|
| Chronological log | `daily`, `weekly` | [`log`](../reference/cli/log.md), [`orient`](../reference/cli/orient.md), [`review`](../reference/cli/review.md) |
| Evidence portfolios | `portfolio`, `evidence` | [`portfolio`](../reference/cli/portfolio.md), [`file`](../reference/cli/file.md) |
| Important questions | `question` | [`question`](../reference/cli/question.md), [`questions`](../reference/cli/questions.md) |
| Project maps | `project`, `action` | [`project`](../reference/cli/project.md), [`action`](../reference/cli/action.md) |
| Stewardships | `stewardship`, `tracking` | [`stewardship`](../reference/cli/stewardship.md), [`track`](../reference/cli/track.md) |
| Commitments register | `commitment` (+ computed) | [`commit`](../reference/cli/commit.md), [`commitments`](../reference/cli/commitments.md) |
| Energy-matched scheduling | (an `energy` on actions) | [`orient --energy`](../reference/cli/orient.md), [`action add --energy`](../reference/cli/action.md) |

Read on: [Note types](note-types.md).
