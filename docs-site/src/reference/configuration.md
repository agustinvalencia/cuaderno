# Configuration reference

The vault's settings live in `.cuaderno/config.toml`, written by `cdno init`. Every key is optional —
defaults are applied when omitted. For the conceptual tour, see
[Configuration](../concepts/configuration.md).

## Full example

```toml
[vault]
name = "My Research Vault"

# Glob patterns excluded from the index (search, lint, link checks).
# Additive only — no "!" negation. Matched against vault-relative paths.
# NEVER deletes files; only scopes what the index considers.
ignore = ["CLAUDE.md", "README.md"]

# Per-type extra required frontmatter fields. Built-in required fields
# are always enforced; these add vault-specific requirements.
[schemas.project]
extra_required = ["collaborators"]

[schemas.evidence]
extra_required = []

# Static values available to every template (tier 3).
[variables]
author = "A. Researcher"
institution = "University of Examples"
orcid = "0000-0000-0000-0000"

# Values cdno prompts for when a template needs them and no flag supplied
# one (tier 4). The string is the prompt shown.
[variables.prompt]
collaborators = "Who are the collaborators?"
experiment_id = "Experiment identifier?"
```

## Keys

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `vault.name` | string | — | A human label for the vault. |
| `ignore` | list of globs | `[]` | Files the index skips. Additive; never deletes. |
| `schemas.<type>.extra_required` | list of strings | `[]` | Extra required frontmatter fields for that note type. |
| `variables.<name>` | string | — | Static template variable (tier 3). |
| `variables.prompt.<name>` | string | — | Prompted template variable (tier 4); value is the prompt text. |

> The **active-project cap** (default 5) is configurable here as well; see
> [Business rules](../concepts/business-rules.md#the-five-project-cap).

## Templates

Templates live in `.cuaderno/templates/` and are pure variable substitution. `cdno` selects the most
specific that exists: an activity-specific template (e.g. `tracking-gym.md`), then a type template
(e.g. `project.md`), then the built-in default. Template field order is the canonical order
[`cdno normalise`](cli/normalise.md) enforces.

### Variable tiers

1. **Built-in** (computed): `{{date}}`, `{{time}}`, `{{year}}`, `{{month}}`, `{{week}}`,
   `{{weekday}}`, `{{day_short}}`, `{{timestamp}}`, `{{date_iso}}`.
2. **Contextual** (command/vault state): `{{title}}`, `{{slug}}`, `{{context}}`, `{{project}}`,
   `{{portfolio}}`, `{{stewardship}}`, `{{routine}}`, `{{source}}`, `{{core_question}}`,
   `{{active_projects}}`.
3. **Vault-level**: anything under `[variables]`.
4. **Prompted**: anything under `[variables.prompt]`.

A `{{cursor}}` marker indicates where an interactive editor should place the cursor after scaffolding
(stripped when Claude supplies content directly).

## See also

- [Configuration](../concepts/configuration.md) — the conceptual overview.
- [Frontmatter fields](frontmatter.md) — what `extra_required` extends.
