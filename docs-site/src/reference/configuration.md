# Configuration reference

The vault's settings live in `.cuaderno/config.toml`, written by `cdno init`. Every key is optional —
defaults are applied when omitted. For the conceptual tour, see
[Configuration](../concepts/configuration.md).

## Full example

```toml
[vault]
name = "My Research Vault"
max_active_projects = 5            # the active-project cap

# Glob patterns excluded from the index (search, lint, link checks).
# Additive only — no "!" negation. Matched against vault-relative paths.
# NEVER deletes files; only scopes what the index considers.
ignore = ["CLAUDE.md", "README.md"]

# Per-type extra required frontmatter fields. Built-in required fields
# are always enforced; these add vault-specific requirements (cdno lint).
[schemas.project]
extra_required = ["collaborators"]

[schemas.evidence]
extra_required = []

# Typed frontmatter fields for a built-in type. Recognised by the desktop
# Templates editor and type-checked by `cdno lint`.
[schemas.daily.fields.meds]
type = "bool"                     # bool | int | float | string | date
default = false                  # static, type-checked against `type`

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]   # allowed values (a string constraint)
default = "ok"

# How an activity's tracked numbers are read back. Each metric declares how
# it collapses to one point per date; without a declaration the activity's
# series come from its body table, with each column summed.
[tracking.practice]
records  = "detail"               # frontmatter key holding repeated records
group_by = "subject"              # one series per distinct value

[tracking.practice.metrics.minutes]
aggregate = "sum"                 # a TOTAL
unit      = "min"

[tracking.practice.metrics.focus]
aggregate = "mean"                # a RATING; sum would grow with how often you log

# Static template variables — resolve in any custom template ({{author}}).
[variables]
author = "A. Researcher"

# Prompted variables — gathered at note creation (--var, prompt, or error).
[variables.prompt]
collaborators = "Who are the collaborators?"
```

## Keys

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `vault.name` | string | `"My Vault"` | A human label for the vault. |
| `vault.max_active_projects` | integer | `5` | The active-project cap. |
| `ignore` | list of globs | `[]` | Files the index skips. Additive; never deletes. See [Ignore globs](#ignore-globs). |
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that **built-in** note type, enforced by `cdno lint`. |
| `schemas.<type>.fields.<name>` | table | — | A **typed** frontmatter field for a built-in note type (`type`, `default`, `required`, `values`, `settable`, `log_on_change`). Recognised by the Templates editor, type-checked by `cdno lint`, and (when `settable`) writable via `cdno frontmatter set`. See [Typed schema fields](#typed-schema-fields). |
| `tracking.<activity>` | table | — | Declares how an activity's tracked numbers are read back (`records`, `group_by`, and a `metrics.<name>` table per metric). Without one, the activity's series come from its body table with each column summed. See [Tracking](#tracking). |
| `note_types.<name>` | table | — | Declares a **config-defined custom note type** (`folder`, `required`/`optional` fields, `template`, …) — a schema-only type for entities the built-ins don't cover. See [Custom note types](custom-note-types.md). |
| `variables.<name>` | string | — | Static template variable; resolves in any custom template (per-type values win on name clash). |
| `variables.prompt.<name>` | string | — | Prompted template variable; the value is the prompt text. Gathered at creation from `--var name=value`, an interactive prompt, or a static `[variables]` default; errors if none supplies it. |

## Ignore globs

`ignore` lists files that live in the vault directory but are not notes — repo scaffolding like
`CLAUDE.md` or `README.md`. They are excluded from the index, and therefore from **search, lint and
backlinks** as well. The files are never touched on disk.

Patterns are matched against each file's vault-relative path:

| Pattern | Matches |
|---|---|
| `CLAUDE.md` | that file at the vault root |
| `**/*.draft.md` | a `.draft.md` at any depth |
| `folder/*/**` | everything **one or more** levels below `folder/<anything>/` |
| `folder/*/*/**` | everything **two or more** levels below `folder/<anything>/` |

The last two are the trap worth knowing. `*` stays inside one path segment but `**` is recursive, so
`portfolios/*/**` does not mean "the level below a portfolio" — it matches the portfolio's own notes
as well as anything nested under them. A glob written that way excludes every note in the folder it
was meant to tidy, and because an unindexed note is also unsearchable and unlinkable, the symptom
looks like a broken view rather than a misconfigured vault.

Two things guard against that:

- `cdno reindex` prints how many files the globs excluded.
- The desktop app shows a dismissible notice when the count looks disproportionate — a lone
  `CLAUDE.md` stays silent, a glob swallowing a large share of the vault does not.

If notes go missing, clear the pattern and run `cdno reindex`: every row comes back.

Note that attachment artefacts filed into a portfolio need no `ignore` entry — they are excluded
automatically, by location. See [vault structure](../concepts/vault-structure.md).

## Typed schema fields

`[schemas.<type>.fields.<name>]` declares a **typed** frontmatter field on a built-in note type. It
is the richer sibling of `extra_required`: instead of just a name, each field carries a type (and
optionally a default and an allowed-value set). Four things consume it today:

- the desktop **Templates editor** recognises the field, so a custom template referencing
  `{{<name>}}` no longer warns "renders literally";
- **note creation** populates the field's `default` at create — a custom template referencing
  `{{<name>}}` renders that default (a field with no default renders `null`), so the value lands in
  the new note's frontmatter instead of a literal `{{<name>}}`;
- **`cdno lint`** type-checks the field — a note whose value doesn't match the declared type (or
  isn't one of `values`) gets a warning;
- the **`set_frontmatter` setter** (`cdno frontmatter set`, MCP `set_frontmatter`) writes the field
  through the index when it is marked `settable = true` — see the
  [`cdno frontmatter` reference](cli/frontmatter.md).

```toml
[schemas.daily.fields.meds]
type = "bool"                     # bool | int | float | string | date
default = false                  # optional; static, type-checked against `type`
settable = true                  # optional; allow `set_frontmatter` to write it (default false)
log_on_change = true             # optional; stamp a daily-log line when it changes

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]   # optional; allowed values (only valid on a string)
default = "ok"
required = false                 # optional; default false
```

| Field key | Type | Default | Purpose |
|-----------|------|---------|---------|
| `type` | `"bool"` \| `"int"` \| `"float"` \| `"string"` \| `"date"` | *(required)* | The field's scalar type. An unknown value is a hard load error. |
| `default` | matching `type` | — | A static default value, type-checked at load. **Populated at create** when a custom template references `{{<name>}}`. A `date` is a quoted `"YYYY-MM-DD"`. |
| `required` | bool | `false` | Reserved for create-time enforcement (a later release); parsed now, but inert — it does not yet block creation. |
| `values` | list of strings | — | An allowed-value constraint. Valid only on a `string` field. |
| `settable` | bool | `false` | Whether `set_frontmatter` (`cdno frontmatter set`, MCP `set_frontmatter`) may write this field. **Default-deny**: absent or `false` means not settable. Never overrides an engine-owned key (`type`, `status`, a period key) — those stay blocked regardless. |
| `log_on_change` | bool | `false` | When a `settable` field's value actually changes, stamp a `key: old → new` line into today's daily note in the same commit. |

Notes and limits:

- **Defaults are static** — there is no `"today"` token; a `date` default is a literal calendar date.
- **A field only lands in frontmatter if a custom template references it** — rendering substitutes
  the `{{<name>}}` tokens a template contains; it never adds a frontmatter line. The shipped built-in
  templates can't reference vault-specific fields, so populate a declared field by adding a custom
  `.cuaderno/templates/<type>.md` that references `{{<name>}}`.
- **A create-path value wins over a declared default** — if the note type's create path already
  supplies a value for that name (an engine-supplied placeholder), that value takes precedence and
  the declared default does not apply. Likewise a `[variables]` static var of the same name wins over
  a schema default.
- **A `[variables.prompt]` name is owned by the prompt** — if a field name is also a prompted
  variable, its value is collected via the prompt (from `--var`, an interactive prompt, or a static
  default), and the schema default is not used. This ensures a supplied answer is never discarded.
- **No `enum` type** — model a closed set as a `string` with `values`.
- **`int` and `float` are distinct** — `int` rejects anything with a fractional part, so a currency
  amount, a measurement or a rate wants `float`. A `float` field accepts a whole number too (`82` and
  `82.5` both validate), because a round reading is written without a decimal point. A non-finite
  default (`nan`, `inf`) is a load error — those values have no meaning as data.
- **List fields are reserved but not yet implemented** — a `list = true` is a load error today.
- **Engine-owned keys are protected** — you can't declare a field named `type`, or a calendar type's
  own period key (`daily`→`date`, `weekly`→`week`, `monthly`→`month`); the vault refuses to open.
  `set_frontmatter` additionally refuses to write `status` for every type — even if a vault declares
  a `status` field `settable = true` — so the lifecycle commands stay its sole writers.
- **`extra_required` still works** and is equivalent to an untyped, non-required `string` field; on a
  name clash an explicit `fields` block wins.
- A malformed field declaration (unknown `type`, a mistyped key, `values` on a non-string, a
  `default` that doesn't type-check) fails at vault-open, like every other config error.

## Tracking

`[tracking.<activity>]` declares how an activity's numbers are read back. Without one, a tracking
note's series come from the first table in its body, with each column summed — right for a rep
sheet, wrong for a balance or a rating. Declaring an activity moves it to frontmatter, where each
metric says how it collapses.

```toml
[tracking.practice]
records  = "detail"        # frontmatter key holding repeated records
group_by = "subject"       # one series per distinct value of this field

[tracking.practice.metrics.minutes]
type      = "int"
aggregate = "sum"          # a TOTAL
unit      = "min"
plot      = "column"

[tracking.practice.metrics.focus]
aggregate = "mean"         # a RATING - a sum would grow with how often you log
```

| Key | Where | Purpose |
|-----|-------|---------|
| `records` | activity | Frontmatter key holding a sequence of flat records. Omit for plain scalars read straight off the entry. |
| `group_by` | activity | Record field the series split on — a category, a subject, a person. One series per distinct value. |
| `at` | *record* | Not a config key but the per-record field that orders a set, so `last` reads the reading you meant. `HH:MM`, `HH:MM:SS`, or a 12-hour time with a meridiem. Needs no quoting — the colon keeps it text — but a colon-less `at: 1800` is a number and never reaches the parser. All-or-nothing: `cdno lint` reports a value it cannot use, or a set only partly stamped. |
| `type` | metric | `bool` \| `int` \| `float` \| `string` \| `date`. Optional. |
| `aggregate` | metric | `sum` \| `mean` \| `last` \| `max` \| `min`. Defaults to `sum`. |
| `group_by` | metric | Overrides the activity's. `"none"` collapses across records for an entry-level series. |
| `derived` | metric | An expression computing this metric from sibling fields, e.g. `"km * rate_per_km"`. Evaluated **per record, before aggregation**. Declare `type` on it and the vault refuses to open. |
| `unit` | metric | Display unit (`min`, `kg`, `EUR`). Carried through to the chart and the MCP series. |
| `label` | metric | Display name for the series, when the metric's key is not what you want on a chart (`resting_hr` → `Resting heart rate`). |
| `plot` | metric | `none` \| `line` \| `column` \| `area` \| `scatter`. Defaults to `none`. Chooses the **mark** the chart draws, and whether the desktop draws it at all — see the note below. |

### Derived metrics

Some tracked quantities are products of others — a cost from a rate and a distance, a load from a
weight and a count. Rather than making whoever writes the entry pre-compute them, declare the
expression:

```toml
[tracking.commute.metrics.km]
aggregate = "sum"

[tracking.commute.metrics.rate_per_km]
aggregate = "last"

[tracking.commute.metrics.cost]
derived   = "km * rate_per_km"
aggregate = "sum"
unit      = "EUR"
```

It is evaluated **per record, then aggregated** — not derived from the aggregates. With two trips
of 10 km at 0.50 and 20 km at 0.25, `cost` is 10.00; deriving from the totals would give a
different, wrong number the moment the rate varies.

The grammar is deliberately tiny — one binary operation, nothing else:

```
expr    := operand OP operand
operand := field-name | number
OP      := '+' | '-' | '*'
```

- **No `/`.** Division is the one operator that manufactures NaN and infinity, and those must
  never reach an aggregate. Multiply by the reciprocal, or pre-compute the ratio.
- **No parentheses, calls, chaining or recursion**, and no deriving from another derived metric.
- **Field names** are letters, digits and `_`, not starting with a digit. Hyphens are excluded
  because `a-b` would be indistinguishable from a subtraction.
- **Numbers are plain decimal** — digits, an optional leading `-`, an optional `.`. Exponent
  notation (`1e-3`) is not supported; write the value out in full.
- **Every operand must name a metric the same activity declares.** A typo is a vault-open error
  naming the field, rather than a silently empty chart. The cost of that requirement: an operand
  that exists only to be multiplied — a rate, say — still becomes a metric of its own. Leave its
  `plot` undeclared (the default) and the desktop will not chart it alongside the result — see
  below.
- **`type` must be omitted** — the output is numeric by construction, so declaring one can only
  contradict it.
- **A record missing an operand contributes nothing** — a gap, on the same rule as a plain metric.
  So does a result that is not finite.

Choosing the aggregate is the whole point, and it follows from what the number *is*:

| Kind | Examples | Aggregate |
|------|----------|-----------|
| Total | amount spent, pages read, minutes practised | `sum` |
| Level | account balance, a measurement, a top set | `last`, `max` |
| Rate or rating | a score out of ten, perceived difficulty | `mean` |

Notes and limits:

- **An absent `[tracking]` section is not an error** — an undeclared activity keeps being served
  from its body table, so nothing forces a migration.
- **Declaring is checked at vault-open**, not at first chart render: an unknown `aggregate`, a
  mistyped key, or a blank `records`/`group_by` fails when the vault is opened, naming the key.
- **An activity may declare no metrics at all.** That is a complete use — recording that something
  happened, with nothing to aggregate.
- **`window` is reserved** for a future time-reduction axis (month-over-month deltas, rollups) and
  is a load error today, so adding the behaviour later is not a breaking change.
- **A declared activity's body table is no longer read** once its frontmatter yields a series, so
  the same metric can never appear twice under two disagreeing numbers. This rule is unconditional
  — it keys on the frontmatter derivation's full produced set, not on what any individual metric's
  `plot` says, so a declared-but-unplotted metric still suppresses its body-table equivalent.
- **`plot` chooses the mark, and gates whether the desktop draws it.** A declared `line`/`column`
  is used as the chart's mark (an `area` or `scatter` resolves to the closest of the two the chart
  draws). `plot = "none"` — the default for a declared metric that names no mark — is still
  emitted and still queryable over MCP, but the desktop leaves it out of the chart pane. Declaring
  an activity is an explicit act, and is allowed to change what is drawn: its frontmatter series
  replace its body-table ones (the rule above, unaffected by this), and only the metrics that opt
  into a mark are charted.

## Templates

Templates live in `.cuaderno/templates/` and are pure variable substitution. `cdno init` writes one
starter (`daily.md`); other types use their built-in default until you add a file. `cdno` selects the
most specific template that exists: a custom variant (e.g. `tracking-gym.md`), then a custom type
(e.g. `project.md`), then the built-in variant default, then the built-in type default. Template
field order is the canonical order
[`cdno normalise`](cli/normalise.md) enforces.

The per-type placeholders that resolve at creation, with a worked example, are in
[Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md). An unknown
placeholder is left verbatim in the note.

Static `[variables]` resolve in any custom template (e.g. `{{author}}`). Prompted
`[variables.prompt]` are gathered at creation (via `--var name=value`, an interactive prompt, or a
static default) — see the
[tutorial](../tutorials/templates-and-frontmatter.md#prompted-variables).

## Editing from the desktop app

You can edit `.cuaderno/config.toml` directly from the desktop app's **Config** view, without
hand-editing the file. It offers a **Raw** text editor and a structured **Form** for note types and
schema extensions; **Check** dry-runs the same validation the app runs when it opens a vault.

Saving is gated so an edit — from either view — can never leave the vault unopenable:

1. The whole candidate is **validated first** — the exact check the app runs at open (TOML parse,
   `ignore` globs, and the `[note_types.*]` / `[schemas.*]` rules). If it would not reopen, the save
   is refused and the file is left untouched.
2. A **content-hash compare-and-swap** then guards against a concurrent hand-edit: if the file
   changed on disk since the editor read it, the save is refused with a "changed on disk — reload"
   notice rather than overwriting the newer version.
3. The vault is then **reloaded live**, so the edit applies with no restart. A Raw save writes the
   buffer **verbatim**; a Form save applies a **surgical** edit to just the table it changed — either
   way comments, key order, and the `[variables]` block are preserved.

The full walkthrough of the Config view is in
[Editing the config in the app](../getting-started/config-editor.md).

## See also

- [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md) — the tutorial.
- [Configuration](../concepts/configuration.md) — the conceptual overview.
- [Frontmatter fields](frontmatter.md) — what `extra_required` extends.
- [Custom note types](custom-note-types.md) — the `[note_types.*]` table in full.
