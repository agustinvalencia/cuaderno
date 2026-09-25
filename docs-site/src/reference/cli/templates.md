# `cdno templates`

Inspect and edit note templates. Use `vars` before writing a custom template in
`.cuaderno/templates/` to see which `{{placeholders}}` a note type supports —
unknown placeholders render verbatim, so this is how you learn the valid set
without reading the source.

| Command | What it does |
|---|---|
| `vars <TYPE>` | List the `{{placeholders}}` a type's template supports. |
| `list` | Every note type, which template is in effect, and where its override lives. |
| `show <TYPE>` | Print a template's effective content verbatim. |
| `eject <TYPE>` | Copy a built-in template into `.cuaderno/templates/` to customise. |
| `save` | Write a template, from a file, from stdin, or via `$EDITOR`. |
| `new` | Scaffold a starter template for a config-defined custom type. |

## `cdno templates vars <type>`

List the `{{placeholders}}` a note type's template supports.

```text
cdno templates vars [OPTIONS] <TYPE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<TYPE>` | Note type: `project`, `action`, `question`, `portfolio`, `evidence`, `stewardship`, `tracking`, `commitment`, `daily`, `weekly`, `inbox`, or a [config-defined custom type](../custom-note-types.md). |

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
| `<TYPE>` | Built-in note type to eject. Omit when using `--all`. A [config-defined custom type](../custom-note-types.md) has no built-in template to eject, so it is refused here — use [`templates new`](#cdno-templates-new) to scaffold one instead. Unlike `templates vars`, which does accept custom types. |

### Options

| Flag | Description |
|------|-------------|
| `--all` | Eject **every** built-in template into `.cuaderno/templates/` at once. Types that already have a template file are skipped (a summary reports which), unless `--force`. Mutually exclusive with `<TYPE>` — pass one or the other. |
| `--force` | Overwrite existing custom template(s). Without it, an existing file is left untouched (and single-type eject errors; `--all` skips it). |

Plus the [global options](overview.md#global-options). With `--json`, single-type
eject emits the `{ path, message }` write result; `--all` emits an object with
`written` and `skipped` arrays (note-type names).

Only base note-type templates eject — no `<type>-<variant>` template ships
built-in. To create a `tracking` activity variant, copy one from
[`examples/templates/tracking/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/templates/tracking)
to `.cuaderno/templates/tracking-<activity>.md` instead.

### Examples

```bash
cdno templates eject project              # → .cuaderno/templates/project.md
cdno templates eject tracking             # → the generic tracking template
cdno templates eject project --force      # overwrite an earlier customisation
cdno templates eject --all                # eject all built-ins, skip customised
cdno templates eject --all --force        # eject all, overwriting everything
```

The written file is exactly the built-in default, so a note created straight
after ejecting is byte-identical to before — customise from there.

## `cdno templates list`

Every note type with the state of its template: whether a custom override exists, which source is in
effect, and the path the override lives (or would live) at.

```text
cdno templates list [OPTIONS]
```

`--json` reports `source` as a stable token — `builtin_default`, `builtin_variant`, `custom_base`,
`custom_variant`, or `none` — rather than the human label in the table.

## `cdno templates show <type>`

Print a template's effective content verbatim: the custom override when one exists, else the
built-in default. A custom type with no template yet shows the starter `new` would write.

```text
cdno templates show [OPTIONS] <TYPE>
```

| Option | Description |
|--------|-------------|
| `--variant <NAME>` | Show a `<type>-<variant>` template instead of the base one. Built-in types only. |

Output is byte-verbatim, so it round-trips:

```bash
cdno templates eject project
cdno templates show project | diff - .cuaderno/templates/project.md   # no output
```

`--json` emits `{content, source}` instead.

## `cdno templates save`

Write a template. On a built-in type this creates the custom override transparently — no prior
`eject` needed.

```text
cdno templates save [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--note-type <TYPE>` | The type whose template to write. |
| `--variant <NAME>` | Write the `<type>-<variant>` template. Built-in types only. |
| `--file <PATH>` | Read the new content from this file, or from stdin when it is `-`. |

Without `--file`, an interactive run opens `$EDITOR` seeded with the template as it stands. Off a
terminal `--file` is required, so a scripted `save` cannot silently truncate a template to nothing.

```bash
cdno templates save --note-type project --file my-project.md
cat my-project.md | cdno templates save --note-type project --file -
```

## `cdno templates new`

Scaffold a starter template for a [config-defined custom type](../custom-note-types.md) that has
none yet — a frontmatter block of `type` plus each declared `required` field as a `{{placeholder}}`,
and a `# {{title}}` heading.

```text
cdno templates new [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--note-type <TYPE>` | The custom type to scaffold for. |

Refuses a built-in type, which already has a default to edit — use `eject` or `save` for those — and
refuses to overwrite an existing template.

A custom type has exactly one template, so `--variant` is not accepted on `show`, `save` or `new` for
one; declare a separate note type instead.

## Related

- [Customising templates and frontmatter](../../tutorials/templates-and-frontmatter.md) — how to write a custom template and use `[variables]` / `[variables.prompt]`.
