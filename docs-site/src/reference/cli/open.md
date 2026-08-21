# `cdno open`

Resolve a note reference and print the note's absolute path. Where
[`cdno search`](search.md) answers *"where did I write about this"*, `open` answers *"take me to
the note I mean"* — it is addressing, not searching.

```text
cdno open [OPTIONS] [REFERENCE]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[REFERENCE]` | The note to resolve. Omit it and pass `--list` to see every note. Tab-completion offers your vault's slugs. |

A reference can be any of:

| Form | Example | Resolves to |
|------|---------|-------------|
| Bare slug | `surrogate-model` | the note with that slug, whatever its type |
| Type-scoped slug | `project:surrogate-model` | that slug within one note type |
| Calendar word | `today`, `yesterday`, `tomorrow` | the daily note for that day |
| Date | `2026-08-21` | that day's daily note |
| ISO week | `2026-W34` | that week's weekly note |
| Month | `2026-08` | that month's monthly note |
| Vault-relative path | `journal/2026/daily/2026-08-21.md` | that file |
| Absolute path inside the vault | `/home/you/vault/projects/foo.md` | that file |

Portfolios and expanded stewardships are addressed by their **folder** name, not the literal
`_index`: `cdno open surrogate-model` reaches `portfolios/surrogate-model/_index.md`.

The calendar words always mean the journal. A note genuinely named `today` stays reachable as
`<type>:today`.

## Options

| Flag | Description |
|------|-------------|
| `--list` | Print every note as `path<TAB>title<TAB>type`, for piping to a fuzzy finder. Resolves nothing and takes no reference. |

Plus the [global options](overview.md#global-options). With `--json`, a resolved reference emits
`{"path": …}` and `--list` emits an array of `{path, title, type}`.

## When a reference does not resolve

Two cases, and they say different things:

- **Nothing matched.** The error names the closest few notes by their type-scoped form — so a
  mistyped slug tells you what you probably meant rather than dumping the vault.
- **The slug matched more than one note.** `cdno open` never guesses, because opening the wrong
  note is a mistake you only discover after typing into it. The error names each type-scoped form
  instead, and any of them resolves. This happens when a stewardship and a portfolio share a name,
  for instance — `stewardships/gym.md` and `portfolios/gym/_index.md` are both `gym`.

A reference that looks like a **path** never falls back to fuzzy matching: a typo there means "no
such file", not a near-miss opened on your behalf.

## Examples

```bash
cdno open today                       # today's daily note
cdno open surrogate-model             # by slug
cdno open project:surrogate-model     # when one slug is used by two types
cdno open 2026-W34                    # that week's weekly note
cdno open --list | head               # every note, tab-separated
cdno open today --json | jq -r .path
```

### With a fuzzy finder

Because `--list` emits vault-relative paths and `open` accepts absolute ones, the round-trip works
from any directory — no need to `cd` into the vault first:

```bash
cdno open "$(cdno open --list | fzf --with-nth=2.. --delimiter='\t' | cut -f1)"
```

Worth a shell function:

```bash
cdo() {
  local pick
  pick=$(cdno open --list | fzf --with-nth=2.. --delimiter='\t' | cut -f1) || return
  [ -n "$pick" ] && cdno open "$pick"
}
```

This beats running `fzf` over the vault directory directly, because the candidates carry each
note's **title** rather than only its filename — filenames here are slugs, so a note titled
"Surrogate model" would otherwise never match the words you remember. The listing also respects
the vault's [`ignore`](../configuration.md) globs.

## See also

- [`search`](search.md) — full-text search when you do not know which note you want.
- [Searching your vault](../../tutorials/search.md).
