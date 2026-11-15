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

When `CUADERNO_INDEX_PATH` is set, `reindex` rebuilds the file it names, leaves `.cuaderno/`
alone, and prints the file's path. It refuses, deleting nothing, when that file is not an index. An
overridden index belongs to one vault; `reindex` is how you give it to another, and it says so when
it does (see
[Keeping the index on each machine](../../concepts/vault-structure.md#keeping-the-index-on-each-machine)).

Because the Markdown files are authoritative, a full rebuild loses no note. It does drop one thing
the Markdown cannot rebuild: the archival snapshots, so every archived action note loses the
frozen-prefix check [`cdno lint`](lint.md) runs on it.

## Examples

```bash
cdno reindex
```

## See also

- [`lint`](lint.md), [`normalise`](normalise.md).
- [Business rules](../../concepts/business-rules.md).
