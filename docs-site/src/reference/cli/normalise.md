# `cdno normalise`

Reorder note frontmatter into the canonical key order of each note's template (a custom
`.cuaderno/templates/` override if present, else the built-in). Notes `cdno` creates are already
canonical; this fixes hand-authored or migrated notes.

```text
cdno normalise [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--check` | Report out-of-order notes without rewriting them. Exits non-zero if any are out of order. |

Plus the [global options](overview.md#global-options). `normalise` ignores `--json`.

## Examples

```bash
cdno normalise            # rewrite notes into canonical frontmatter order
cdno normalise --check    # report only; non-zero exit if anything is out of order (CI-friendly)
```

`normalise` only reorders existing keys — it never changes values or adds/removes fields. The
canonical order is whatever the matching [template](../../concepts/configuration.md#templates)
defines.

## See also

- [Configuration](../../concepts/configuration.md#templates) — how templates define field order.
- [`lint`](lint.md).
