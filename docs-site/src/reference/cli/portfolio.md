# `cdno portfolio`

Manage evidence portfolios: create, list, show, and link to a question or project. Filing evidence
*into* a portfolio is the separate [`cdno file`](file.md) verb.

```text
cdno portfolio [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-portfolio-create) | Create a portfolio under `portfolios/<slug>/` |
| [`list`](#cdno-portfolio-list) | List portfolios with evidence counts + staleness |
| [`show`](#cdno-portfolio-show) | Show a portfolio's frontmatter + evidence inventory |
| [`link`](#cdno-portfolio-link) | Link an existing portfolio to a question or project |

`create`/`link` honour `--json` (`{path, message}`); `list`/`show` emit their data under `--json`.

---

## `cdno portfolio create`

Create a new portfolio. The slug derives from the question.

| Flag | Description |
|------|-------------|
| `--question <QUESTION>` | The question this dossier accumulates evidence for. |
| `--project <PROJECT>` | Optional wikilink to a project to associate it with. |
| `--var <NAME=VALUE>` | Value for a custom template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno portfolio create --question "Sparse vs dense attention OOD"
cdno portfolio create --question "Sparse vs dense attention OOD" --project projects/surrogate-model
```

## `cdno portfolio list`

List every portfolio with its evidence count and staleness. Honours `--json`.

```bash
cdno portfolio list
cdno portfolio list --json | jq '.[] | {slug, evidence_count}'
```

## `cdno portfolio show`

Show a portfolio's frontmatter and its evidence inventory. Honours `--json` (a detail object with the
evidence list).

| Flag | Description |
|------|-------------|
| `--portfolio <PORTFOLIO>` | Portfolio slug. |

```bash
cdno portfolio show --portfolio sparse-vs-dense-attention-ood
cdno portfolio show --portfolio sparse-vs-dense-attention-ood --json
```

## `cdno portfolio link`

Link an existing portfolio to an existing question **or** project (the retrofit path — pass exactly
one of `--question`/`--project`). Backlinks are set on both sides.

| Flag | Description |
|------|-------------|
| `--portfolio <PORTFOLIO>` | Portfolio slug. |
| `--question <QUESTION>` | Question to link (mutually exclusive with `--project`). |
| `--project <PROJECT>` | Project wikilink to link (mutually exclusive with `--question`). |

```bash
cdno portfolio link --portfolio sparse-vs-dense-attention-ood --project projects/surrogate-model
cdno portfolio link --portfolio sparse-vs-dense-attention-ood --question questions/research/surrogate-cost
```

## Related MCP tools

[`create_portfolio`](../mcp/creation-and-lifecycle.md), [`get_portfolio_contents`](../mcp/reads.md)
(show), [`link_portfolio_to_question`](../mcp/creation-and-lifecycle.md),
[`link_portfolio_to_project`](../mcp/creation-and-lifecycle.md).

## See also

- [Research and evidence](../../tutorials/research-and-evidence.md).
- [`file`](file.md) — add evidence to a portfolio.
