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

Takes only the [global options](overview.md#global-options).

### Sources

Each placeholder is classified by where its value comes from:

| Source | Meaning |
|--------|---------|
| `supplied` | Filled automatically by the note type's create command. This is the type's **complete** create-path key set — including body placeholders and keys the default template happens not to reference (e.g. `daily`'s `weekday`, `tracking`'s `routine`) — so it matches the per-type table in [Customising templates and frontmatter](../../tutorials/templates-and-frontmatter.md) exactly. |
| `config` | A static `[variables]` entry in `.cuaderno/config.toml`, available to any template. |
| `prompt` | A `[variables.prompt]` entry — a value must be provided at creation (via `--var name=value`, the MCP `vars` parameter, or interactively). The prompt message is shown. |

A config or prompt name that collides with a `supplied` key is omitted: the
supplied value shadows it, so it would never take effect.

With `--json`, emits an array of `{ name, source }` objects (`prompt` entries
also carry `message`).

### Examples

```bash
cdno templates vars project
cdno templates vars tracking
cdno templates vars question --json | jq -r '.[].name'
```

## `cdno templates eject <type>`

Copy a built-in template into `.cuaderno/templates/<type>.md` as an editable
starting point. Note types use an in-binary default until you add a file for
them (only `daily` is seeded on `cdno init`); this materialises one so you can
customise it (add sections, reorder frontmatter, reference `{{placeholders}}`
from `templates vars`) without hand-reconstructing it from the source tree.

```text
cdno templates eject [OPTIONS] <TYPE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<TYPE>` | Note type to eject (same set as `templates vars`). |

### Options

| Flag | Description |
|------|-------------|
| `--force` | Overwrite an existing custom template. Without it, an existing file is left untouched and the command errors. |

Plus the [global options](overview.md#global-options). With `--json`, emits the
`{ path, message }` write result.

Only base note-type templates eject — no `<type>-<variant>` template ships
built-in. To create a `tracking` activity variant, copy one from
[`examples/templates/tracking/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/templates/tracking)
to `.cuaderno/templates/tracking-<activity>.md` instead.

### Examples

```bash
cdno templates eject project              # → .cuaderno/templates/project.md
cdno templates eject tracking             # → the generic tracking template
cdno templates eject project --force      # overwrite an earlier customisation
```

The written file is exactly the built-in default, so a note created straight
after ejecting is byte-identical to before — customise from there.

## Related

- [Customising templates and frontmatter](../../tutorials/templates-and-frontmatter.md) — how to write a custom template and use `[variables]` / `[variables.prompt]`.
