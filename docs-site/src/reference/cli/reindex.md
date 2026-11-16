# `cdno reindex`

Rebuild the SQLite index from scratch off the Markdown source of truth. The recovery path for a
corrupt or stale index.

```text
cdno reindex [OPTIONS]
```

## Options

| Flag | Effect |
|------|--------|
| `--rebind` | With `CUADERNO_INDEX_PATH` set: rebuild the file even when it belongs to another vault, giving it to this one and discarding the other vault's index. Stop whatever serves the other vault first. Without it, such a file is refused. |
| `--drop-snapshots` | Leave the archival snapshots behind instead of carrying them into the rebuilt index (see [Archival snapshots](#archival-snapshots)). |

Plus the [global options](overview.md#global-options). `reindex` ignores `--json`.

## When you need it

Almost never — Cuaderno reconciles the index automatically on every run of any command but
[`now`](now.md), and shell completion does not open it (see
[Business rules](../../concepts/business-rules.md#startup-reconciliation)). Reach for `reindex` when:

- search or link results look wrong after a large external edit or a sync conflict, or
- you deleted `.cuaderno/index.db` and want to rebuild it eagerly rather than on next use.

## What it does

`reindex` builds a fresh index from the notes beside the live one, then copies it over the live
file **in place**, holding the vault write lock while it does. The file is never deleted or
replaced, so a process that already has it open (an MCP server, [`cdno watch`](watch.md)) reads the
rebuilt index on its next query, with no restart, and no write by another command lands half
before and half after. While another command holds the vault write lock, `reindex` waits for it
(up to five seconds), and then fails with the usual lock error, leaving the index as it was.

If the index failed its integrity check, `reindex` says so, and keeps a copy of the damaged file
beside it with `.previous` appended to its name (`.cuaderno/index.db.previous` by default).

A file too damaged to rebuild in place (one SQLite refuses to open, or to write over) is the one
case that still needs a new file. `reindex` moves it aside to `.previous`, under the vault write
lock, builds a new index from the notes, and tells you to **restart any process that had the index
open**: until it restarts, such a process keeps using the old file. The archival snapshots in the
damaged file are lost with it.

When `CUADERNO_INDEX_PATH` is set, `reindex` rebuilds the file it names, leaves `.cuaderno/`
alone, and prints the file's path. It refuses, changing nothing, when that file is not one cuaderno
created as its index (another application's SQLite database included). An overridden index belongs
to one vault: a plain `reindex` in another vault is refused, and `reindex --rebind` is how you give
the file to it, after stopping whatever serves the vault it belonged to; it says when it rebinds
(see
[Keeping the index on each machine](../../concepts/vault-structure.md#keeping-the-index-on-each-machine)).
A rebind is not done in place: the other vault's file is deleted and a new one built for this
vault, so none of the other vault's rows, its archival snapshots included, come across, and a
process still serving the other vault from that file keeps its own copy instead of starting to read
this vault's notes.

## Archival snapshots

Because the Markdown files are authoritative, a rebuild loses no note. The one thing in the index
the Markdown cannot rebuild is the archival snapshots, the record [`cdno lint`](lint.md) checks each
archived action note's frozen prefix against. `reindex` carries them into the rebuilt index, so the
check survives a rebuild. A snapshot whose note is no longer indexed (deleted, or not parsing) is
not carried, and `reindex` names it in a warning.

`--drop-snapshots` rebuilds without them. Use it only for a snapshot that can never verify again,
such as the one [`cdno lint`](lint.md) reports as predating the masked hash: every archived action
note then loses its frozen-prefix check, since nothing re-records a snapshot for a note already
archived.

## Examples

```bash
cdno reindex
cdno reindex --drop-snapshots   # also forget the archival snapshots
```

## See also

- [`lint`](lint.md), [`normalise`](normalise.md).
- [Business rules](../../concepts/business-rules.md).
