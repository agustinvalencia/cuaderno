# Commitments and deadlines

A **commitment** is a dated promise — to others, or a hard promise to yourself. Cuaderno keeps these
distinct from your to-do list and gives you one aggregated view of everything with a deadline,
wherever it lives.

## The aggregated view

```bash
cdno commitments              # everything due, sorted by date, overdue flagged
cdno commitments --weeks 6    # look six weeks ahead instead of the default two
```

[`cdno commitments`](../reference/cli/commitments.md) is a **computed** view. It always includes a
30-day overdue look-back on top of the lookahead window, so nothing slips silently into the past. It
draws from four sources (see [Business rules](../concepts/business-rules.md#commitments-are-aggregated-not-stored-in-one-place)):

1. **Project milestones** marked `--hard`.
2. **Stewardship periodic commitments** (recurring dashboard lines).
3. **Standalone commitment notes** (below).
4. **Action notes** with a self-imposed `due:` not tied to a milestone.

## Standalone commitments

For a one-off promise that isn't naturally a project milestone or a recurring stewardship line:

```bash
cdno commit create --title "Pay rent" --due 2026-06-01 --context personal
# -> commitments/pay-rent.md

# Optionally attribute it to a project or stewardship:
cdno commit create --title "Review Erik's draft" --due 2026-05-20 --context work --project projects/icml-paper
```

When it's fulfilled, mark it done — it's stamped and moved to `commitments/_done/<year>/`:

```bash
cdno commit done --slug pay-rent
```

## When a promise ends without being kept

Not every promise is kept. The client cancels, the plan changes, the thing is overtaken by events.
Marking it done would be the wrong record — `cdno commitments` and every weekly and monthly review
read the daily log back, and a completion says the promise was fulfilled:

```bash
cdno commit drop --slug quarterly-report --reason "the client cancelled the engagement"
```

The note is archived beside your completed ones, stamped `status: dropped` with no completion date,
so it never counts as finished work. `--reason` is optional; a typo needs no explanation, a
cancellation usually deserves one.

If the date merely moved, [`reschedule`](../reference/cli/commit.md#cdno-commit-reschedule) is the
verb you want — it records the move rather than ending the promise.

Nothing is destroyed by a drop: the note keeps its body and its `created` date. But a drop is
re-decided rather than undone — there is no un-drop. If the promise comes back, make it again, and
the record honestly shows both decisions.

## Deadlines that live elsewhere

You often don't need a standalone note — put the deadline where the work is:

```bash
# A hard project milestone shows up in `cdno commitments`:
cdno project milestone add --slug icml-paper --title "Camera-ready" --date 2026-02-01 --hard

# A recurring obligation on a stewardship dashboard:
cdno stewardship add-periodic --stewardship finances --title "File quarterly taxes" \
     --every "every 3 months" --next 2026-07-15
```

Both surface in the same aggregated list, so `cdno commitments` is the single place to answer "what
have I promised, and when?".

Next: [Stewardships and tracking](stewardships-and-tracking.md).
