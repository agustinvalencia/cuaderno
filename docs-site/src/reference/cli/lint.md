# `cdno lint`

Validate every indexed note and report what is wrong with it — frontmatter, links, attachment
pairing, and lines the canonical parsers silently skip. Errors fail the command; warnings (such as
broken wikilinks) are non-fatal unless `--strict` is given.

A wikilink or embed that points at an **attachment** — a pasted image, a filed PDF — is not a broken
link: attachments are never indexed, but the target is resolved against the filesystem (relative to
the linking note, then to the vault root) before a link is called broken. Only a target that matches
nothing at all is reported, and a missing `![[embed]]` reads as a missing file rather than a link
that "resolves to no note".

A link that resolves only through a note's `aliases:` entry — a former name — is a straggler: it still
works, but lint warns and names the note's current slug (`[[old]]` names a renamed note — now `new`),
so you can fix the link by hand, or rewrite them all with
[`cdno note rename <note> --relink`](note.md#relinking-the-stragglers---relink). Lint also warns when an alias collides with another note's name. If it equals another note's file
stem, the stem wins and the alias can never be followed (alias `x` on `<path>` is shadowed by
`<other>`). If it equals the folder name of a folder note such as `portfolios/x/_index.md`, the alias
takes the bare link `[[x]]` (alias `x` on `<path>` takes bare `[[x]]` from folder note `<other>`);
the folder form `[[portfolios/x]]` still reaches the folder note. A straggler to a folder note is
told to use the folder form, which names the note unambiguously.

A bare link to a folder note's own name resolves: `[[surrogate-model]]` reaches
`portfolios/surrogate-model/_index.md`, and an expanded stewardship answers to its folder name the
same way, so lint does not report it. It is the last rule tried: a note with that file name keeps
the bare link (several such notes leave it unresolved), so does the one note holding that alias, and
an alias two notes share leaves it unresolved. Two folder notes with one name leave it unresolved
too. A folder note at the vault root is an exact path match for its name, so none of this applies
to it. A portfolio that lists its own folder name in `aliases:` is followed through the alias, and
a bare link to it is reported as a renamed-note link. The folder form
`[[portfolios/surrogate-model]]` is the one that is always right.

It also reports lines that were plainly meant to be structured but will never be read as such,
because the parsers that consume them skip what they cannot parse rather than complaining. That
covers malformed `## Active Habits` and `## Periodic Commitments` bullets on a stewardship
dashboard, and — in a daily note's `## Logs` — a start or close marker that
[`cdno now`](now.md) will not see:

```text
[warning] journal/2026/daily/2026-09-15.md: log line `- **09:30**: started [[alpha]] - Draft methods (deep)` reads as a `started` marker but `cdno now` will not see it -- found an ASCII hyphen (-) where an em-dash (—) separates the slug from the action
```

The shape has to be `- **HH:MM**: started [[slug]] — text`, and every part of it matters: the `- `
bullet and the stamp are what make the line a log entry at all, and the separator must be a real em
dash (U+2014). A stamp that was attempted and mangled — `- 09:20:` unbolded, `- **25:99**:` out of
range, `- **09:40**` with no colon — is caught too, not only one that is missing outright. The check is
deliberately narrow — the marker has to open the entry and be followed immediately by `[[` — so
ordinary prose in `## Logs`, including a sentence that merely mentions starting something or names a
note mid-sentence, is never flagged. The `renamed [[new]] — Title` line a rename writes, and the
`relinked [[new]] — Title` line of `--relink`, get the same near-miss check (neither is a focus
marker, so `cdno now` never reads them), and a `was:` continuation
under it that contains a `[[` link is a warning, because the value is a copied slug and a link would
be rewritten by the next rename.

It checks that an **archived action note** (`actions/_done/<year>/`) has only grown since it was
archived. The archival recorded the note's frozen prefix; lines appended after it (a late
retrospective) are fine, while an edit to an existing line, or a cut into the prefix, is an error:

```text
[error] actions/_done/2026/characterise.md: archived action note modified an existing line (append-only after completion)
```

A rename may change the prefix in one way only: it re-targets references, rewriting the address of a
wikilink (`[[old]]` becomes `[[surrogate-model]]`) and a reference field such as `project:`. So the
prefix is accepted when either its bytes are unchanged or they match with every wikilink address and
reference-field value set aside. The cost of that rule is that a hand edit which only re-targets a
reference inside a frozen prefix is no longer detected; any other change still is. This holds on every
clone that opened the vault once on this version with the backfill completing (it is retried at every open, and a lock timeout only postpones it) before the renamed file arrived: a note archived on an
older version and renamed elsewhere first is reported as an error that names
[`cdno reindex`](reindex.md), which clears it by rebuilding the index without any archival snapshots,
so every archived note loses this check. On a renamed note, a hand addition that follows an unmatched
backtick at the end of the prefix with no blank line in between is reported too; start a late addition
after a blank line. A note archived by
a newer cdno, whose snapshot this cdno cannot read in the second form, gets a warning naming the
version, and only the unchanged-bytes form is checked for it.

It checks each project map's `status` against the folder the map sits in and against its `closed:`
date. A status its folder contradicts is an error, since the project verbs find a map by its folder
and act on it by its status; the message says which way to repair it:

```text
[error] projects/_parked/alpha.md: project says `status: active` but is not at `projects/<slug>.md`; move it there, or set the status its folder stands for
```

A `completed` or `dropped` map outside `projects/_done/<year>/` is told to move there and set
`closed: <date>`, or to set `status: active` and close it with
[`cdno project complete`](project.md). A `closed:` date that disagrees with the status is a
warning: a completed or dropped map without one cannot be placed by the reviews, and an active or
parked map carrying one should have `closed: null`.

It reads each open map's `held_by:` (the name of whoever else is moving it) against the vault's
`[projects] holder_sentinels` list (see [configuration](../configuration.md)). A value that is not a
plain name, such as an unquoted wikilink, is an error, because the holder you wrote is not honoured
and the map still counts toward the cap (or would, once a parked map is activated). For an unquoted
wikilink the message says to quote it; for anything else (a number, a date, a list) it says to write a
single quoted name:

```text
[error] projects/harbour.md: project `held_by:` is not a plain name, so the holder is not honoured and the map counts toward the cap; quote it: `held_by: "[[people/ana]]"`
```

A value on the list, or a blank one, names you or nobody, so the map is yours and counts. That is a
warning:

```text
[warning] projects/harbour.md: project `held_by: "tbd"` does not name someone else: delete the line, or name the holder; a holder that names you or nobody means the map is yours
```

And it warns, once, on `.cuaderno/config.toml` when the projects that count (active and naming no
other holder) exceed `max_active_projects`, listing them. The project verbs refuse to go over the
cap, so this catches a cap lowered in config or maps edited by hand:

```text
[warning] .cuaderno/config.toml: 6 projects count toward the cap of 5 (`max_active_projects`): [[alpha]], [[bravo]], [[charlie]], [[delta]], [[echo]], [[foxtrot]]; park or close one until no more than 5 count
```

If a project map cannot be read at all (an unknown `status`, a missing `context` or `created`), the
count is unknown, and the same file gets an error saying so: `could not count the projects against
the cap: …`. The project verbs fail on such a map too, so fix it first.

It also warns when a customised template (`.cuaderno/templates/<type>.md`) lacks a frontmatter key that
the built-in template has, typically one a release added after you customised it, and reports a
customised template it cannot read (for instance one that is not UTF-8) as an error:

```text
[warning] .cuaderno/templates/project.md: custom template `project` lacks `closed` (run `cdno templates sync project`)
```

[`cdno templates sync`](templates.md#cdno-templates-sync-type) adds the key without touching your
edits. Lint cannot tell a new key from one you removed on purpose, so a built-in key you deleted from
your template is reported too.

It reads `[vault] method_version` (see [configuration](../configuration.md#method-version)) and
reports, on `.cuaderno/config.toml`, a vault this cdno would not write to as an error: one on an
**older major** version (the message points at `cdno migrate --dry-run`, then `cdno migrate`) or a
**newer major** (upgrade cdno). A vault on a **newer minor** version is a warning, since writes
proceed:

```text
[warning] .cuaderno/config.toml: this vault follows RLM 1.4, a newer minor version than the RLM 1.3 this cdno implements — writes proceed; upgrade cdno for the practices it adds
```

A vault on this cdno's version, or on an older minor one (an unstamped vault included), gets no
row here. A malformed `method_version` (`"v1"`, `"1.1.0"`) is not a lint row: it is a
configuration error, so the vault does not open and every command, `lint` included, fails with an
error naming the key. The one exception is a long-running server (`cdno-mcp-server`) whose config
was edited to a malformed value after it opened: it keeps running, and there `lint` (and the MCP
`lint` tool) reports one error row on `.cuaderno/config.toml` naming the key instead of aborting.
Fix the value (see [configuration](../configuration.md#method-version)).

While the vault records a method version below RLM 1.1 (an unstamped vault reads as 1.0), it gives
one warning per **overdue** hard date on a parked project map, on every run, since the commitments
register keeps those dates as of RLM 1.1: the one-time read-through of parked deadlines. The row sits on the map for a hard
milestone and on the action note for a `due:`, and names the fix:

```text
[warning] projects/_parked/beta.md: hard milestone `Grant report` (2026-04-15) on parked project [[beta]] is overdue and in the commitments register again (a parked map's hard dates are promises under RLM 1.1): drop it with `cdno project milestone drop`, or re-date it on the map; this read-through row disappears once the vault records RLM 1.1, which `cdno migrate` records
```

An action note's row says to re-date its `due:` in the note, or drop the action with
[`cdno action drop`](action.md#cdno-action-drop). Future dates are not warned on;
[`cdno migrate --dry-run`](migrate.md) lists them all. Once the vault records RLM 1.1 the rows are
gone for good.

## Typst notes

A note written in Typst (`.typ`, a `#metadata((…)) <cdno>` block first in the file) is linted
like any other note, plus these rows. Errors fail the command; warnings do not, unless `--strict`.

| Row | Level | When |
|---|---|---|
| metadata block missing a required key | error | `type` is absent, or `title` on a type that carries a title heading (`concept`, or a custom type with a `title_field`) |
| metadata block not literal | error | a value is anything but a string, an array of strings, a boolean or a number |
| metadata block not first or not top-level | error | something other than whitespace or comments precedes the block, it is nested, or there is a second block |
| `<cdno>` is not the label of a `#metadata((…))` call | error | the label sits on something else, or only inside a comment or a string |
| a second `<cdno>` label | error | a stray label after a valid block |
| metadata present but the `<cdno>` label is missing, mistyped or nested | error | a `#metadata((type: …))` call with no `<cdno>` directly after it; the file is not a note until it has one |
| link interior not one inline run | error | an unescaped markup character in `[[…]]`; the row gives the line and the remedy |
| Typst note does not parse | warning | the syntax tree holds an error, typically after a hand edit |
| stem shared between `.md` and `.typ` | error | both exist: `[[x]]` can name only one. An editor that resolves `[[x]]` only to `x.md` may have created the empty file |
| Typst note of a type that does not admit Typst | error | the block's `type` is not listed under `[typst] note_types`; the file is not indexed |
| Typst file is neither a note nor an attachment | warning | a loose `.typ` in a folder that admits Typst, with no label and no owning note |
| labelled Typst file outside the configured types | warning | a labelled `.typ` in the folder of a type that `[typst] note_types` leaves out; it is not indexed |
| Markdown note in a Typst note's assets folder | warning | `<stem>/` beside `<stem>.typ` holds a `.md` with frontmatter, which would be a note anywhere else; it is not indexed |

```text
[warning] portfolios/surrogate-error-budget/template.typ: Typst file is neither a note nor an attachment: it has no `<cdno>` block and no note owns its folder; add a block to make it a note, or move it into the assets folder of the note that uses it
```

A dangling `[[link]]` in a Typst body is reported as in a Markdown note.

```text
cdno lint [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--strict` | Treat warnings as failures too (exit non-zero on any issue). |

Plus the [global options](overview.md#global-options). `lint` ignores `--json`.

## Exit status

- Clean, or warnings only without `--strict` → exit `0`.
- Any error (e.g. unknown note type, invalid frontmatter) → non-zero.
- With `--strict`, any warning also → non-zero. Useful in CI to keep a vault pristine.

## Examples

```bash
cdno lint                 # report issues; fail only on errors
cdno lint --strict        # fail on warnings too (e.g. broken links)
```

## Related MCP tool

[`lint`](../mcp/reads.md).

## See also

- [`reindex`](reindex.md), [`normalise`](normalise.md) — the other maintenance verbs.
