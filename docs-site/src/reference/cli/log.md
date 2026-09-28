# `cdno log`

Append a log entry to today's daily note (creating the note if it doesn't exist yet).

```text
cdno log [OPTIONS] <MESSAGE>
cdno log note [OPTIONS]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<MESSAGE>` | The log message. Quote it if it contains spaces. |

## Options

| Flag | Description |
|------|-------------|
| `--at <TIMESTAMP>` | Override the timestamp. Accepts `YYYY-MM-DDTHH:MM:SS` or `YYYY-MM-DDTHH:MM`. Defaults to now. |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result.

## Examples

```bash
cdno log "scaled the mesh to 2M cells; 4x runtime, still stable"

# Backdate an entry:
cdno log "forgot to record: fixed the sampler seed" --at 2026-04-24T18:30

# Scripted:
cdno log "nightly run complete" --json
# -> { "path": "journal/2026/daily/2026-04-25.md", "message": "Logged to ..." }
```

Daily notes are [append-only](../../concepts/business-rules.md) — `log` only ever adds.

A first argument that names a subcommand (`note`) selects it. To log that bare word as a message,
pass it after `--`: `cdno log -- note`.

## `cdno log note`

Write worked-out substance (a derivation, a procedure, a page of reasoning) to a daily note as one
`### <heading>` entry under `## Notes`. The pointer line
`noted [[journal/<year>/daily/<date>#<heading>]]` is written to `## Logs` for you in the same
write, followed by the body's wikilinks in parentheses when it has any. Keep `## Logs` for
one-line events and put the substance here.

```text
cdno log note [--heading <STRING>] [--body-file <PATH>] [--date <YYYY-MM-DD>]
```

| Flag | Description |
|------|-------------|
| `--heading <STRING>` | The entry's heading, written as `### <heading>` and used as the pointer's anchor. Prompted in an interactive session. |
| `--body-file <PATH>` | A file holding the entry's body. Leading blank lines and trailing whitespace are dropped; the first line's indentation is kept, so an entry may open with an indented code block. A blank body is refused. Opened in your editor in an interactive session. |
| `--date <YYYY-MM-DD>` | The day whose note receives the entry, stamped at the current time. Defaults to today. |

Without `--heading` or `--body-file`, an interactive session prompts for the heading and then
opens the body in your editor, and confirms before writing; under `--no-interactive` (or `--json`)
each is a missing-flag error.

The heading must be unique within the day, must not reuse a daily section name (`Standup`,
`Intention`, `Agenda`, `Meeting`, `Notes`, `Logs`), and must not contain `[`, `]`, `|`, `#` or
inline markup (bold, italics, code) nor start with `^`. Headings inside the body must be level 3
or deeper and are held to the same uniqueness rule. A refused entry writes nothing.

End an entry that could be reused beyond today with the tag `#concept` on the body's last line,
so the review can find it as a candidate for promotion to a concept note.

The command prints `Noted <target>`, where the target is the entry's anchored link
(`journal/…#<heading>`). With `--json` it emits `path`, `message`, `target` and `log_line`:

```bash
cdno log note --heading "Woodbury identity" --body-file woodbury.md --json
# -> { "path": "journal/2026/daily/2026-04-25.md",
#      "message": "Noted journal/2026/daily/2026-04-25#Woodbury identity",
#      "target": "journal/2026/daily/2026-04-25#Woodbury identity",
#      "log_line": "noted [[journal/2026/daily/2026-04-25#Woodbury identity]] ([[projects/surrogate-model]])" }
```

Cite the entry elsewhere as `[[<target>]]`.

## Related MCP tool

[`append_to_log`](../mcp/writes.md) — the same operation for AI clients; `note_to_daily` is the
counterpart of `cdno log note`.

## See also

- [The daily loop](../../tutorials/daily-loop.md).
