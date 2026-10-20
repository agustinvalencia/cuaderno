# Managing projects

A **project** is a lightweight map of a piece of active work: a current state, next actions,
milestones, and things you're waiting on. You keep at most [five active](../concepts/business-rules.md)
at once. All the verbs live under [`cdno project`](../reference/cli/project.md) (plus
[`cdno action`](../reference/cli/action.md) for the next-action list).

## Create one

```bash
cdno project create --title "Surrogate model" --context work
# -> projects/surrogate-model.md   (slug derived from the title)
```

`--context` is the [life domain](../concepts/contexts-and-energy.md). Optionally link the project's
core question with `--question questions/research/surrogate-cost` — and if you skip it, or the
question changes later, `cdno project core-question --slug surrogate-model --question <target>`
sets it afterwards (`--clear` detaches). If you're already at five active projects, the new one is
created **parked** — activate it once you free a slot.

## See where things stand

```bash
cdno project list                 # active projects + a state snippet
cdno project show surrogate-model # one project in detail
cdno status                       # all active projects + their top action
```

Add `--json` to any of these for structured output (see [JSON output](../reference/json-output.md)).

## Update the current state

The Current State is the project's one mutable paragraph — "where is this right now?". Updating it
auto-logs the *previous* state to today's journal first, so you never lose the trail:

```bash
cdno project state --slug surrogate-model \
  --text "Mesh scaling works to 2M cells; assembly is now the bottleneck"
```

## Next actions

Actions are the things to do next. By default they're inline bullets on the project:

```bash
cdno action add --project surrogate-model --title "Profile the assembly step" --energy medium
cdno action list --project surrogate-model
cdno action complete --project surrogate-model --query "profile the assembly"
```

See [Actions](actions.md) for the inline-vs-manifest distinction and promotion.

## Milestones

Milestones are markers of progress. Mark one `--hard` to make it a real deadline that shows up
in the aggregated [commitments](commitments.md) view:

```bash
cdno project milestone add --slug surrogate-model --title "Submit to ICML" --date 2026-01-22 --hard
cdno project milestone done --slug surrogate-model --query "submit to icml"
```

`--date` is optional. When a milestone is gated by a condition rather than a date, leave it off
rather than inventing an estimate — a made-up date reads back later like a commitment somebody
made:

```bash
cdno project milestone add --slug surrogate-model --title "All Round-1 replies received"
```

An undated milestone records `target: TBD`, stays out of the commitments view, and completes
exactly like a dated one. `--hard` needs a real date.

## Waiting-on

Track external blockers so they're visible instead of forgotten:

```bash
cdno project waiting add --slug surrogate-model --description "Cluster quota increase from IT"
cdno project waiting resolve --slug surrogate-model --query "cluster quota"
```

## Park and re-activate

Parking is first-class and reversible — it's how you respect the five-project cap without deleting
anything:

```bash
cdno project park --slug surrogate-model        # -> projects/_parked/, frees a slot
cdno project activate --slug surrogate-model     # bring it back (must be under the cap)
```

A parked map is still a **complete map**. Parking changes two things only: the project stops
counting, and you read it at the monthly review instead of every day. You can keep writing on it
exactly as on an active one, without activating it first:

```bash
cdno action add --project bayesian-opt --title "Re-read the acquisition-function survey" --energy light
cdno project state --slug bayesian-opt --text "Shelved until the ICML submission is in"
cdno project milestone add --slug bayesian-opt --title "Decide whether to restart" --date 2026-07-01
```

Its hard milestones and dated actions stay in [`cdno commitments`](commitments.md): a deadline on a
parked map is still a promise. What a parked map refuses is *work*: `cdno action start` (and
`switch` and `resume`) needs an active project, because a project you are working on is one you are
moving. If you find yourself working on a parked map, activate it, parking another if the cap is
full.

## Hand a project off

Some projects stay open while someone else is moving them: the survey is with the harbour office,
the draft is with your co-author. Name the holder, and the map says so:

```bash
cdno project hand-off --slug hull-survey --to "the harbour office"
# Handed hull-survey to the harbour office (projects/hull-survey.md)
```

The map gains `held_by: "the harbour office"`, today's daily note gets an entry:

```text
- **09:10**: holder on [[hull-survey]]
  was: (mine)
  now: the harbour office
```

and the project, if active, no longer counts toward the cap: `cdno project list` shows it with a
`held by: the harbour office · does not count toward the cap` line. It stays where it is, keeps its
actions, log and deadlines, and is still read at the weekly review, where the question is not "did
it move?" but "what came back, and what is waiting on my decision?". Say what you are waiting for:

```bash
cdno project waiting add --slug hull-survey --description "Inspection report from the harbour office"
```

When the work comes back to you, take it back:

```bash
cdno project take-back --slug hull-survey
# Took hull-survey back (projects/hull-survey.md)
```

That logs `holder on [[hull-survey]]` with `now: (mine)`, and an active project counts again, so at
the cap the take-back is refused with the projects that count. A parked project stays parked; taking
it back only clears the name.

Three guards keep it honest:

- **If you cannot name who has it, it is yours.** `--to tbd`, `--to someone` or `--to me` is refused
  (the list is [`[projects] holder_sentinels`](../reference/configuration.md#projects)).
- **A hand-off is not made mid-work.** While an action on the project is in focus, pause it first.
- **Handing off is never how you make room at the cap.** Name a holder only when someone else really
  has the project. To free a slot, park, complete or drop one.

## Close a project

A project ends one of two ways, and the log records which:

```bash
cdno project complete --slug surrogate-model                               # the work is done
cdno project drop --slug bayesian-opt --reason "superseded by the ICML work"  # it is not happening
```

Both move the map to `projects/_done/<year>/`, stamp `closed:` with today's date and free the slot;
both work on a parked project too. If anything is still open, the command stops and lists it:

```text
surrogate-model has 2 open items:
  - [ ] Draft methods section with ablation figures (medium)
  - [ ] ICML paper submitted — hard: 2026-05-22
Complete or drop each of them (or add it to the project that now owns it), then run again.
```

Tick what was done, drop what is not happening (each with its own reason), and run `complete`
again. A `drop` can instead let the open items go with the project: it asks first, or takes
`--drop-open` in a script. Commitments you made to other people are never touched by either.

`cdno project list --closed` shows what has ended, newest first, and the MCP weekly and monthly
contexts (`get_weekly_context`, `get_monthly_context`) list the projects that closed in their window. A closed project can come back with
`cdno project activate`, or take the other outcome later if the first one was wrong.

Next: [Research and evidence](research-and-evidence.md).
