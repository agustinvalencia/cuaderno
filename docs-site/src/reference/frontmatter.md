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

## `monthly`

```yaml
type: monthly
month: 2026-04
date_start: 2026-04-01
date_end: 2026-04-30
```

## `project`

```yaml
type: project
context: work          # work | side-project | university | family | household | legal | personal
status: active         # active | parked | completed | dropped
created: 2026-04-25
core_question?: "[[questions/research/surrogate-cost]]"
held_by?: "the harbour office"   # who else is moving it; set by hand-off, cleared by take-back
closed: null           # set by complete/drop, cleared by activate
```

`status` and `closed` are owned by the lifecycle verbs (`park`, `activate`, `complete`, `drop`), so
`cdno frontmatter set` refuses both. `status` has to match the folder the map lives in: `projects/`
for `active`, `projects/_parked/` for `parked`, and `projects/_done/<year>/` for `completed` or
`dropped`. A map written before `closed:` existed needs no edit, since a missing `closed:` reads as
open.

`held_by` (optional, on an active or parked map) names whoever else is moving the project. An active
map that names a holder does not count toward the
[cap](../concepts/business-rules.md#handed-off-projects); the map keeps its status and folder.
[`cdno project hand-off`](cli/project.md#cdno-project-hand-off) writes it quoted and
[`take-back`](cli/project.md#cdno-project-take-back) writes `held_by: null`, each logging
`holder on [[<slug>]]`, so `cdno frontmatter set` refuses it. When you edit it by hand:

- A missing line, `null`, a blank value, or a value on
  [`[projects] holder_sentinels`](configuration.md#projects) (`me`, `tbd`, `someone`, …) means no
  holder: the map is yours and counts. `cdno lint` warns on a sentinel or blank value.
- A value that is not a plain string (an unquoted `[[people/ana]]`, which YAML reads as a list, a
  number, a date) also means no holder, and `cdno lint` reports it as an error. Quote a wikilink:
  `held_by: "[[people/ana]]"`.

## `action`

```yaml
type: action
status: active         # active | completed | blocked | dropped
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
status: active         # active | completed | dropped
due: 2026-06-01
created: 2026-04-25
completed?: 2026-06-01
context: personal
project?: icml-paper
stewardship?: finances
```

## Extending schemas

You can require additional fields per type via `config.toml` (`[schemas.<type>] extra_required`) —
see [Configuration reference](configuration.md). Required built-in fields are always enforced on top.
