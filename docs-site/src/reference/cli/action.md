# `cdno action`

Manage a project's next actions: add (optionally as a manifest note), promote a bullet to a note,
complete, and list.

```text
cdno action [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`add`](#cdno-action-add) | Append a next action to a project |
| [`promote`](#cdno-action-promote) | Promote a plain bullet to a wikilinked manifest note |
| [`complete`](#cdno-action-complete) | Mark an action done by substring match |
| [`list`](#cdno-action-list) | List a project's open actions |

Write subcommands honour `--json` (`{path, message}`, non-interactive); `list` emits its data under
`--json`.

---

## `cdno action add`

Append a next action to a project. `--note` also scaffolds a manifest note and wikilinks the bullet.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--title <TITLE>` | Action title. |
| `--energy <ENERGY>` | `deep`, `medium`, or `light`. |
| `--note` | Also create a manifest note alongside the bullet and wikilink it. |
| `--var <NAME=VALUE>` | Value for a custom action-note template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Only applies with `--note` (a plain bullet isn't templated). See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno action add --project surrogate-model --title "Profile the assembly step" --energy medium
cdno action add --project surrogate-model --title "Characterise sample efficiency" --energy deep --note
```

## `cdno action promote`

Promote an existing plain bullet to a wikilinked manifest note. Substring-matches the bullet text;
energy is inherited.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Case-insensitive substring of the bullet text. |

```bash
cdno action promote --project surrogate-model --query "profile the assembly"
```

## `cdno action complete`

Mark a next action completed by case-insensitive substring match. A wikilinked bullet also archives
its note to `actions/_done/<year>/`.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Substring of the bullet text. |

```bash
cdno action complete --project surrogate-model --query "feature set B"
```

## `cdno action list`

List a project's open action bullets, with attached-note status (active / blocked / completed) inline
when present. Honours `--json`.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |

```bash
cdno action list --project surrogate-model
cdno action list --project surrogate-model --json
```

## Related MCP tools

[`add_action`](../mcp/writes.md), [`promote_action`](../mcp/writes.md),
[`complete_action`](../mcp/writes.md). (Open actions are also visible via
[`get_project_context`](../mcp/reads.md).)

## See also

- [Actions](../../tutorials/actions.md).
- [`project`](project.md).
