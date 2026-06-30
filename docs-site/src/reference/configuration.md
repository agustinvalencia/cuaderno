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

# NOTE: [variables] and [variables.prompt] are parsed but NOT yet applied
# during note creation (reserved for a future release). See "Variables".
[variables]
author = "A. Researcher"

[variables.prompt]
collaborators = "Who are the collaborators?"
```

## Keys

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `vault.name` | string | `"My Vault"` | A human label for the vault. |
| `vault.max_active_projects` | integer | `5` | The active-project cap. |
| `ignore` | list of globs | `[]` | Files the index skips. Additive; never deletes. |
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that note type, enforced by `cdno lint`. |
| `variables.<name>` | string | — | Static template variable. **Parsed but not yet applied at note creation.** |
| `variables.prompt.<name>` | string | — | Prompted template variable; value is the prompt text. **Parsed but not yet applied.** |

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

> **`[variables]` / `[variables.prompt]` are not yet applied at note creation** (v0.1.24). They parse
> fine and are reserved for a future release, but a placeholder backed by config (e.g. `{{author}}`)
> currently renders literally rather than being substituted. Stick to the per-type placeholders for now.

## See also

- [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md) — the tutorial.
- [Configuration](../concepts/configuration.md) — the conceptual overview.
- [Frontmatter fields](frontmatter.md) — what `extra_required` extends.
