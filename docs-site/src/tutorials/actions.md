# Actions

An **action** is the next concrete thing to do on a project. Cuaderno treats actions as *cheap by
default*: the normal form is a single bullet on the project map, not a note you have to create and
maintain. You only give an action its own note when it grows into real investigation. All verbs live
under [`cdno action`](../reference/cli/action.md).

## Add a next action

```bash
cdno action add --project surrogate-model --title "Profile the assembly step" --energy medium
```

This appends a checkbox bullet to the project's next-actions list, tagged with its
[energy](../concepts/contexts-and-energy.md). That's usually all an action ever is.

## List open actions

```bash
cdno action list --project surrogate-model
```

Bullets that have been promoted to notes show their status (active / blocked / completed /
dropped) inline.
`--json` gives the structured list.

## Promote an action to a manifest note

When an action becomes an investigation that spans days and produces evidence, promote it. This
rewrites the bullet as a wikilink and scaffolds an `action` note (with status, energy, criteria,
links). The match is a case-insensitive substring of the bullet text; energy is inherited:

```bash
cdno action promote --project surrogate-model --query "profile the assembly"
```

You can also create an action *already* promoted by passing `--note` to `add`:

```bash
cdno action add --project surrogate-model \
                --title "Characterise sample efficiency across mesh sizes" \
                --energy deep --note
```

## Complete an action

Completing matches a bullet by substring, ticks it off, and logs it to today's journal. If the
action had a manifest note, that note is archived to `actions/_done/<year>/` and becomes append-only:

```bash
cdno action complete --project surrogate-model --query "feature set B"
```

## Drop an action

Not everything on the list gets done. When an action is superseded, abandoned or reprioritised,
drop it rather than completing it:

```bash
cdno action drop --project surrogate-model --query "demo proposal" \
    --reason "superseded by the demo-planning action"
```

This matches and archives exactly like `complete`, but the note is stamped `status: dropped` with no
completion date, and the journal records `action dropped on …` instead of `action done on …`.

The distinction matters more than it looks. The daily log is what the weekly review, the monthly
scan and every later verdict read back from — so completing work that was never performed leaves
your own vault asserting something untrue, and the only repair is a correction line written by hand.
A dropped action also stays out of the completed-actions views, because it carries no completion
date.

`--reason` is optional and worth giving. "Superseded by X" and "no longer wanted" are different
facts, and only one of them tells you to go looking for the replacement.

## Inline vs. manifest — when to promote

| Use an **inline bullet** (default) | Promote to a **manifest note** |
|------------------------------------|--------------------------------|
| A discrete next step | An investigation spanning multiple days |
| Done in one sitting | Produces evidence / artefacts to link |
| No supporting detail needed | Needs success criteria, status, or its own notes |

Keeping the default cheap is deliberate — it means capturing the next step never costs more than a
sentence. See [The Research Logbook Method](../concepts/rlm.md#actions-not-tasks) for the rationale.

Next: [Commitments and deadlines](commitments.md).
