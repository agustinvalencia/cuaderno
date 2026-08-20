# Colour and interactivity

`cdno`'s human-readable output is coloured, and several read commands offer to open one of the rows
they just printed. Both behaviours are for people at a terminal, and both switch themselves off the
moment the output is going somewhere else.

## Cards

Listings whose items carry prose — `project list`, `portfolio list`, `stewardship list`, `questions`,
`search`, and the active-projects section of `orient` — render as cards: a coloured bar down the left
of every line an item owns, the identifier as a title, a badge aligned into a shared column, and the
text wrapped underneath.

```text
3 active projects

▎ surrogate    side-project
▎ Six contributors settled; scope is fixed to the solver rather than the
▎ mesher, and the validation plan is agreed.
▎ next: Draft the validation plan

▎ mesh         work
▎ Coarse-mesh run validated end to end; two workstreams in flight.
▎ next: Profile the assembly step
```

What the bar's colour means depends on the listing, and it is always the thing that listing exists to
surface: **context** for `project list` and `orient`, **staleness** for `portfolio list` and
`stewardship list` (amber once nothing has been filed for a month), and **domain** for `questions`.
`search` colours every hit alike — relevance is already the ordering.

Commands whose rows are genuinely tabular keep their tables — `status`, `commitments`, `orient`'s
commitments section, and a portfolio's evidence list all have short, aligned fields where a column
beats a card. `show` commands keep their plain line shape too: a bar earns its space by marking where
one item ends and the next begins, and a detail view has only one item.

`note list` is untouched. It prints one bare path per line because it exists to be piped into other
tools, and a header or a gutter would break that.

## Colour

| Setting | Effect |
|---|---|
| `--color auto` | **Default.** Colour only when stdout is a terminal. |
| `--color always` | Colour even when redirected — for `cdno project list --color always \| less -R`. |
| `--color never` | Never colour. |

`--colour` is accepted as a spelling of the same flag.

Under `auto`, three environment variables are consulted, in this order:

1. **`NO_COLOR`** (set and non-empty) turns colour off.
2. **`CLICOLOR=0`** turns colour off.
3. **`CLICOLOR_FORCE`** (set and non-empty) turns colour on even when redirected.

`NO_COLOR` deliberately outranks `CLICOLOR_FORCE`: `NO_COLOR` is something you export once as a
preference about your own terminal, while `CLICOLOR_FORCE` is usually set by a harness that has no
standing to override it. An explicit `--color` outranks all three.

**`--json` output is never coloured**, whatever any of the above says. Scripts can pass `--color always`
safely.

## Interactive reports

In a terminal, `project list`, `portfolio list`, `stewardship list`, `orient`, and `status` follow
their output with a picker:

```text
? Inspect a project
❯ surrogate (side-project)
  mesh (work)
  garden (family)
[↑↓ to move, enter to inspect, Esc to leave]
```

Choosing a row prints exactly what the matching `show` command would print, then asks again. **Esc or
Ctrl-C leaves**, with exit status 0.

The listing is always printed first, so the prompt only ever adds to what you would have seen. It is
skipped entirely when:

- **either** stdin or stdout is not a terminal — piped, redirected, `< /dev/null`, a background job,
  or running under CI, which is the usual reason neither end is a terminal,
- `--no-interactive` is passed, or
- `--json` is passed.

Both streams matter: the listing is written to stdout but the picker reads stdin, so a caller with a
terminal on only one end is not offered the prompt. There is no separate CI detection — a CI job has
no terminal, and that is what actually decides it.

That makes the same command safe for a person, a shell pipeline, and an AI agent without changing
anything about how it is invoked.

## Width

On a terminal, output is laid out to the terminal's width as measured when the command runs. `cdno`
prints once and exits, so resizing afterwards does not reflow anything already on screen — run the
command again. Everywhere else — piped, redirected, or when the terminal reports no usable size (a
pty opened without one reports zero columns) — it lays out to a fixed 100 columns, so captured output
is deterministic and diffable.

Text from your notes is sanitised before it is laid out: tabs become spaces, and carriage returns and
escape sequences are replaced. A note is data, and without this a stray escape in a note could repaint
the terminal, move the cursor, or draw over the card's own gutter.
