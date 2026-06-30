# Customising templates and frontmatter

When `cdno` scaffolds a note it fills a **template**. Every note type has a built-in template, and you
can override any of them per-vault — to change the structure, the default sections, or the frontmatter
fields. You can also **require extra frontmatter fields** so [`cdno lint`](../reference/cli/lint.md)
keeps your notes consistent. This tutorial walks through both, hands-on.

> What's covered here is the shipped behaviour. Config-driven template *variables*
> (`[variables]` / `[variables.prompt]`) are recognised in `config.toml` but **not yet applied during
> note creation** — see [the caveat below](#a-note-on-config-variables).

## Where templates live

Templates live in `.cuaderno/templates/`, one Markdown file per note type (e.g. `project.md`,
`evidence.md`). `cdno` resolves the **effective** template at creation time:

1. a custom **variant** file — `<type>-<variant>.md` (only `tracking` has variants: `gym`, `body`, `swim`), then
2. a custom **type** file — `<type>.md`, then
3. the built-in **variant** default, then
4. the built-in **type** default.

So a custom file in `.cuaderno/templates/` always wins over the built-in.

> `cdno init` writes just one starter template — `.cuaderno/templates/daily.md`. Every other type
> uses its built-in default until you add a file for it. To customise one, create that file yourself
> (the next section shows how).

## Customise a template

Say you want every project to start with a **## Risks** section. Create
`.cuaderno/templates/project.md`:

```markdown
---
type: project
context: {{context}}
status: {{status}}
created: {{created}}
core_question: {{core_question}}
---

# {{title}}

## Current State

## Risks

## Next Actions
```

Now create a project:

```bash
cdno project create --title "Surrogate model" --context work
```

The new `projects/surrogate-model.md` follows your template — including the `## Risks` section:

```markdown
---
type: project
context: work
status: active
created: 2026-06-30
core_question: null
---

# Surrogate model

## Current State

## Risks

## Next Actions
```

A good way to start is to copy the built-in as your base, then edit. The built-ins live in the
[source tree](https://github.com/agustinvalencia/cuaderno/tree/main/crates/cdno-domain/templates);
or just look at a note `cdno` already created and shape the template to match.

> Editing a template only affects notes created **afterwards** — existing notes are untouched. (And
> `cdno normalise` only reorders frontmatter keys; it won't add a new section like `## Risks` to old
> notes.)

### Tracking variants

`tracking` is the one type with variants. A custom `.cuaderno/templates/tracking-gym.md` overrides
the gym template specifically, while `.cuaderno/templates/tracking.md` overrides the generic fallback
used for every other activity. So you can give `cdno track gym` a bespoke layout without touching
`cdno track swim`.

## Template variables

Templates use `{{placeholder}}` markers. `cdno` substitutes the values the note's creation command
supplies. Two rules to know:

- An **omitted optional** value renders as `null` (e.g. `core_question: null` above when you don't
  pass `--question`).
- An **unknown** placeholder is left **verbatim** — `{{nope}}` stays as the literal text `{{nope}}`
  in the note. So a template should only use the placeholders its note type actually provides.

Each type provides these:

| Note type | Available `{{placeholders}}` |
|-----------|------------------------------|
| `daily` | `date`, `heading`, `weekday` |
| `weekly` | `week`, `week_num`, `year`, `date_start`, `date_end` |
| `project` | `title`, `context`, `status`, `created`, `core_question` |
| `action` | `title`, `slug`, `project`, `energy`, `status`, `created`, `due`, `completed`, `milestone`, `criteria`, `blocker`, `tags` |
| `portfolio` | `question`, `project`, `created` |
| `evidence` | `source`, `origin`, `portfolio`, `content`, `created` |
| `stewardship` | `name`, `context` |
| `tracking` | `stewardship`, `activity`, `activity_title`, `routine`, `content`, `date`, `date_long` |
| `question` | `question`, `domain`, `created`, `updated` |
| `commitment` | `title`, `context`, `status`, `due`, `project`, `stewardship`, `created`, `completed` |
| `inbox` | `body`, `created` |

You can use any subset, in any order, and add as much static Markdown around them as you like.

### Static config variables

Beyond the per-type placeholders above, a custom template can reference **vault-wide static
variables** you define under `[variables]` in `.cuaderno/config.toml`. These resolve on every note
type. For example:

```toml
# .cuaderno/config.toml
[variables]
author = "A. Researcher"
institution = "University of Examples"
```

A custom template can then use `{{author}}` / `{{institution}}` and they'll be substituted at
creation. Precedence: a per-type (contextual) placeholder of the same name always wins over a config
variable, so config vars only fill names the note type doesn't already supply.

> **Prompted variables (`[variables.prompt]`) aren't wired in yet.** That section is parsed but not
> applied at note creation — a `{{ticket}}` backed by `[variables.prompt]` still renders literally for
> now. Static `[variables]` (above) do work. Prompted variables are coming in a follow-up.

## Frontmatter field order and `normalise`

Your template also defines the **canonical order** of frontmatter keys for that type. Notes `cdno`
creates are already in that order; for hand-authored or migrated notes,
[`cdno normalise`](../reference/cli/normalise.md) reorders their frontmatter to match the template
(`--check` reports without writing). So if you reorder the keys in `project.md`, a later
`cdno normalise` brings older project notes into line.

## Require extra frontmatter fields

Beyond the built-in required fields, you can demand vault-specific ones per type with a
`[schemas.<type>]` section. For example, to require every project to name an `owner`, add to
`.cuaderno/config.toml`:

```toml
[schemas.project]
extra_required = ["owner"]
```

This is enforced by [`cdno lint`](../reference/cli/lint.md), which now **errors** on any project
missing the field (a missing key, or one whose value is null, fails):

```bash
cdno lint
# [error] projects/surrogate-model.md: missing required field `owner` for note type `project`
# Error: found 1 error(s), 0 warning(s)
```

`lint` exits non-zero on errors, so this is a good gate to run in a pre-commit hook or CI over a
git-tracked vault.

### Satisfy the requirement going forward

Add the field to your template so **new** notes carry it. Give it a non-null default you can edit per
note (an empty key — `owner:` — is YAML `null` and still fails the lint; use a placeholder value):

```markdown
---
type: project
context: {{context}}
status: {{status}}
created: {{created}}
core_question: {{core_question}}
owner: unassigned
---
```

New projects are now born with `owner: unassigned` (edit it as needed) and pass the lint.
**Existing** notes aren't changed retroactively — fix them by adding the field, then re-run
`cdno lint` until it's clean.

> Required fields are about *presence*, not value: any non-null value satisfies the check. Combine
> `extra_required` with a template default and the occasional `cdno lint` and your vault stays
> uniform without any per-note ceremony.

## See also

- [Configuration](../concepts/configuration.md) — the configurable surface.
- [Configuration reference](../reference/configuration.md) — every `config.toml` key.
- [Frontmatter fields](../reference/frontmatter.md) — the built-in fields per note type.
- [`normalise`](../reference/cli/normalise.md), [`lint`](../reference/cli/lint.md).
