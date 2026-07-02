# `cdno note`

Create and list notes of a [config-defined custom type](../custom-note-types.md) (declared under
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

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result. If the type ships no template (`.cuaderno/templates/<type>.md`), a minimal note is
synthesised from the declared fields plus a `# <title>` heading.

## `cdno note list <type>`

List every note of custom type `<type>`, by path.

```text
cdno note list <TYPE>
```

## Examples

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note list person
```

## Related

- [Custom note types](../custom-note-types.md) — declaring a type and the full feature.
