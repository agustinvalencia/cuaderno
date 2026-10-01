# `cdno lint`

Validate every indexed note and report what is wrong with it — frontmatter, links, attachment
pairing, and lines the canonical parsers silently skip. Errors fail the command; warnings (such as
broken wikilinks) are non-fatal unless `--strict` is given.

A wikilink or embed that points at an **attachment** — a pasted image, a filed PDF — is not a broken
link: attachments are never indexed, but the target is resolved against the filesystem (relative to
the linking note, then to the vault root) before a link is called broken. Only a target that matches
nothing at all is reported, and a missing `![[embed]]` reads as a missing file rather than a link
that "resolves to no note".

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
note mid-sentence, is never flagged.

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

It also warns when a customised template (`.cuaderno/templates/<type>.md`) lacks a frontmatter key that
the built-in template has, typically one a release added after you customised it, and reports a
customised template it cannot read (for instance one that is not UTF-8) as an error:

```text
[warning] .cuaderno/templates/project.md: custom template `project` lacks `closed` (run `cdno templates sync project`)
```

[`cdno templates sync`](templates.md#cdno-templates-sync-type) adds the key without touching your
edits. Lint cannot tell a new key from one you removed on purpose, so a built-in key you deleted from
your template is reported too.

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
