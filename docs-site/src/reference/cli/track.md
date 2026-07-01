# `cdno track`

File a tracking note under an **expanded** stewardship. The activity is positional and selects the
template: a vault's `.cuaderno/templates/tracking-<activity>.md` if present, else the generic
built-in. No activity-specific templates ship built-in — ready-made `gym`/`body`/`swim` variants are
in [`examples/templates/tracking/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/templates/tracking).

```text
cdno track [OPTIONS] <ACTIVITY>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<ACTIVITY>` | Activity slug — anything you track (e.g. `gym`, `swim`, `reading`). Selects `tracking-<activity>.md` if present, else the generic template. |

## Options

| Flag | Description |
|------|-------------|
| `--stewardship <STEWARDSHIP>` | Stewardship slug. Defaults to the only expanded stewardship if there's exactly one; otherwise required. |
| `--routine <ROUTINE>` | Bare slug of a routine doc — wrapped into `[[stewardships/<slug>/routines/<routine>]]` and substituted into the template's `routine:` field. |
| `--content <CONTENT>` | Inline body. Optional; defaults to empty so you can fill in tables afterward. |
| `--var <NAME=VALUE>` | Value for a custom tracking template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Prompts come from the activity's template (e.g. `tracking-gym`) when one exists. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result and runs non-interactively.

## Examples

```bash
cdno track gym --stewardship health --content "Upper body A; RDL up to 25kg"
cdno track body --stewardship health --content "Weight 78.4kg, resting HR 54"
cdno track swim --stewardship health --routine endurance-1

# With one expanded stewardship, --stewardship can be omitted:
cdno track gym --content "Legs day"
```

Tracking entries are [append-only](../../concepts/business-rules.md) and only land in **expanded**
stewardships (those created with `--tracking`).

## Related MCP tool

[`create_tracking_entry`](../mcp/writes.md).

## See also

- [Stewardships and tracking](../../tutorials/stewardships-and-tracking.md).
- [`stewardship`](stewardship.md).
