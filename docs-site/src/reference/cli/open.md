# `cdno open`

Open a note in your editor. Where [`cdno search`](search.md) answers *"where did I write about
this"*, `open` answers *"take me to the note I mean"* — it is addressing, not searching.

```text
cdno open [OPTIONS] [REFERENCE]
```

Run it with no reference and it offers a picker over every note, most-recently-edited first.

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
| `--path` | Print the note's absolute path instead of opening it. |
| `--list` | Print every note as `path<TAB>title<TAB>type`, for piping to a fuzzy finder. Opens nothing and takes no reference. |
| `--editor <COMMAND>` | Editor command template for this invocation. Outranks `$CUADERNO_EDITOR`, `$VISUAL`, and `$EDITOR`. |

Plus the [global options](overview.md#global-options). With `--json`, a resolved reference emits
`{"path": …}` and `--list` emits an array of `{path, title, type}`.

**When stdout is not a terminal, `cdno open` prints the path instead of launching anything.** So
`cdno open today | cat`, a script, `--no-interactive`, and `--json` all behave the same way, and no
editor is ever started into a pipe.

## Choosing the editor

First one set wins:

| | |
|---|---|
| `--editor <COMMAND>` | this invocation only |
| `$CUADERNO_EDITOR` | your shell |
| `$VISUAL`, then `$EDITOR` | your shell |
| *(nothing set)* | the operating system's default handler for `.md` |

```bash
export CUADERNO_EDITOR='code -g {path}'
```

`{path}` marks where the note's path goes. Leave it out and the path is appended, so a bare
`nvim` works. Quoting is honoured, so a program name may contain spaces:

```bash
export CUADERNO_EDITOR='"/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl" -w {path}'
```

A value whose first word contains `://` is handed to the operating system instead of executed,
with the path percent-encoded — `obsidian://open?path={path}`.

`cdno` waits for the editor to exit. It does not try to guess whether your editor is a terminal
or a GUI one — `code -w` blocks and `code` does not, and a guess would be wrong in a way you
could not override. Waiting is correct for every terminal editor and harmless for a GUI one that
returns immediately. If the editor exits non-zero, `cdno open` exits with the same code, the way
`git commit` treats an abandoned edit.

### There is no per-vault editor setting, on purpose

You might expect `.cuaderno/config.toml` to carry this — a research vault that opens in Obsidian,
a code vault that opens in your editor. It deliberately does not.

A vault is a git repository, and `--vault` exists so cdno can be pointed at one you did not
create. A setting that names *a program to run* cannot live in data that gets cloned and synced:
opening a note in someone else's vault would run their choice of program on your machine. It is
the same reason git does not honour every setting from a repository you just cloned.

Restricting such a setting to a bare binary name would not help. `sh` is a binary name, and
`sh <the note>` executes the note's own contents — which, in a vault you cloned, the author also
wrote. The fix is that the setting comes from your shell rather than from the data.

If you work across vaults that want different editors, set `CUADERNO_EDITOR` in a per-directory
shell hook (`direnv`, or your shell's `chpwd`), which keeps the decision on your machine.

## Archived notes

Opening a note under `actions/_done/` prints a warning first: its text was frozen when it was
archived, and [`cdno lint`](lint.md) reports an edit to the existing text as an error. Appending
to it is fine. The warning does not stop you — markdown remains the source of truth.

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
cdno open                             # pick from every note
cdno open today                       # today's daily note
cdno open surrogate-model             # by slug
cdno open project:surrogate-model     # when one slug is used by two types
cdno open 2026-W34                    # that week's weekly note
cdno open --path today                # print the path, open nothing
cdno open --list | head               # every note, tab-separated
cdno open today --json | jq -r .path
cdno open notes --editor 'code -g {path}'
```

### With a fuzzy finder

`cdno open` has its own picker, so you do not need `fzf` at all. If you would rather use yours —
your keybindings, your preview window — `--list` gives it candidates. Because `--list` emits
vault-relative paths and `open` accepts absolute ones, the round-trip works from any directory,
with no need to `cd` into the vault first:

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
