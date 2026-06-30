# `cdno question`

Manage question notes: create one, then transition its status. Each status transition is logged to
today's daily note. To *list* active questions, use [`cdno questions`](questions.md).

```text
cdno question [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-question-create) | Create a new question note |
| `park` | Set status to `parked` |
| `answer` | Set status to `answered` |
| `retire` | Set status to `retired` |
| `activate` | Set status to `active` |

All honour `--json` (`{path, message}`, non-interactive).

---

## `cdno question create`

Create a new question under `questions/<domain>/<slug>.md`. The slug derives from the text.

| Flag | Description |
|------|-------------|
| `--domain <DOMAIN>` | `research` or `life`. |
| `--text <TEXT>` | The question text (becomes the body H1). |
| `--var <NAME=VALUE>` | Value for a custom template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno question create --domain research --text "Does sparse attention beat dense OOD?"
```

## Status transitions

`park`, `answer`, `retire`, and `activate` each take a `--slug`. The interactive picker offers only
eligible questions for that transition (e.g. you can only `park` an active question).

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Question slug. |

```bash
cdno question park   --slug does-sparse-attention-beat-dense-ood
cdno question answer --slug does-sparse-attention-beat-dense-ood
cdno question retire --slug does-sparse-attention-beat-dense-ood
cdno question activate --slug does-sparse-attention-beat-dense-ood
```

## Related MCP tools

[`create_question`](../mcp/creation-and-lifecycle.md),
[`set_question_status`](../mcp/creation-and-lifecycle.md). (List via
[`get_active_questions`](../mcp/reads.md).)

## See also

- [Research and evidence](../../tutorials/research-and-evidence.md).
- [`questions`](questions.md) — list active questions.
