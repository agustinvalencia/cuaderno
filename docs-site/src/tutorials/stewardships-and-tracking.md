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
have one, else a generic fallback. (Ready-made variants live in the repo's
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
  cdno track body --stewardship health --at 2026-04-06
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

A metric that reports a **level** rather than a total — a balance, a measurement, the last set of
the day — reduces to the last record in the entry, and "last" means the order the records appear in
the file. If you append them out of order, give **every** record an `at` field and they sort by it:

```yaml
detail:
  - { balance: 1200, at: "09:00" }
  - { balance: 1240, at: "18:00" }
```

It is all-or-nothing: if any record lacks an `at`, or carries one that does not parse (`09:00`,
`9:00`, `9:00 AM` and their with-seconds forms all do), the file's own order stands and nothing is
reordered. The key is `at` rather than `time` precisely because `time` is a plausible *metric* —
a swim split, a lap time — and ordering a record set by one of its own measurements would report
a number that was never the last reading.

Declaring a metric under `[schemas.tracking.fields]` (see
[Configuration](../reference/configuration.md)) gets it type-checked on the way in — a `float` for
a measurement or an amount, an `int` for a count. Anything undeclared is written as given, except
the four keys that identify the note — `type`, `stewardship`, `activity`, `date` — which a metric
may not name. They are the note's identity rather than data about it, and the engine owns them:
`date` is fixed by the filename, and `activity` is what every reader groups by.

Tracking entries are [append-only](../concepts/business-rules.md) — they're your historical record.
Read them back over a window via the MCP `get_stewardship_tracking` tool, or with
[`cdno search`](search.md).

## From entries to series

Filing an entry is half the story. The other half — how those numbers turn into a chart, and why a
series sometimes shows a number that means nothing — comes down to two things: which **aggregate**
each metric declares, and which **source**, body table or frontmatter, the activity is read from.
Every `[tracking.<activity>]` key is documented in full in the
[Tracking](../reference/configuration.md#tracking) section of the configuration reference; this is
the plain-language version of why those keys exist.

### The metric-kind taxonomy

Every number you track is one of four kinds, and the kind decides the aggregate it wants:

| Kind | Examples | Aggregate |
|------|----------|-----------|
| Total | amount spent, pages read, minutes practised | `sum` |
| Level | account balance, a measurement, a top set | `last`, or `max` for a high-water mark |
| Rate or rating | a score out of ten, perceived difficulty | `mean` |
| Occurrence | a call, a visit | none — declare no metrics; the entry's existence is the record |

Summing is right for exactly one of these — a total — and wrong for the rest. It's wrong for a
level because it adds successive readings of one quantity instead of reporting the quantity: three
statements through a spending-free month, each reading a balance of roughly 12,480, would sum to
over 37,000 — a number nothing in the account corresponds to. It's wrong for a rating because the
series grows with how often you record it, not with how the session actually went — log twice a day
instead of once and the total doubles for no reason but the logging cadence. An occurrence doesn't
want a number at all: declaring the activity with no metrics is a complete, valid use — the record
is "this happened", not "how much".

### Two worked examples

**A scalar activity, a level.** The `savings` activity shown above has no repeated records — every
entry is one reading. `balance` is a level, so it wants `last`, not the default `sum`; `contributed`
is a total, so the default is already right:

```toml
[tracking.savings]
# no `records` — this activity's metrics are scalars read straight off the entry

[tracking.savings.metrics.balance]
aggregate = "last"     # a LEVEL — the reading itself, not a running total of readings
unit      = "EUR"

[tracking.savings.metrics.contributed]
aggregate = "sum"      # a TOTAL — money added since the last entry
unit      = "EUR"
```

Filed against the frontmatter shown earlier (`balance: 12480.50`, `contributed: 400.00`), this
produces two series: `savings · balance`, whose points are each entry's balance verbatim, and
`savings · contributed`, whose points are the sum of every contribution recorded on that date.

**A record-based activity, two aggregates over the same records.** `practice` splits by `subject`
and tracks two different kinds of number per session:

```toml
[tracking.practice]
records  = "detail"        # frontmatter key holding the repeated records
group_by = "subject"       # one series per distinct subject

[tracking.practice.metrics.minutes]
type      = "int"
aggregate = "sum"          # a TOTAL — minutes practised
unit      = "min"

[tracking.practice.metrics.focus]
aggregate = "mean"         # a RATING — sum would grow with how often you log, not with how focused you were
```

One entry covering two subjects in the same sitting:

```yaml
---
type: tracking
stewardship: study
activity: practice
date: 2026-07-25
detail:
  - { minutes: 25, focus: 8, subject: harmony }
  - { minutes: 20, focus: 6, subject: harmony }
  - { minutes: 15, focus: 9, subject: sight-reading }
---
```

Four series come out of it, named `<activity> · <group> · <metric>`:

- `practice · harmony · minutes` = 45 (25 + 20, summed)
- `practice · harmony · focus` = 7 (the mean of 8 and 6)
- `practice · sight-reading · minutes` = 15
- `practice · sight-reading · focus` = 9

Same records, same date, two different reductions — because `minutes` and `focus` are different
kinds of number, not because one config option was chosen for the whole activity.

### Gaps, not zeros

A subject you didn't practise on a given day produces **no point** on that day, not a zero.
Zero-filling would draw a false line down to the axis — a session that didn't happen is not the same
as a session scored zero. The same rule means a brand-new subject needs no configuration change: the
first time `ear-training` shows up in a `detail` record, its series starts there, on that date, with
every date before it correctly absent rather than zero.

### Which source a series comes from

An **undeclared** activity — no `[tracking.<activity>]` for it in `config.toml` — is read from the
first table in the note's body: one series per column, each column's numeric cells summed. That's
right for a rep count and wrong for almost everything else, which is why the declared path above
exists. If you're still filing through a table, see
[Wide, not long](https://github.com/agustinvalencia/cuaderno/blob/main/examples/templates/tracking/README.md)
in the example templates — a table column *is* a series, and a `Metric` / `Value` row shape sums
unrelated numbers into one meaningless total.

Declaring an activity moves it to frontmatter: once the declaration yields a series for that
activity, its body tables are no longer read — every note of it, not just the ones carrying
frontmatter metrics. So the same metric can never appear twice under two disagreeing numbers, one
from a table and one from the declared reduction. It also means a half-migrated activity shows only
what its frontmatter carries, so migrate an activity's notes together rather than one at a time.

Next: [Weekly review](weekly-review.md).
