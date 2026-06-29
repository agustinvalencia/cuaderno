# Note types

Every note in a vault has a `type:` in its frontmatter. Cuaderno parses that frontmatter into a
typed structure — if it parses, it's valid. There are **ten** note types.

| Type | Lives in | Mutability | Purpose |
|------|----------|------------|---------|
| `daily` | `journal/daily/` | Append-only | One day's chronological log |
| `weekly` | `journal/weekly/` | Append-only | Weekly review (Wins, Challenges, One Improvement, This Week's Goal) |
| `project` | `projects/` (+ `_parked/`) | **Mutable** | Project map: state, next actions, milestones, waiting-on |
| `action` | `actions/` → `actions/_done/<year>/` | Mutable while open, then archived | Manifest note for an action-as-investigation |
| `portfolio` | `portfolios/<slug>/_index.md` | Occasionally edited | Index/summary of an evidence dossier |
| `evidence` | `portfolios/<slug>/` | Append-only | A single piece of evidence (paper, result, note) |
| `stewardship` | `stewardships/` (flat or folder) | Occasionally edited | Dashboard for a perpetual responsibility |
| `tracking` | `stewardships/<slug>/tracking/` | Append-only | One time-series entry (a gym session, a measurement) |
| `question` | `questions/research/` or `questions/life/` | Status transitions | An important research or life question |
| `commitment` | `commitments/` → `commitments/_done/` | Moves on completion | A standalone dated promise |

Plus the **inbox**: raw, untyped captures in `inbox/` awaiting [triage](../tutorials/inbox-and-triage.md).

## The journal: `daily` and `weekly`

The chronological backbone. Daily notes collect timestamped log lines plus structured sections
(Intention, Agenda, Standup, Meeting). Weekly notes hold the review (Wins, Challenges, One
Improvement) and the week's single goal. Both are **append-only** — the historical record only grows.

## `project` — the one mutable map

Projects are the only freely-mutable note type. A project carries a **Current State**, a list of
**next actions** (inline bullets by default), **milestones**, and **waiting-on** items. When you
update the Current State, the previous state is auto-logged to today's daily note first, so history
is never lost (see [Business rules](business-rules.md)). At most **five** projects are active at
once; the rest live parked in `projects/_parked/`.

## `action` — inline by default, a note when it grows

An action's default form is a checkbox bullet on its project map. That's usually all you need. When a
single action becomes an investigation spanning days and artefacts, you **promote** it to a manifest
`action` note (heavier: status, energy, criteria, links). Completing an action removes the bullet,
logs it, and — if it had a note — archives that note to `actions/_done/<year>/`. See
[Actions](../tutorials/actions.md).

## `portfolio` + `evidence` — dossiers per question

A portfolio is a folder named for a question, with an `_index.md` (the `portfolio` note) and a set of
`evidence` notes filed into it over time. Each evidence note records a `source` and an `origin`
(a wikilink to whatever produced it). Evidence is append-only — a portfolio is a growing record. See
[Research and evidence](../tutorials/research-and-evidence.md).

## `stewardship` + `tracking` — long-haul responsibilities

A stewardship is a dashboard for something you tend indefinitely. It can be **flat** (a single
`stewardships/<slug>.md`) or **expanded** (a `stewardships/<slug>/` folder with `_index.md`, a
`tracking/` subfolder for time-series entries, and a `routines/` subfolder for reference docs).
Expanded stewardships accept `tracking` notes via [`cdno track`](../reference/cli/track.md). See
[Stewardships and tracking](../tutorials/stewardships-and-tracking.md).

## `question` — important questions, kept visible

A question note has a `domain` (`research` or `life`) and a `status` (`active`, `parked`, `answered`,
`retired`). Questions anchor portfolios and projects (via the `core_question` link). List the active
ones any time with [`cdno questions`](../reference/cli/questions.md).

## `commitment` — dated promises

A standalone promise with a hard `due:` date and a `context`. On fulfilment it moves to
`commitments/_done/<year>/`. Standalone commitments are one of four sources feeding the aggregated
[commitments view](../tutorials/commitments.md).

For the exact frontmatter fields of each type, see [Frontmatter fields](../reference/frontmatter.md).
Next: [Vault structure](vault-structure.md).
