# Searching your vault

Cuaderno keeps a full-text index of every note, so you can find anything fast — ranked best-first,
with filters by type, date, and portfolio. The command is [`cdno search`](../reference/cli/search.md).

## Basic search

```bash
cdno search "preconditioner"
cdno search "sparse attention"     # multiple words are ANDed
```

The query is matched case-insensitively and terms are combined with AND. Quotes and operators are
treated as literal words, not search syntax. Results come back ranked, best match first.

## Filter the results

```bash
# Only one note type:
cdno search "ablation" --type evidence

# Within a date window (inclusive):
cdno search "mesh" --from 2026-03-01 --to 2026-03-31

# Only inside one portfolio:
cdno search "speedup" --portfolio sparse-vs-dense-attention-ood

# Cap the number of hits (default 20):
cdno search "todo" --limit 5
```

Filters combine, so you can scope tightly:

```bash
cdno search "preconditioner" --type evidence --portfolio sparse-vs-dense-attention-ood --from 2026-01-01
```

## Scripting with `--json`

`--json` returns the ranked hits as a JSON array — each with `path`, `note_type`, `title`, `snippet`,
and `score` — ready for `jq` or another tool:

```bash
cdno search "speedup" --json | jq -r '.[].path'
```

See [JSON output](../reference/json-output.md) for the exact shape.

## When search feels stale

Search reads the SQLite index, which is reconciled automatically on every run. If results ever look
out of date (e.g. after a bulk external edit), rebuild it explicitly:

```bash
cdno reindex
```

The Markdown files are always the source of truth; the index is just a rebuildable cache (see
[Business rules](../concepts/business-rules.md)).

That completes the tutorials. For exhaustive detail on any command, see the
[CLI reference](../reference/cli/overview.md).
