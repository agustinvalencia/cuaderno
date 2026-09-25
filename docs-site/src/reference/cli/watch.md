# `cdno watch`

Watch the vault and reconcile the index whenever a note changes outside `cdno`.

```text
cdno watch [OPTIONS]
```

Every other command reconciles once, at startup, and exits. That is enough when the process is
short-lived, but it means a note you edit in another editor is invisible to `search`, `lint` and
backlinks until the next `cdno` command happens to run. `watch` closes that gap: it stays in the
foreground and reconciles on each debounced change.

Markdown on disk remains the source of truth — this only keeps the cache honest.

## Options

Only the [global options](overview.md#global-options).

## Behaviour

- **Foreground until `Ctrl-C`.** There is no daemon mode; run it in a spare terminal or under your
  own process supervisor.
- **Debounced and coalesced.** An editor's atomic-save storm becomes one pass, and a bulk change
  (a `git checkout` across hundreds of notes) reconciles once rather than once per file.
- **Markdown only, outside `.cuaderno/`.** Reconciliation reads every directory in the vault, and
  the filesystem reports those reads — so reacting to them would make each pass trigger the next.
  Restricting to `.md` files outside `.cuaderno/` keeps the watcher from hearing itself.
- **A failed pass does not stop the watch.** The usual cause is a file caught half-written; the next
  event reconciles again.

Each pass prints what changed, so an edit that did not land is visible as `no index change`.

## Limitations

- **A `.cuaderno/config.toml` change needs a restart.** Honouring a new `ignore` glob means
  rebuilding the vault rather than reconciling it. `watch` says so at startup rather than quietly
  using stale globs.
- **Interrupting a pass is safe but leaves the index incomplete** until the next run — it is a cache,
  and the notes are untouched.

## Examples

```bash
cdno watch                       # in the vault
cdno watch --vault ~/notebook    # or point it at one
```

## See also

- [`reindex`](reindex.md) — force a full rebuild.
- [Business rules](../../concepts/business-rules.md#startup-reconciliation).
