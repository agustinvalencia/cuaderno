# Example tracking templates

Cuaderno's `tracking` note type ships **one** built-in template — a neutral `generic` shape (an H1
plus a `## Notes` section). Activity-specific variants are entirely **per-vault**: drop a file at
`<vault>/.cuaderno/templates/tracking-<activity>.md` and any `cdno track --activity <activity>` (or
the `create_tracking_entry` MCP tool) picks it up automatically. The resolver slugifies the activity
and looks up `tracking-<slug>`, falling back to `generic` when there's no match — nothing about
specific activities is baked into the binary.

These three files are ready-made variants you can use as-is or adapt:

| File | Activity slug | Shape |
|------|---------------|-------|
| `gym.md` | `gym` | 5-column exercise table (`Exercise / Sets / Reps / Weight / Notes`) + `routine:` / `duration_min:` frontmatter |
| `body.md` | `body` | Body-metrics table (`Weight / Waist / Sleep`) |
| `swim.md` | `swim` | Swim-set table (`Set / Distance / Stroke / Time / Notes`) + `duration_min:` |

## Install one

```bash
# From the vault root — e.g. to use the gym exercise table:
mkdir -p .cuaderno/templates
cp <path-to-cuaderno>/examples/templates/tracking/gym.md .cuaderno/templates/tracking-gym.md
```

Then `cdno track --stewardship <expanded-stewardship> --activity gym` renders the exercise table.
Edit the copied file freely — a custom template always wins over the built-in.

## Roll your own

Copy `gym.md` to `tracking-<your-activity>.md`, change `activity:` and the H1, and reshape the table
for what you track. `cdno templates vars tracking` lists the `{{placeholders}}` the create path
supplies (`stewardship`, `activity`, `activity_title`, `date`, `date_long`, `content`, `routine`).
