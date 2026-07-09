# Configuration reference

The vault's settings live in `.cuaderno/config.toml`, written by `cdno init`. Every key is optional —
defaults are applied when omitted. For the conceptual tour, see
[Configuration](../concepts/configuration.md).

## Full example

```toml
[vault]
name = "My Research Vault"
max_active_projects = 5            # the active-project cap

# Glob patterns excluded from the index (search, lint, link checks).
# Additive only — no "!" negation. Matched against vault-relative paths.
# NEVER deletes files; only scopes what the index considers.
ignore = ["CLAUDE.md", "README.md"]

# Per-type extra required frontmatter fields. Built-in required fields
# are always enforced; these add vault-specific requirements (cdno lint).
[schemas.project]
extra_required = ["collaborators"]

[schemas.evidence]
extra_required = []

# Typed frontmatter fields for a built-in type. Recognised by the desktop
# Templates editor and type-checked by `cdno lint`.
[schemas.daily.fields.meds]
type = "bool"                     # bool | int | string | date
default = false                  # static, type-checked against `type`

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]   # allowed values (a string constraint)
default = "ok"

# Static template variables — resolve in any custom template ({{author}}).
[variables]
author = "A. Researcher"

# Prompted variables — gathered at note creation (--var, prompt, or error).
[variables.prompt]
collaborators = "Who are the collaborators?"
```

## Keys

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `vault.name` | string | `"My Vault"` | A human label for the vault. |
| `vault.max_active_projects` | integer | `5` | The active-project cap. |
| `ignore` | list of globs | `[]` | Files the index skips. Additive; never deletes. |
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that **built-in** note type, enforced by `cdno lint`. |
| `schemas.<type>.fields.<name>` | table | — | A **typed** frontmatter field for a built-in note type (`type`, `default`, `required`, `values`). Recognised by the Templates editor and type-checked by `cdno lint`. See [Typed schema fields](#typed-schema-fields). |
| `note_types.<name>` | table | — | Declares a **config-defined custom note type** (`folder`, `required`/`optional` fields, `template`, …) — a schema-only type for entities the built-ins don't cover. See [Custom note types](custom-note-types.md). |
| `variables.<name>` | string | — | Static template variable; resolves in any custom template (per-type values win on name clash). |
| `variables.prompt.<name>` | string | — | Prompted template variable; the value is the prompt text. Gathered at creation from `--var name=value`, an interactive prompt, or a static `[variables]` default; errors if none supplies it. |

## Typed schema fields

`[schemas.<type>.fields.<name>]` declares a **typed** frontmatter field on a built-in note type. It
is the richer sibling of `extra_required`: instead of just a name, each field carries a type (and
optionally a default and an allowed-value set). Two things consume it today:

- the desktop **Templates editor** recognises the field, so a custom template referencing
  `{{<name>}}` no longer warns "renders literally";
- **`cdno lint`** type-checks the field — a note whose value doesn't match the declared type (or
  isn't one of `values`) gets a warning.

```toml
[schemas.daily.fields.meds]
type = "bool"                     # bool | int | string | date
default = false                  # optional; static, type-checked against `type`

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]   # optional; allowed values (only valid on a string)
default = "ok"
required = false                 # optional; default false
```

| Field key | Type | Default | Purpose |
|-----------|------|---------|---------|
| `type` | `"bool"` \| `"int"` \| `"string"` \| `"date"` | *(required)* | The field's scalar type. An unknown value is a hard load error. |
| `default` | matching `type` | — | A static default value, type-checked at load. A `date` is a quoted `"YYYY-MM-DD"`. |
| `required` | bool | `false` | Reserved for create-time enforcement (a later release); parsed now. |
| `values` | list of strings | — | An allowed-value constraint. Valid only on a `string` field. |

Notes and limits:

- **Defaults are static** — there is no `"today"` token; a `date` default is a literal calendar date.
- **No `enum` type** — model a closed set as a `string` with `values`.
- **List fields are reserved but not yet implemented** — a `list = true` is a load error today.
- **Engine-owned keys are protected** — you can't declare a field named `type`, or a calendar type's
  own period key (`daily`→`date`, `weekly`→`week`, `monthly`→`month`); the vault refuses to open.
- **`extra_required` still works** and is equivalent to an untyped, non-required `string` field; on a
  name clash an explicit `fields` block wins.
- A malformed field declaration (unknown `type`, a mistyped key, `values` on a non-string, a
  `default` that doesn't type-check) fails at vault-open, like every other config error.

## Templates

Templates live in `.cuaderno/templates/` and are pure variable substitution. `cdno init` writes one
starter (`daily.md`); other types use their built-in default until you add a file. `cdno` selects the
most specific template that exists: a custom variant (e.g. `tracking-gym.md`), then a custom type
(e.g. `project.md`), then the built-in variant default, then the built-in type default. Template
field order is the canonical order
[`cdno normalise`](cli/normalise.md) enforces.

The per-type placeholders that resolve at creation, with a worked example, are in
[Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md). An unknown
placeholder is left verbatim in the note.

Static `[variables]` resolve in any custom template (e.g. `{{author}}`). Prompted
`[variables.prompt]` are gathered at creation (via `--var name=value`, an interactive prompt, or a
static default) — see the
[tutorial](../tutorials/templates-and-frontmatter.md#prompted-variables).

## See also

- [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md) — the tutorial.
- [Configuration](../concepts/configuration.md) — the conceptual overview.
- [Frontmatter fields](frontmatter.md) — what `extra_required` extends.
- [Custom note types](custom-note-types.md) — the `[note_types.*]` table in full.
