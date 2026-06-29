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
- **Templates.** Override the built-in note templates by editing the copies in `.cuaderno/templates/`.
- **Schema extensions.** Add vault-specific required frontmatter fields per note type (e.g. require
  `collaborators` on every project).
- **Variables.** Static and prompted values your templates can substitute.

## Templates

When `cdno` scaffolds a note, it fills a template. Templates are **pure variable substitution** — no
conditionals, no logic. If you need different shapes for different situations, make different template
files; `cdno` picks the most specific one that exists:

1. An **activity-specific** template (for tracking, e.g. `tracking-gym.md`), then
2. a **type** template (e.g. `project.md`), then
3. the **built-in** default compiled into the binary.

Because templates also define the canonical *order* of frontmatter keys,
[`cdno normalise`](../reference/cli/normalise.md) uses them to reorder hand-authored or migrated
notes into a consistent shape.

## Variables

Templates can reference `{{variables}}`, resolved from four tiers:

1. **Built-in** — computed automatically: `{{date}}`, `{{time}}`, `{{year}}`, `{{month}}`,
   `{{week}}`, `{{weekday}}`, `{{timestamp}}`, and friends.
2. **Contextual** — from the command and vault state: `{{title}}`, `{{slug}}`, `{{context}}`,
   `{{project}}`, `{{portfolio}}`, `{{stewardship}}`, `{{source}}`, `{{core_question}}`, …
3. **Vault-level** — static values you set under `[variables]` in `config.toml` (e.g. your name,
   institution, ORCID).
4. **Prompted** — values under `[variables.prompt]`; if a template needs one and no flag supplied it,
   `cdno` asks interactively (and errors in non-interactive mode).

A special `{{cursor}}` marker tells an interactive editor where to drop your cursor after scaffolding
(it's stripped when Claude provides the content directly).

## Example

```toml
[vault]
name = "My Research Vault"

# Skip these from the index entirely (search/lint/links). Never deletes files.
ignore = ["CLAUDE.md", "README.md"]

# Require an extra field on every project note:
[schemas.project]
extra_required = ["collaborators"]

# Static values usable in any template:
[variables]
author = "A. Researcher"
institution = "University of Examples"

# Values cdno will prompt for when a template needs them:
[variables.prompt]
collaborators = "Who are the collaborators?"
```

For the complete key-by-key reference, see [Configuration reference](../reference/configuration.md).

That's the concepts tour — next, put it to work in the [Tutorials](../tutorials/daily-loop.md).
