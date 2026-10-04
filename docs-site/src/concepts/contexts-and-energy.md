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

## Focus

Energy says what kind of work to pick; **focus** says what you are on right now. It is a single
slot, and it is deliberate at both ends: you put something in it, and you take it out.

| To… | Run | Log line |
|-----|-----|----------|
| put an action in the slot | [`action start`](../reference/cli/action.md#cdno-action-start) | `started [[slug]] — text` |
| finish it | `action complete` | `action done on …` |
| abandon it | `action drop` | `action dropped on …` |
| set it aside, to come back | [`action pause`](../reference/cli/action.md#cdno-action-pause) | `action paused on …`, with `next:` and `reason:` |
| move to something else | [`action switch`](../reference/cli/action.md#cdno-action-switch) | a `paused` line then a `started` line |
| pick it up again | [`action resume`](../reference/cli/action.md#cdno-action-resume) | `resumed [[slug]] — text` |

[`cdno now`](../reference/cli/now.md) reads it back. Nothing is stored beside the daily log: the
focus is replayed from it each time, so there is nothing to keep in sync and a line you write by
hand counts as long as it has the writers' shape.

**One slot.** Starting something while something else is in focus is refused, naming the way forward
(`switch`, or `pause` first). You can always move on; you just say so, which makes the move a line in
the log instead of a silent change of subject. The same holds on reading: a newer start displaces an
older one for good, and finishing the newer one does not bring the older back.

**The window.** A start left open at midnight is still the focus in the morning, with its day named,
and expires after that. The window is [`carry_over_days`](../reference/configuration.md#focus), default
`1`; set it to `0` to read today only. `resume` re-anchors a carried focus to today, so work you keep
going on does not expire, and work you stop lets it lapse.

**Pause and resume.** `pause` is the honest way to stop without finishing: the action stays open on
the map, and an optional `--next` records where to pick up. Pauses are remembered for
[`paused_lookback_days`](../reference/configuration.md#focus), default 14, which covers a weekend. A
paused action stops being offered once something later started, resumed, finished, dropped or
promoted it. Promoting a bullet while it is in focus is fine: the focus follows it to the new note.

**With an assistant.** Over [MCP](../reference/mcp/writes.md#focus-tools) the focus rides along on
the reads and on most write results, so an agent knows what you are on without asking. The server
never refuses a write for being outside the focus. The agent's job is to notice a change of subject
and say it in one sentence, offering to capture the thought and stay, or to move over if you say so,
and to give you a cue to get back (`next:` is what it reads out). It does not ask you why.

**The `during:` tag.** While a focus is open, a log line written by `cdno log`, an inbox capture, and
a `## Notes` entry are tagged with its project: `during: [[surrogate-model]]` under the log line,
`captured_during: surrogate-model` in an inbox item. The tag is what lets a later review see where
captures came from, and it survives triage because the discard line copies it.

Next: [Configuration](configuration.md).
