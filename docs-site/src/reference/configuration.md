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
| `ignore` | list of globs | `[]` | Files the index skips. Additive; never deletes. See [Ignore globs](#ignore-globs). |
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that **built-in** note type, enforced by `cdno lint`. |
| `schemas.<type>.fields.<name>` | table | — | A **typed** frontmatter field for a built-in note type (`type`, `default`, `required`, `values`, `settable`, `log_on_change`). Recognised by the Templates editor, type-checked by `cdno lint`, and (when `settable`) writable via `cdno frontmatter set`. See [Typed schema fields](#typed-schema-fields). |
| `note_types.<name>` | table | — | Declares a **config-defined custom note type** (`folder`, `required`/`optional` fields, `template`, …) — a schema-only type for entities the built-ins don't cover. See [Custom note types](custom-note-types.md). |
| `variables.<name>` | string | — | Static template variable; resolves in any custom template (per-type values win on name clash). |
| `variables.prompt.<name>` | string | — | Prompted template variable; the value is the prompt text. Gathered at creation from `--var name=value`, an interactive prompt, or a static `[variables]` default; errors if none supplies it. |

## Ignore globs

`ignore` lists files that live in the vault directory but are not notes — repo scaffolding like
`CLAUDE.md` or `README.md`. They are excluded from the index, and therefore from **search, lint and
backlinks** as well. The files are never touched on disk.

Patterns are matched against each file's vault-relative path:

| Pattern | Matches |
|---|---|
| `CLAUDE.md` | that file at the vault root |
| `**/*.draft.md` | a `.draft.md` at any depth |
| `folder/*/**` | everything **one or more** levels below `folder/<anything>/` |
| `folder/*/*/**` | everything **two or more** levels below `folder/<anything>/` |

The last two are the trap worth knowing. `*` stays inside one path segment but `**` is recursive, so
`portfolios/*/**` does not mean "the level below a portfolio" — it matches the portfolio's own notes
as well as anything nested under them. A glob written that way excludes every note in the folder it
was meant to tidy, and because an unindexed note is also unsearchable and unlinkable, the symptom
looks like a broken view rather than a misconfigured vault.

Two things guard against that:

- `cdno reindex` prints how many files the globs excluded.
- The desktop app shows a dismissible notice when the count looks disproportionate — a lone
  `CLAUDE.md` stays silent, a glob swallowing a large share of the vault does not.

If notes go missing, clear the pattern and run `cdno reindex`: every row comes back.

Note that attachment artefacts filed into a portfolio need no `ignore` entry — they are excluded
automatically, by location. See [vault structure](../concepts/vault-structure.md).

## Typed schema fields

`[schemas.<type>.fields.<name>]` declares a **typed** frontmatter field on a built-in note type. It
is the richer sibling of `extra_required`: instead of just a name, each field carries a type (and
optionally a default and an allowed-value set). Four things consume it today:

- the desktop **Templates editor** recognises the field, so a custom template referencing
  `{{<name>}}` no longer warns "renders literally";
- **note creation** populates the field's `default` at create — a custom template referencing
  `{{<name>}}` renders that default (a field with no default renders `null`), so the value lands in
  the new note's frontmatter instead of a literal `{{<name>}}`;
- **`cdno lint`** type-checks the field — a note whose value doesn't match the declared type (or
  isn't one of `values`) gets a warning;
- the **`set_frontmatter` setter** (`cdno frontmatter set`, MCP `set_frontmatter`) writes the field
  through the index when it is marked `settable = true` — see the
  [`cdno frontmatter` reference](cli/frontmatter.md).

```toml
[schemas.daily.fields.meds]
type = "bool"                     # bool | int | string | date
default = false                  # optional; static, type-checked against `type`
settable = true                  # optional; allow `set_frontmatter` to write it (default false)
log_on_change = true             # optional; stamp a daily-log line when it changes

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]   # optional; allowed values (only valid on a string)
default = "ok"
required = false                 # optional; default false
```

| Field key | Type | Default | Purpose |
|-----------|------|---------|---------|
| `type` | `"bool"` \| `"int"` \| `"string"` \| `"date"` | *(required)* | The field's scalar type. An unknown value is a hard load error. |
| `default` | matching `type` | — | A static default value, type-checked at load. **Populated at create** when a custom template references `{{<name>}}`. A `date` is a quoted `"YYYY-MM-DD"`. |
| `required` | bool | `false` | Reserved for create-time enforcement (a later release); parsed now, but inert — it does not yet block creation. |
| `values` | list of strings | — | An allowed-value constraint. Valid only on a `string` field. |
| `settable` | bool | `false` | Whether `set_frontmatter` (`cdno frontmatter set`, MCP `set_frontmatter`) may write this field. **Default-deny**: absent or `false` means not settable. Never overrides an engine-owned key (`type`, `status`, a period key) — those stay blocked regardless. |
| `log_on_change` | bool | `false` | When a `settable` field's value actually changes, stamp a `key: old → new` line into today's daily note in the same commit. |

Notes and limits:

- **Defaults are static** — there is no `"today"` token; a `date` default is a literal calendar date.
- **A field only lands in frontmatter if a custom template references it** — rendering substitutes
  the `{{<name>}}` tokens a template contains; it never adds a frontmatter line. The shipped built-in
  templates can't reference vault-specific fields, so populate a declared field by adding a custom
  `.cuaderno/templates/<type>.md` that references `{{<name>}}`.
- **A create-path value wins over a declared default** — if the note type's create path already
  supplies a value for that name (an engine-supplied placeholder), that value takes precedence and
  the declared default does not apply. Likewise a `[variables]` static var of the same name wins over
  a schema default.
- **A `[variables.prompt]` name is owned by the prompt** — if a field name is also a prompted
  variable, its value is collected via the prompt (from `--var`, an interactive prompt, or a static
  default), and the schema default is not used. This ensures a supplied answer is never discarded.
- **No `enum` type** — model a closed set as a `string` with `values`.
- **List fields are reserved but not yet implemented** — a `list = true` is a load error today.
- **Engine-owned keys are protected** — you can't declare a field named `type`, or a calendar type's
  own period key (`daily`→`date`, `weekly`→`week`, `monthly`→`month`); the vault refuses to open.
  `set_frontmatter` additionally refuses to write `status` for every type — even if a vault declares
  a `status` field `settable = true` — so the lifecycle commands stay its sole writers.
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

## Editing from the desktop app

You can edit `.cuaderno/config.toml` directly from the desktop app's **Config** view, without
hand-editing the file. It offers a **Raw** text editor and a structured **Form** for note types and
schema extensions; **Check** dry-runs the same validation the app runs when it opens a vault.

Saving is gated so an edit — from either view — can never leave the vault unopenable:

1. The whole candidate is **validated first** — the exact check the app runs at open (TOML parse,
   `ignore` globs, and the `[note_types.*]` / `[schemas.*]` rules). If it would not reopen, the save
   is refused and the file is left untouched.
2. A **content-hash compare-and-swap** then guards against a concurrent hand-edit: if the file
   changed on disk since the editor read it, the save is refused with a "changed on disk — reload"
   notice rather than overwriting the newer version.
3. The vault is then **reloaded live**, so the edit applies with no restart. A Raw save writes the
   buffer **verbatim**; a Form save applies a **surgical** edit to just the table it changed — either
   way comments, key order, and the `[variables]` block are preserved.

The full walkthrough of the Config view is in
[Editing the config in the app](../getting-started/config-editor.md).

## See also

- [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md) — the tutorial.
- [Configuration](../concepts/configuration.md) — the conceptual overview.
- [Frontmatter fields](frontmatter.md) — what `extra_required` extends.
- [Custom note types](custom-note-types.md) — the `[note_types.*]` table in full.
