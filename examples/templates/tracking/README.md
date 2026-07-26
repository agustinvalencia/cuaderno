# Example tracking templates

Cuaderno's `tracking` note type ships **one** built-in template — a neutral `generic` shape (an H1
plus a `## Notes` section). Activity-specific variants are entirely **per-vault**: drop a file at
`<vault>/.cuaderno/templates/tracking-<activity>.md` and any `cdno track <activity>` (or the
`create_tracking_entry` MCP tool) picks it up automatically. The resolver slugifies the activity
and looks up `tracking-<slug>`, falling back to `generic` when there's no match — nothing about
specific activities is baked into the binary.

These files are ready-made variants you can use as-is or adapt:

| File | Activity slug | Shape |
|------|---------------|-------|
| `gym.md` | `gym` | 5-column exercise table (`Exercise / Sets / Reps / Weight / Notes`) + `routine:` / `duration_min:` frontmatter |
| `body.md` | `body` | Wide measurements table — one column per metric |
| `swim.md` | `swim` | Swim-set table (`Set / Distance / Stroke / Time / Notes`) + `duration_min:` |
| `spending.md` | `spending` | Frontmatter records, grouped by category |
| `reading.md` | `reading` | Plain frontmatter scalars |

Tracking is domain-neutral. The first three are fitness-shaped because that is a common starting
point, not because the engine knows anything about exercise — the last two are here to make that
concrete.

## Where the numbers should live

Two substrates, and the choice decides what you can ask later.

**Frontmatter** (`spending.md`, `reading.md`) is the one to reach for. Values are indexed as data,
each metric reduces by its own rule, and a field like `category` can split one entry into a series
per value. Declare the contract for an activity and each metric says how it collapses: `sum` for a
total, `last` for a level such as a balance or a measurement, `mean` for a rate or a rating. An
entry holding several comparable items writes a **sequence of flat records** (`spending.md`), so
the same category can appear twice in one entry and still land in one series.

**A body table** (`gym.md`, `body.md`, `swim.md`) is the older route and still works. It reads the
first table in the note and **sums each column** — right for a count, wrong for anything else — so
it suits a rep sheet better than a set of measurements.

## Wide, not long — if you use a table

This is the rule that bites, and it is why `body.md` looks the way it does. A table column *is* a
series, so put **one column per metric and one row per entry**:

```markdown
| Weight (kg) | Waist (cm) | Sleep (h) |
|-------------|------------|-----------|
| 78.4        | 82         | 7.2       |
```

Three series, each carrying its own value. The tempting alternative — a `Metric` / `Value` pair per
row — gives **one** series whose value is weight + waist + sleep added together:

```markdown
| Metric      | Value |    <-- do not do this
|-------------|-------|
| Weight      | 78.4  |
| Waist       | 82    |
| Sleep (avg) | 7.2   |
```

The engine has no way to know those rows are unrelated: it sees one numeric column and sums it, and
167.6 is not a number that means anything. A long shape is fine when the rows genuinely belong to
the same quantity — the sets of one exercise, the legs of one swim — because then the sum is the
session total you wanted.

## Install one

```bash
# From the vault root — e.g. to use the gym exercise table:
mkdir -p .cuaderno/templates
cp <path-to-cuaderno>/examples/templates/tracking/gym.md .cuaderno/templates/tracking-gym.md
```

Then `cdno track gym --stewardship <expanded-stewardship>` renders the exercise table.
Edit the copied file freely — a custom template always wins over the built-in.

## Roll your own

Copy the closest variant to `tracking-<your-activity>.md`, change `activity:` and the H1, and
reshape it for what you track. The create path supplies `stewardship`, `activity`,
`activity_title`, `date`, `date_long`, `content`, and `routine` — reference any as
`{{placeholder}}`. (`cdno templates vars tracking` lists this complete set — including `routine` —
since it comes from the create path, not from whichever template is effective.)
