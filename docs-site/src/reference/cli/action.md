# `cdno action`

Manage a project's next actions: add (optionally as a manifest note), promote a bullet to a note,
complete, drop, and list.

```text
cdno action [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`add`](#cdno-action-add) | Append a next action to a project |
| [`promote`](#cdno-action-promote) | Promote a plain bullet to a wikilinked manifest note |
| [`complete`](#cdno-action-complete) | Mark an action done by substring match |
| [`drop`](#cdno-action-drop) | Close an action **without** recording it as done |
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
| `--var <NAME=VALUE>` | Value for a custom action-note template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Promotion scaffolds an action note, so it gathers the same prompts as `add --note`. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

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

## `cdno action drop`

Close a next action **without recording it as done** — for work that was superseded, abandoned or
reprioritised. Matches the bullet exactly as `complete` does; a wikilinked bullet also archives its
note to `actions/_done/<year>/`, stamped `status: dropped` with no completion date.

| Flag | Description |
|------|-------------|
| `--project <SLUG>` | Project slug. |
| `--query <QUERY>` | Substring of the bullet text. |
| `--reason <TEXT>` | Optional: why it was dropped. |

```bash
cdno action drop --project surrogate-model --query "demo proposal" \
    --reason "superseded by the demo-planning action"
```

Use this rather than `complete` whenever the work was not actually performed. `complete` writes
`action done on [[…]]` into the daily log, which is the record the weekly review, the monthly scan
and every later verdict read back from — so completing something that never happened leaves the
vault asserting work nobody did, and the only repair is a correction line written by hand.

`--reason` is optional but worth giving: "superseded by X" and "no longer wanted" are different
facts, and only one of them tells a later reader to go looking for the replacement. It is recorded
on a continuation line under the log entry.

A dropped action does **not** appear in the completed-actions views that the weekly and monthly
reviews build, because it carries no completion date.

## `cdno action list`

List a project's open action bullets, with attached-note status (active / blocked / completed / dropped) inline
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
[`complete_action`](../mcp/writes.md), [`drop_action`](../mcp/writes.md). (Open actions are also visible via
[`get_project_context`](../mcp/reads.md).)

## See also

- [Actions](../../tutorials/actions.md).
- [`project`](project.md).
