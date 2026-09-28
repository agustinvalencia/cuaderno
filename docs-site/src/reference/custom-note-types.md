# Custom note types

Cuaderno ships twelve built-in note types (see [Note types](../concepts/note-types.md)). If you need
an entity the built-ins don't cover — people, books, clients, recipes — you can declare your own
**custom note type** under `[note_types.<name>]` in `.cuaderno/config.toml`. No recompile, no plugin.

## What a custom type is (and is not)

A custom type is **schema-only**. It gives you:

- a folder its notes live in,
- enforced `required` / `optional` frontmatter fields (checked by `cdno lint`),
- an optional template,
- canonical frontmatter ordering (`cdno normalise`),
- full participation in indexing, full-text search, backlinks, and `cdno note`;
- a creation line in the daily log (`<type> created [[<folder>/<slug>]] — <title>`);
- in-place revision with a logged reason ([`cdno note revise`](cli/note.md#cdno-note-revise-note),
  or `revise_note` over MCP), unless the type is declared `append_only = true`.

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
append_only = false          # optional; `true` makes `note revise` refuse the type (lint does not check it)
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

The note is written to `<folder>/<slug(title)>.md`. Creation is logged to today's daily note as
`<type> created [[<folder>/<slug>]] — <title>`, so do not log it again by hand.

The template is looked up in `.cuaderno/templates/` under the name `template` gives, or
`<name>.md` when the declaration names none. There is no built-in fallback for a custom type: if
that file exists (`.cuaderno/templates/person.md`), it is rendered; otherwise Cuaderno
**synthesises** a minimal note
— a frontmatter block of your fields plus a `# <title>` heading — so a type works before you author
its template. (Field values are always emitted as strings, so a value with a colon, `#`, or newline
round-trips safely; author a template if you need richer frontmatter shapes for keys you do not pass
as fields.)

Whatever a template renders, the frontmatter that is written carries `type: <type>` and every field
you passed (including `--origin`) with the value you passed: a string equal to it, or a plain
number, boolean or null that reads back as the same text (so `priority: {{priority}}` with `5` stays
the number `5`). If the rendered frontmatter already does, the note is written exactly as rendered. Otherwise Cuaderno repairs it — a missing
field is appended, an unquoted `origin: {{origin}}` that YAML would read as a list (or reject) is
rewritten as a quoted string — and re-serialises just the frontmatter block, which drops that
block's comments and quoting style; the note body is left as rendered. So a template that forgets
`{{origin}}`, or renders no frontmatter at all, still records every field you supplied.

If the rendered frontmatter is not valid YAML at all (for example an unquoted `origin: {{origin}}`
given two links), it cannot be repaired key by key, so Cuaderno rebuilds it: `type`, then the
type's declared fields in declared order, each with the value you passed or, for `title`, `slug`,
`created` and `date`, the value create computes. The note body is again left exactly as rendered.
Any key that only the template wrote is lost in that case. The simplest way to avoid it is to
leave `origin` out of the template and let the repair add it.

The body (`--body-file`, or `body` over MCP) is written without its title heading: the engine
writes the `# <title>` H1, and a leading `# <title>` line in the body is dropped. It fills the
template's `{{body}}` placeholder, or is inserted after the H1 when there is none. The body wins
over any other value of `{{body}}`; without one, a declared `body` field, a `[variables]` value or
a prompted `body` fills the placeholder as usual, and it renders empty only when nothing does. The
body is substituted as raw text, so keep `{{body}}` in the note body: in the template's frontmatter
a multi-line body can add or break YAML keys, and nothing refuses that.

From an MCP client, the equivalent tool is `create_custom_note` (`{ type_name, title, fields, vars, body, origin }`).

## Discovering placeholders and searching

- `cdno templates vars person` lists the `{{placeholders}}` a `person` template may reference — its
  create-path built-ins (`title`, `slug`, `created`, `date`, `body`) plus your declared fields.
- `cdno templates eject person` does **not** apply — a custom type has no built-in template to
  materialise; author `.cuaderno/templates/person.md` by hand.
- `cdno search <query> --type person` filters results to that type. `--type` accepts any built-in or
  custom name; a name that is neither errors with the valid set. Shell completion offers your
  vault's types.

## Relationship to `[schemas.*]`

`[note_types.<name>]` *defines* a type: its folder, its `required` and `optional` fields, its
template. `[schemas.<name>]` attaches **typed field declarations** to a type, and it keys a custom
type as well as a built-in. A name under `[note_types]` may not be a built-in.

For a custom type, a `[schemas.<name>.fields.<field>]` declaration (see
[Configuration reference](configuration.md)) does this:

- **Lint type-checks it.** When the field is present in a note, `cdno lint` warns if its value does
  not match the declared `type` (or `values`), e.g. ``field `difficulty` is not a valid int for note
  type `concept` ``. A warning, never an error. A value passed with `--field` (or `fields`) at
  creation is always written as a string, so a typed `int`, `float` or `bool` field set that way
  draws this warning; set it afterwards with `cdno frontmatter set`, which writes the declared type.
- **`settable = true` makes it writable** with [`cdno frontmatter set`](cli/frontmatter.md) or the
  `set_frontmatter` MCP tool, type-checked and without a hand edit. The key must already be in the
  note's frontmatter: the setter rewrites a field, it does not add one.
- **`list_note_types` reports it** in the type's `fields`, so an assistant can see it.

Two parts of a schema do **not** apply to a custom type:

- **`extra_required` is not enforced.** A custom type's required fields come from its own
  `required` list, and that is what `cdno lint` and creation check. An `extra_required` name on a
  custom type only shows up in `list_note_types`, as a `string` field in `fields`. Put the field in
  `required`.
- **A declared `default` is not written at creation.** A custom note's frontmatter carries what the
  template renders plus the fields you pass; to give every new note a field, write it into the
  template.

The schema does not widen what `--field` accepts either: a key you pass at creation must still be
listed in the type's `required` or `optional`.

## Reading and revising

Any note of a custom type can be read whole with [`cdno open`](cli/open.md) (`<type>:<slug>` works
as a reference) or the `read_note` MCP tool, and refined in place with
[`cdno note revise`](cli/note.md#cdno-note-revise-note) or `revise_note`: either the whole body
or one section at a time, with a required reason that is logged as
`revised [[<path>]] — <reason>`. Built-in types are refused (their sections belong to their own
commands), and so is a custom type declared `append_only = true`.

## Worked examples

[Tracking people](../tutorials/tracking-people.md) walks a `person` type end to end — declaring it,
creating people, and linking them from your notes to answer "what was my last interaction with X?".

[The concept library](../concepts/concept-library.md) is the custom type `cdno init` declares for
you, and [Building a concept library](../tutorials/concept-library.md) walks its creation with
`--body-file` and `--origin`, and its revision.
