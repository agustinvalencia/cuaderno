# Managing projects

A **project** is a lightweight map of a piece of active work: a current state, next actions,
milestones, and things you're waiting on. You keep at most [five active](../concepts/business-rules.md)
at once. All the verbs live under [`cdno project`](../reference/cli/project.md) (plus
[`cdno action`](action.md) for the next-action list).

## Create one

```bash
cdno project create --title "Surrogate model" --context work
# -> projects/surrogate-model.md   (slug derived from the title)
```

`--context` is the [life domain](../concepts/contexts-and-energy.md). Optionally link the project's
core question with `--question questions/research/surrogate-cost`. If you're already at five active
projects, the new one is created **parked** — activate it once you free a slot.

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

Milestones are dated markers of progress. Mark one `--hard` to make it a real deadline that shows up
in the aggregated [commitments](commitments.md) view:

```bash
cdno project milestone add --slug surrogate-model --title "Submit to ICML" --date 2026-01-22 --hard
cdno project milestone done --slug surrogate-model --query "submit to icml"
```

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

Next: [Research and evidence](research-and-evidence.md).
