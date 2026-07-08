# `cdno review`

Guided review rituals.

```text
cdno review [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`weekly`](#cdno-review-weekly) | Walk the retrospective sections into this week's note and set next week's goal. |
| [`monthly`](#cdno-review-monthly) | Walk the monthly sections into this month's note. |

`review` ignores `--json`.

---

## `cdno review weekly`

Walk the retrospective sections (Wins, Challenges, One Improvement) into **this** week's note, then
set next week's goal as the **This Week's Goal** of **next** week's note. When run non-interactively,
it reads the current note instead of prompting.

```text
cdno review weekly [OPTIONS]
```

### Options

Only the [global options](overview.md#global-options).

### Examples

```bash
cdno review weekly                  # interactive: prompts for each section, sets next week's goal
cdno review weekly --no-interactive # read the current weekly note without prompting
```

---

## `cdno review monthly`

Walk the monthly sections (Wins, Themes, Next Month's Focus) into **this** month's note. Unlike the
weekly ritual there is no cross-note carry-forward — every section lands in the same month's note.
When run non-interactively, it reads the current note instead of prompting.

```text
cdno review monthly [OPTIONS]
```

### Options

Only the [global options](overview.md#global-options).

### Examples

```bash
cdno review monthly                  # interactive: prompts for each section
cdno review monthly --no-interactive # read the current monthly note without prompting
```

## Related MCP tools

[`get_weekly_context`](../mcp/reads.md), [`read_weekly_note`](../mcp/reads.md),
[`upsert_weekly_section`](../mcp/writes.md), [`read_monthly_note`](../mcp/reads.md),
[`upsert_monthly_section`](../mcp/writes.md).

## See also

- [Weekly review](../../tutorials/weekly-review.md).
- [Note types](../../concepts/note-types.md).
- [`weekly`](weekly.md) / [`monthly`](monthly.md) — just view the notes.
