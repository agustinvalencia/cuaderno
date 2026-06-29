# Contexts and energy

Two small enumerations show up across many commands. Both are fixed vocabularies (not free text), so
they stay consistent and filterable.

## Contexts — the life domain

A **context** classifies which part of life a project, stewardship, or commitment belongs to. The
set is fixed:

| Context | Typical use |
|---------|-------------|
| `work` | Your main job |
| `side-project` | Personal projects outside work |
| `university` | Studies, coursework, a degree |
| `family` | Family responsibilities |
| `household` | Running a home |
| `legal` | Paperwork, contracts, official matters |
| `personal` | Health, growth, anything else personal |

You set a context when you create a project (`--context`), stewardship (`--context`), or commitment
(`--context`). It groups and colours items in views and lets the system reason about balance across
your life, not just your work.

> Contexts are a compile-time set, not configurable — keeping the vocabulary small and shared is the
> point. (Stewardships accept the same set.)

## Energy — the effort a thing needs

An **energy level** tags how much focus an action demands, so the morning suggestion can match the
work to how you actually feel:

| Energy | Meaning |
|--------|---------|
| `deep` | Heavy, uninterrupted focus (real thinking, hard implementation) |
| `medium` | Moderate focus (routine progress, review) |
| `light` | Low focus (admin, quick wins, tidying) |

You tag an action with `--energy` when you add it. Then:

```bash
# "I have a clear morning" — bias the suggestion toward deep work:
cdno orient --energy deep

# "I'm fried" — surface something light instead:
cdno orient --energy light
```

[`cdno orient`](../reference/cli/orient.md) uses the energy bias to pick *which* next action to
suggest as your starting point. Matching the task to your state — rather than forcing the hardest
thing first — is part of what makes the daily loop sustainable.

Next: [Configuration](configuration.md).
