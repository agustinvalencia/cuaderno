# Configuration

Vault behaviour is configured in `.cuaderno/config.toml`, written for you by `cdno init`. The
defaults are sensible — you can run for a long time without touching it. This page explains *what* is
configurable and why; the [Configuration reference](../reference/configuration.md) lists every key.

## What you can configure

- **The active-project cap.** Change the default of five via `[vault] max_active_projects`.
- **Ignore globs.** Patterns for files the index should skip — `CLAUDE.md`, `README.md`, scratch
  notes — so they don't appear in search, lint, or link checks. Patterns are additive (no negation),
  matched against vault-relative paths, and **never delete anything on disk** — they only scope what
  the index considers.
- **Templates.** Override any built-in note template by adding a file under `.cuaderno/templates/`.
- **Schema extensions.** Add vault-specific required frontmatter fields per note type (e.g. require
  `collaborators` on every project), enforced by `cdno lint`.

The hands-on walkthrough is [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md).

## Templates

When `cdno` scaffolds a note, it fills a template. Templates are **pure variable substitution** — no
conditionals, no logic. `cdno init` writes one starter template (`.cuaderno/templates/daily.md`);
every other type uses its built-in default until you add a file for it. `cdno` picks the most
specific template that exists:

1. a custom **variant** template (for tracking, e.g. `tracking-gym.md`), then
2. a custom **type** template (e.g. `project.md`), then
3. the **built-in variant** default, then
4. the **built-in type** default.

Because templates also define the canonical *order* of frontmatter keys,
[`cdno normalise`](../reference/cli/normalise.md) uses them to reorder hand-authored or migrated
notes into a consistent shape.

## Variables

Templates use `{{placeholder}}` markers that `cdno` fills from the values each note's creation
command supplies — `{{title}}`, `{{context}}`, `{{created}}`, and so on. The exact set available per
note type, and how to use them in a custom template, is covered in
[Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md). An unknown
placeholder is left verbatim, so use only the ones a type provides.

Custom templates can also reference **static vault variables** you set under `[variables]` in
`config.toml` (e.g. `{{author}}`); these resolve on every note type, with per-type values taking
precedence over a config variable of the same name.

> **Prompted variables (`[variables.prompt]`) aren't wired in yet** — that section is parsed but not
> applied at note creation, so a `{{ticket}}` backed by it still renders literally for now. Static
> `[variables]` do work. Prompted variables land in a follow-up.

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

# Static template variables — resolve in any custom template (e.g. {{author}}):
[variables]
author = "A. Researcher"

# Prompted variables — parsed but NOT yet applied at note creation (follow-up):
[variables.prompt]
collaborators = "Who are the collaborators?"
```

For the complete key-by-key reference, see [Configuration reference](../reference/configuration.md).

That's the concepts tour — next, put it to work in the [Tutorials](../tutorials/daily-loop.md).
