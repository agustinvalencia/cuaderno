# Custom note types

Cuaderno ships eleven built-in note types (see [Note types](../concepts/note-types.md)). If you need
an entity the built-ins don't cover — people, books, clients, recipes — you can declare your own
**custom note type** under `[note_types.<name>]` in `.cuaderno/config.toml`. No recompile, no plugin.

## What a custom type is (and is not)

A custom type is **schema-only**. It gives you:

- a folder its notes live in,
- enforced `required` / `optional` frontmatter fields (checked by `cdno lint`),
- an optional template,
- canonical frontmatter ordering (`cdno normalise`),
- full participation in indexing, full-text search, backlinks, and `cdno note`.

It does **not** get bespoke behaviour. The 5-project cap, project state history, commitment
aggregation, tracking streams, and action lifecycle belong to specific built-in types and are not
available to custom types. A custom type is therefore invisible to `cdno orient`, the project cap,
and the commitments view — by design. If you need that behaviour, use (or extend) a built-in type.

## Declaring a type

```toml
[note_types.person]
folder = "people"            # required — vault-relative, must not be a built-in folder
required = ["name"]          # fields that must be present and non-null (lint errors otherwise)
optional = ["role", "org"]   # fields that may be present; part of the canonical order
template = "person.md"       # optional; defaults to "<name>.md" under .cuaderno/templates/
append_only = false          # optional; accepted now, lint enforcement is a later addition
title_field = "name"         # optional; which frontmatter field holds the display title (default: the H1)
date_field = "met_on"        # optional; which field carries the note's date (for date-filtered search)
```

Validation runs at vault-open, so a malformed declaration fails fast. Cuaderno rejects a type whose:

- `folder` is empty, has surrounding whitespace, escapes the vault (`..`, absolute, `\`), or
  collides with a built-in folder (`projects`, `journal`, …) or another custom type's folder;
- `template` is not a bare filename;
- `title_field` / `date_field` names a field that isn't in `required`/`optional`;
- **name shadows a built-in type** (`project`, `daily`, … — case-insensitive). Built-in names are
  reserved so a stray `type:` typo can't silently mint a type.

## Creating notes

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note list person
```

`--field name=value` is repeatable; each key must be a declared `required`/`optional` field, and
every `required` field must be supplied. `--var name=value` supplies a template's
[prompted variables](../tutorials/templates-and-frontmatter.md#prompted-variables).

The note is written to `<folder>/<slug(title)>.md`. If the type has a template
(`.cuaderno/templates/person.md`), it is rendered; otherwise Cuaderno **synthesises** a minimal note
— a frontmatter block of your fields plus a `# <title>` heading — so a type works before you author
its template. (Field values are always emitted as strings, so a value with a colon, `#`, or newline
round-trips safely; author a template if you need richer frontmatter shapes.)

From an MCP client, the equivalent tool is `create_custom_note` (`{ type_name, title, fields, vars }`).

## Discovering placeholders and searching

- `cdno templates vars person` lists the `{{placeholders}}` a `person` template may reference — its
  create-path built-ins (`title`, `slug`, `created`, `date`) plus your declared fields.
- `cdno templates eject person` does **not** apply — a custom type has no built-in template to
  materialise; author `.cuaderno/templates/person.md` by hand.
- `cdno search <query> --type person` filters results to that type. `--type` accepts any built-in or
  custom name; a name that is neither errors with the valid set. Shell completion offers your
  vault's types.

## Relationship to `[schemas.*]`

`[note_types.*]` *defines* a new type; `[schemas.<builtin>]` *extends* a built-in with
`extra_required` fields. They are separate tables with separate purposes — a name under
`[note_types]` may not be a built-in, and `[schemas.<custom>]` has no effect (a custom type's
required fields come from its own `required` list).

## A worked example

[Tracking people](../tutorials/tracking-people.md) walks a `person` type end to end — declaring it,
creating people, and linking them from your notes to answer "what was my last interaction with X?".
