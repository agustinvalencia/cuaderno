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
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that note type, enforced by `cdno lint`. |
| `variables.<name>` | string | — | Static template variable; resolves in any custom template (per-type values win on name clash). |
| `variables.prompt.<name>` | string | — | Prompted template variable; the value is the prompt text. Gathered at creation from `--var name=value`, an interactive prompt, or a static `[variables]` default; errors if none supplies it. |

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
