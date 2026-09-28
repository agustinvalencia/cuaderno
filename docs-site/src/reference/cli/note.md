# `cdno note`

Create, list and revise notes of a [config-defined custom type](../custom-note-types.md) (declared under
`[note_types.<name>]` in `.cuaderno/config.toml`). Built-in types have their own verbs
(`cdno project create`, `cdno question create`, …); this is the generic surface for custom types.

## `cdno note create <type>`

Create a note of custom type `<type>`, written to `<folder>/<slug(title)>.md`.

```text
cdno note create [OPTIONS] <TYPE> --title <TITLE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<TYPE>` | A config-defined custom type (e.g. `person`). A built-in type is refused — use its own create command. |

### Options

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Required. The note's title; its slug becomes the filename. |
| `--field <NAME=VALUE>` | A frontmatter field, repeatable. Each key must be a declared `required`/`optional` field of the type; every `required` field must be supplied. |
| `--var <NAME=VALUE>` | A value for the type's template [prompted variable](../../tutorials/templates-and-frontmatter.md#prompted-variables), repeatable. |
| `--body-file <PATH>` | The note's body, read from a file, without the title heading (the engine writes the `# <title>` H1; a leading `# <title>` line in the file is dropped). Written verbatim apart from leading and trailing blank lines and trailing whitespace. It fills the template's `{{body}}` placeholder, or is inserted after the H1 when the template has none. When the template has `{{body}}` and nothing else fills it (`--field body=`, `--var body=` or a `[variables] body` in the config), it is opened in an editor if omitted interactively, and required under `--no-interactive`. |
| `--origin <STRING>` | Where the note came from: one string of wikilinks, e.g. `[[journal/2026/daily/2026-09-02#Woodbury identity]]`. Written, trimmed, as the `origin` frontmatter string even when the template omits or does not quote `{{origin}}`, so the type must declare `origin` (the `concept` type does); otherwise the command is refused. |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result. If the type ships no template (`.cuaderno/templates/<type>.md`), a minimal note is
synthesised from the declared fields plus a `# <title>` heading.

Creation is logged to today's daily note as `<type> created [[…]] — <title>`; do not log it again
by hand.

## `cdno note list <type>`

List every note of custom type `<type>`, by path.

```text
cdno note list <TYPE>
```

## `cdno note revise [note]`

Refine a mutable custom note (such as a concept) in place. The revision is logged to today's daily
note as `revised [[<path>]] — <reason>`, or `revised [[<path>#<Section>]] — <reason>` for a
section, in the same write; do not log it again by hand. Built-in note types (project, action,
daily, …) and custom types declared `append_only = true` are refused.

```text
cdno note revise [OPTIONS] [NOTE]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[NOTE]` | The note to revise, resolved as `cdno open` resolves a reference: a vault path with or without `.md`, a slug, or `type:slug`. An unknown or ambiguous reference is reported as `cdno open` reports it. Omitted in an interactive session, a picker offers every note. |

### Options

| Flag | Description |
|------|-------------|
| `--body-file <PATH>` | Replace the whole body (everything after the frontmatter, which is kept as it is) with the file's contents, written verbatim. Cannot be combined with `--section`. |
| `--section <STRING>` | The heading text of a section to upsert, without the `#` markers. Requires `--content-file`. An existing section is replaced together with its sub-sections; a missing one is appended as `## <section>`. |
| `--content-file <PATH>` | The section's new text, without its heading. Requires `--section`. |
| `--reason <STRING>` | Why the note was revised, in a short clause; it becomes the daily-log line. |

Plus the [global options](overview.md#global-options). With neither `--body-file` nor `--section`,
an interactive session opens the note's current body in your editor, then asks for the reason and
confirms before writing; under `--no-interactive` both are required. A note chosen in a picker
(an omitted, ambiguous or unmatched reference) also brings the confirm, even when every flag was
given. The note is read before
anything is gathered, and a note that changed on disk after that read (an edit saved while the
editor was open, say) is refused rather than overwritten: read it again and redo the revision.

Text identical to the note's current content writes and logs nothing and prints
`No change to <path>`. That includes closing the editor without changing anything: the reason and
confirm are still asked for, and the result is `No change to <path>`. With `--json`, emits `{path, message, changed, new_hash, log_line,
section_target}`.

## Examples

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note list person
cdno note revise concept:woodbury-identity --section Proof --content-file proof.md \
  --reason "shorter proof via push-through"
```

## Related

- [Custom note types](../custom-note-types.md) — declaring a type and the full feature.
- [The concept library](../../concepts/concept-library.md) and
  [Building a concept library](../../tutorials/concept-library.md) — `note create --origin` and
  `note revise` in use.
