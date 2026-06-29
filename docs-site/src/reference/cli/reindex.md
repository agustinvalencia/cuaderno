# `cdno reindex`

Rebuild the SQLite index from scratch off the Markdown source of truth. The recovery path for a
corrupt or stale index.

```text
cdno reindex [OPTIONS]
```

## Options

Only the [global options](overview.md#global-options). `reindex` ignores `--json`.

## When you need it

Almost never — Cuaderno reconciles the index automatically on every run (see
[Business rules](../../concepts/business-rules.md#startup-reconciliation)). Reach for `reindex` when:

- search or link results look wrong after a large external edit or a sync conflict, or
- you deleted `.cuaderno/index.db` and want to rebuild it eagerly rather than on next use.

Because the Markdown files are authoritative, a full rebuild is always safe.

## Examples

```bash
cdno reindex
```

## See also

- [`lint`](lint.md), [`normalise`](normalise.md).
- [Business rules](../../concepts/business-rules.md).
