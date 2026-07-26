# Stewardships and tracking

A **stewardship** is a small, bounded, perpetual responsibility — your health, your finances, a
service you maintain. Unlike a project it never "finishes"; you just tend it. Stewardships can carry
recurring commitments and, when expanded, time-series **tracking**. Verbs:
[`cdno stewardship`](../reference/cli/stewardship.md) and [`cdno track`](../reference/cli/track.md).

## Two shapes: flat and expanded

```bash
# Flat — a single dashboard file, no tracking. Good for "finances".
cdno stewardship create --name "Finances" --context household

# Expanded — a folder with room for tracking/ and routines/. Use --tracking.
cdno stewardship create --name "Health" --context personal --tracking
```

A flat stewardship is `stewardships/<slug>.md`. An expanded one is `stewardships/<slug>/` with an
`_index.md`, a `tracking/` subfolder for entries, and a `routines/` subfolder for reference docs
(workout plans, checklists — not logs). Only **expanded** stewardships accept tracking entries.

## See what you're tending

```bash
cdno stewardship list                 # each one's variant, tracking count, staleness badge
cdno stewardship show --slug health
```

## Periodic commitments

Recurring obligations attached to a stewardship show up in the aggregated
[commitments](commitments.md) view:

```bash
cdno stewardship add-periodic --stewardship health --title "Dental check-up" \
     --every "every 6 months" --next 2026-09-01
```

`--every` takes a [recurrence](../reference/recurrence.md): `daily`, `weekly`, `monthly`, `yearly`,
or `every N months`.

## Tracking entries

For habits and metrics on an expanded stewardship, file a tracking note. The **activity** is
positional and selects the template — a vault's `.cuaderno/templates/tracking-<activity>.md` if you
have one, else a generic fallback. (Ready-made `gym`/`body`/`swim` variants live in the repo's
`examples/templates/tracking/`; see [Customising templates](templates-and-frontmatter.md#tracking-variants).)

```bash
cdno track gym --stewardship health --content "Upper body A; RDL up to 25kg"
cdno track body --stewardship health --content "Weight 78.4kg, resting HR 54"
cdno track swim --stewardship health --content "1500m, 28min"
```

- `--stewardship` can be omitted when there's exactly **one** expanded stewardship — Cuaderno
  defaults to it. With more than one, it's required.
- `--routine` links a reference doc from the stewardship's `routines/` folder into the entry — but
  only when the resolved template has a `routine:` field (the `gym.md` example variant does; the
  generic default has none, so it no-ops there).
- `--content` is optional; leave it empty and fill the entry's tables in afterward.
- `--at` files the entry for a day that has already passed. Recording lags the event more often
  than not — a statement reconciled at the weekend, a reading taken this morning and typed up
  tonight — and without it the entry lands on the wrong day and the trend bends.

  ```bash
  cdno track body --stewardship health --at 2026-04-06T08:00
  ```

  Filing is always journalled to **today's** daily log, naming the day the entry describes, so a
  backfill stays visible in the record rather than quietly appearing in the past. Dates more than
  50 years back or a year ahead are refused — that far out is a typo, and a typo would reshape a
  trend without saying so.

### Numbers belong in frontmatter

A number written into the body is prose; a number in frontmatter is data. An agent files them
through the MCP `create_tracking_entry` tool's `metrics` parameter, and each key becomes a
frontmatter key:

```yaml
---
type: tracking
stewardship: finances
activity: savings
date: 2026-07-25
balance: 12480.50
contributed: 400.00
---
```

When one entry holds several comparable items — three subjects practised, four categories spent
against — write a **sequence of flat records** rather than one key per item, so the same subject
can recur within the entry:

```yaml
detail:
  - { minutes: 25, subject: harmony }
  - { minutes: 20, subject: harmony }
  - { minutes: 15, subject: sight-reading }
```

Declaring a metric under `[schemas.tracking.fields]` (see
[Configuration](../reference/configuration.md)) gets it type-checked on the way in — a `float` for
a measurement or an amount, an `int` for a count. Anything undeclared is written as given.

Tracking entries are [append-only](../concepts/business-rules.md) — they're your historical record.
Read them back over a window via the MCP `get_stewardship_tracking` tool, or with
[`cdno search`](search.md).

Next: [Weekly review](weekly-review.md).
