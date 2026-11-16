# `cdno reindex`

Rebuild the SQLite index from scratch off the Markdown source of truth. The recovery path for a
corrupt or stale index.

```text
cdno reindex [OPTIONS]
```

## Options

| Flag | Effect |
|------|--------|
| `--rebind` | With `CUADERNO_INDEX_PATH` set: rebuild the file even when it is bound to another vault path, giving it to this vault and discarding the index recorded for that path, archival snapshots included. Stop whatever serves the other vault first. Without it, such a file is refused. See [Rebinding](#rebinding). |
| `--drop-snapshots` | Leave the archival snapshots behind instead of carrying them into the rebuilt index (see [Archival snapshots](#archival-snapshots)). |

Plus the [global options](overview.md#global-options). `reindex` ignores `--json`.

## When you need it

Almost never — Cuaderno reconciles the index automatically on every run of any command but
[`now`](now.md), and shell completion does not open it (see
[Business rules](../../concepts/business-rules.md#startup-reconciliation)). Reach for `reindex` when:

- search or link results look wrong after a large external edit or a sync conflict, or
- you deleted `.cuaderno/index.db` and want to rebuild it eagerly rather than on next use.

Reach for `reindex --drop-snapshots` when [`cdno lint`](lint.md) names it: an archived action
note whose snapshot predates the masked hash and was renamed on another clone can never verify
again. A plain `reindex` keeps the snapshots, so it no longer clears that error, nor the error on
an archived note you edited inside its frozen prefix on purpose; only `--drop-snapshots` does, at
the cost described under [Archival snapshots](#archival-snapshots).

## What it does

`reindex` builds a fresh index from the notes beside the live one, then copies it over the live
file **in place**, holding the vault write lock while it does. The file is never deleted or
replaced, so a process **on this machine** that already has it open (an MCP server,
[`cdno watch`](watch.md)) reads the rebuilt index on its next query, with no restart, and no write
by another command on this machine lands half before and half after. A process on another machine
or kernel sharing the file (a container bind-mounting the vault, a network share) is not covered:
SQLite's locks and the vault lock do not reach it, which is why each machine should have its own
index (see
[Keeping the index on each machine](../../concepts/vault-structure.md#keeping-the-index-on-each-machine)).
While another command holds the vault write lock, `reindex` waits for it (up to five seconds), and
then fails with the usual lock error, leaving the index as it was.

If the index failed its integrity check, `reindex` says so, and keeps a copy of the damaged file
beside it with `.previous` appended to its name (`.cuaderno/index.db.previous` by default). If the
check could not run at all, it says that instead, and keeps the copy too.

A file too damaged to rebuild in place (one SQLite refuses to open, or to write over) is the one
case that still needs a new file. `reindex` moves it aside to `.previous`, under the vault write
lock, and builds a new index from the notes. An MCP server that had the index open finds the file
moved on its next sweep and reopens the new one by itself; `reindex` tells you to **restart any
other process that had it open** (`cdno watch`): until it restarts, such a process keeps using the
old file. The archival snapshots in the damaged file are lost with it.

When `CUADERNO_INDEX_PATH` is set, `reindex` rebuilds the file it names, leaves `.cuaderno/`
alone, and prints the file's path. It refuses, changing nothing, when that file is not one cuaderno
created as its index (another application's SQLite database included).

## Rebinding

An overridden index is bound to one vault path (see
[Keeping the index on each machine](../../concepts/vault-structure.md#keeping-the-index-on-each-machine)):
a plain `reindex` from any other path is refused, and `reindex --rebind` is how you give the file
to the vault you run it in, after stopping whatever serves the vault it belonged to; it says when
it rebinds.

A rebind is not done in place: the file is deleted and a new one built for this vault, so none of
its rows, its archival snapshots included, come across, and a process still using the file for the
old path keeps its own copy instead of starting to read this vault's notes.

The binding is the vault's path, so **a vault folder you moved or renamed** looks like another
vault: every open is refused, and `--rebind` is the way out, but the snapshots it discards are then
that vault's own. The output names the old path either way, since the two cases look alike to it.

## Archival snapshots

Because the Markdown files are authoritative, a rebuild loses no note. The one thing in the index
the Markdown cannot rebuild is the archival snapshots, the record [`cdno lint`](lint.md) checks each
archived action note's frozen prefix against, and that [`cdno open`](open.md) reads to warn you
before it opens an archived note in your editor. `reindex` carries them into the rebuilt index, so
both survive a rebuild. A snapshot whose note is no longer indexed (deleted, or not parsing) is not
carried, and `reindex` names it in a warning.

`--drop-snapshots` rebuilds without them, and says how many it dropped (or that there were none).
Use it only for a snapshot that can never verify again, such as the one [`cdno lint`](lint.md)
reports as predating the masked hash: every archived action note then loses its frozen-prefix
check, and `cdno open` no longer warns before opening one, since nothing re-records a snapshot for
a note already archived.

## Examples

```bash
cdno reindex
cdno reindex --drop-snapshots   # also forget the archival snapshots
```

## See also

- [`lint`](lint.md), [`normalise`](normalise.md).
- [Business rules](../../concepts/business-rules.md).
