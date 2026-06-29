# Frontmatter fields

Every note begins with a YAML frontmatter block. Cuaderno parses it into a typed structure — if it
parses, it's valid (see [Business rules](../concepts/business-rules.md)). This page lists the fields
per [note type](../concepts/note-types.md). `?` marks an optional field.

> Notes that `cdno` creates are already well-formed. You mainly need this when hand-authoring or
> migrating notes — and [`cdno normalise`](cli/normalise.md) will reorder keys to the canonical order
> for you.

## `daily`

```yaml
type: daily
date: 2026-04-25
tags: []          # auto-populated
```

## `weekly`

```yaml
type: weekly
week: 2026-W17
date_start: 2026-04-20
date_end: 2026-04-26
```

## `project`

```yaml
type: project
context: work          # work | side-project | university | family | household | legal | personal
status: active         # active | parked
created: 2026-04-25
core_question?: "[[questions/research/surrogate-cost]]"
```

## `action`

```yaml
type: action
status: active         # active | completed | blocked
project: surrogate-model
energy: deep           # deep | medium | light
milestone?: "[[...]]"
due?: 2026-05-10
criteria?: "Definition of done"
blocker?: "What it's waiting on"
created: 2026-04-25
completed?: 2026-05-01
tags?: []
```

## `portfolio`

```yaml
type: portfolio
question: "Sparse vs dense attention OOD"
created: 2026-03-01
project?: "[[projects/surrogate-model]]"
```

## `evidence`

```yaml
type: evidence
created: 2026-03-15
source: "Chen et al. 2025"
portfolio: sparse-vs-dense-attention-ood
origin: "[[projects/surrogate-model]]"
kind?: pdf             # only for attachment stubs (pdf | image | video | …)
```

## `stewardship`

```yaml
type: stewardship
context: personal      # one of the life-domain contexts
```

## `tracking`

```yaml
type: tracking
stewardship: health
activity: gym
date: 2026-04-06
duration_min?: 60
routine?: "[[stewardships/health/routines/upper-body-a]]"
```

## `question`

```yaml
type: question
domain: research       # research | life
status: active         # active | parked | answered | retired
created: 2026-04-25
updated?: 2026-05-01
```

## `commitment`

```yaml
type: commitment
due: 2026-06-01
context: personal
project?: icml-paper
stewardship?: finances
```

## Extending schemas

You can require additional fields per type via `config.toml` (`[schemas.<type>] extra_required`) —
see [Configuration reference](configuration.md). Required built-in fields are always enforced on top.
