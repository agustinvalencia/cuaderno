# `cdno templates`

Inspect note templates. Use it before writing a custom template in
`.cuaderno/templates/` to see which `{{placeholders}}` a note type supports —
unknown placeholders render verbatim, so this is how you learn the valid set
without reading the source.

## `cdno templates vars <type>`

List the `{{placeholders}}` a note type's template supports.

```text
cdno templates vars [OPTIONS] <TYPE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<TYPE>` | Note type: `project`, `action`, `question`, `portfolio`, `evidence`, `stewardship`, `tracking`, `commitment`, `daily`, `weekly`, or `inbox`. |

### Options

| Flag | Description |
|------|-------------|
| `--variant <VARIANT>` | Template variant (e.g. `gym` for `tracking`) — selects the variant's built-in template when one exists. |

Plus the [global options](overview.md#global-options).

### Sources

Each placeholder is classified by where its value comes from:

| Source | Meaning |
|--------|---------|
| `supplied` | Filled automatically by the note type's create command. Derived from the built-in template, so the list can't drift from what the scaffold actually provides. |
| `config` | A static `[variables]` entry in `.cuaderno/config.toml`, available to any template. |
| `prompt` | A `[variables.prompt]` entry — a value must be provided at creation (via `--var name=value`, the MCP `vars` parameter, or interactively). The prompt message is shown. |

A config or prompt name that collides with a `supplied` key is omitted: the
supplied value shadows it, so it would never take effect.

With `--json`, emits an array of `{ name, source }` objects (`prompt` entries
also carry `message`).

### Examples

```bash
cdno templates vars project
cdno templates vars tracking --variant gym
cdno templates vars question --json | jq -r '.[].name'
```

## Related

- [Customising templates and frontmatter](../../tutorials/templates-and-frontmatter.md) — how to write a custom template and use `[variables]` / `[variables.prompt]`.
