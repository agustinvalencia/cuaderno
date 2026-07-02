# `cdno search`

Full-text search across all notes, ranked best-first, with optional filters by note type, date
window, and portfolio.

```text
cdno search [OPTIONS] <QUERY>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<QUERY>` | Search text. Matched case-insensitively; terms are ANDed. Quotes and operators are treated as literal words. |

## Options

| Flag | Description |
|------|-------------|
| `--type <TYPE>` | Restrict to one note type (e.g. `daily`, `project`, `evidence`, or a [config-defined custom type](../custom-note-types.md)). A name that is neither built-in nor a registered custom type errors with the valid set; tab-completion offers your vault's types. |
| `--from <FROM>` | Inclusive earliest note date (`YYYY-MM-DD`). |
| `--to <TO>` | Inclusive latest note date (`YYYY-MM-DD`). |
| `--portfolio <PORTFOLIO>` | Restrict to notes in this portfolio. |
| `--limit <LIMIT>` | Maximum results. Default `20`. |

Plus the [global options](overview.md#global-options). With `--json`, emits an array of hits, each
with `path`, `note_type`, `title`, `snippet`, and `score`.

## Examples

```bash
cdno search "preconditioner"
cdno search "sparse attention" --type evidence
cdno search "mesh" --from 2026-03-01 --to 2026-03-31 --limit 5
cdno search "speedup" --portfolio sparse-vs-dense-attention-ood --json | jq -r '.[].path'
```

## Related MCP tool

[`search_notes`](../mcp/reads.md).

## See also

- [Searching your vault](../../tutorials/search.md).
- [`reindex`](reindex.md) — rebuild the index if results look stale.
