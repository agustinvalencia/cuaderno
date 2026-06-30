# Configuration

Vault behaviour is configured in `.cuaderno/config.toml`, written for you by `cdno init`. The
defaults are sensible — you can run for a long time without touching it. This page explains *what* is
configurable and why; the [Configuration reference](../reference/configuration.md) lists every key.

## What you can configure

- **The active-project cap.** Change the default of five active projects.
- **Ignore globs.** Patterns for files the index should skip — `CLAUDE.md`, `README.md`, scratch
  notes — so they don't appear in search, lint, or link checks. Patterns are additive (no negation),
  matched against vault-relative paths, and **never delete anything on disk** — they only scope what
  the index considers.
- **Templates.** Override any built-in note template by adding a file under `.cuaderno/templates/`.
- **Schema extensions.** Add vault-specific required frontmatter fields per note type (e.g. require
  `collaborators` on every project), enforced by `cdno lint`.
- **The active-project cap.** Change the default of five via `[vault] max_active_projects`.

The hands-on walkthrough is [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md).

## Templates

When `cdno` scaffolds a note, it fills a template. Templates are **pure variable substitution** — no
conditionals, no logic. `cdno init` writes one starter template (`.cuaderno/templates/daily.md`);
every other type uses its built-in default until you add a file for it. `cdno` picks the most
specific template that exists:

1. A custom **variant** template (for tracking, e.g. `tracking-gym.md`), then
2. a custom **type** template (e.g. `project.md`), then
3. the **built-in** default compiled into the binary.

Because templates also define the canonical *order* of frontmatter keys,
[`cdno normalise`](../reference/cli/normalise.md) uses them to reorder hand-authored or migrated
notes into a consistent shape.

## Variables

Templates use `{{placeholder}}` markers that `cdno` fills from the values each note's creation
command supplies — `{{title}}`, `{{context}}`, `{{created}}`, and so on. The exact set available per
note type, and how to use them in a custom template, is covered in
[Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md). An unknown
placeholder is left verbatim, so use only the ones a type provides.

> **Config-driven variables are not wired in yet.** `config.toml` accepts `[variables]` (static) and
> `[variables.prompt]` (prompted) sections — and the `init` file documents them — but as of v0.1.24
> they are **parsed but not applied during note creation**. A `{{author}}` backed by
> `[variables] author = "..."` would render literally as `{{author}}`. They're reserved for a future
> release; don't rely on them yet.

## Example

```toml
[vault]
name = "My Research Vault"
max_active_projects = 5            # the active-project cap

# Skip these from the index entirely (search/lint/links). Never deletes files.
ignore = ["CLAUDE.md", "README.md"]

# Require an extra field on every project note — enforced by `cdno lint`:
[schemas.project]
extra_required = ["collaborators"]

# Parsed, but NOT yet applied during note creation (reserved for a future
# release — see the note above):
[variables]
author = "A. Researcher"

[variables.prompt]
collaborators = "Who are the collaborators?"
```

For the complete key-by-key reference, see [Configuration reference](../reference/configuration.md).

That's the concepts tour — next, put it to work in the [Tutorials](../tutorials/daily-loop.md).
