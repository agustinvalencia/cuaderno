# `cdno lint`

Validate every indexed note and report frontmatter problems. Errors fail the command; warnings (such
as broken wikilinks) are non-fatal unless `--strict` is given.

A wikilink or embed that points at an **attachment** — a pasted image, a filed PDF — is not a broken
link: attachments are never indexed, but the target is resolved against the filesystem (relative to
the linking note, then to the vault root) before a link is called broken. Only a target that matches
nothing at all is reported, and a missing `![[embed]]` reads as a missing file rather than a link
that "resolves to no note".

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
