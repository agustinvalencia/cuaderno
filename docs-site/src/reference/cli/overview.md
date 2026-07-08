# CLI reference

`cdno` is the command-line interface to a Cuaderno vault. This section documents every command. For
the *why* behind them, see [Concepts](../../concepts/rlm.md) and [Tutorials](../../tutorials/daily-loop.md).

```text
cdno [OPTIONS] <COMMAND>
```

Run `cdno --help` for the command list, or `cdno <command> --help` for any command's flags.

## Global options

These apply to every command:

| Flag | Effect |
|------|--------|
| `--vault <PATH>` | Operate on the vault at `PATH`. Overrides both discovery and `CUADERNO_VAULT_PATH`. |
| `--json` | Emit machine-readable JSON instead of a formatted table (see below). |
| `--no-interactive` | Never prompt; a missing required argument is an error. Implicit when stdout isn't a TTY. |
| `-h, --help` | Print help. |
| `-V, --version` | Print the version (top level only). |

## Finding the vault

When `--vault` is not given, `cdno` discovers the vault by walking **up** from the current directory
until it finds a `.cuaderno/` folder. If that finds nothing, it falls back to the
`CUADERNO_VAULT_PATH` environment variable. Standing inside a vault always wins over the env var.
See [Initialise a vault](../../getting-started/initialise-a-vault.md).

## Interactive vs. scripted

Write commands follow one convention (the "flags-and-prompts" pattern):

- **Interactive terminal, missing a required flag** → `cdno` prompts for it, then asks you to confirm
  before writing.
- **Non-interactive** (piped, redirected, in CI, or `--no-interactive`) → a missing required flag is
  an error. Supply every flag and the command runs unattended.

This means the same command serves a human at a prompt and a script with no changes.

## JSON output

`--json` makes any supported verb emit structured output:

- **Read verbs** (`commitments`, `questions`, `status`, `orient`, `search`, and the `list`/`show`
  verbs of `project`/`portfolio`/`stewardship`, plus `action list`) emit their listing or detail
  object.
- **Write verbs** (`log`, `capture`, `file`, `track`, and the create/update verbs of `project`,
  `action`, `portfolio`, `stewardship`, `question`, `commit`) emit a `{ "path": ..., "message": ... }`
  result, and **run non-interactively** (so prompts can't corrupt the JSON on a terminal).
- **Maintenance / interactive / bootstrap commands** (`init`, `lint`, `reindex`, `normalise`,
  `triage`, `review`, `weekly`, `monthly`) ignore `--json`.

The CLI's JSON shapes match the [MCP server](../mcp/overview.md) DTOs. See
[JSON output](../json-output.md) for every shape.

## The commands

| Command | What it does |
|---------|--------------|
| [`init`](init.md) | Create a new vault |
| [`log`](log.md) | Append a line to today's daily note |
| [`capture`](capture.md) | Drop a quick note into the inbox |
| [`triage`](triage.md) | Process inbox captures |
| [`orient`](orient.md) | Morning orientation |
| [`status`](status.md) | Active projects + top actions |
| [`weekly`](weekly.md) | Show the weekly note |
| [`monthly`](monthly.md) | Show the monthly note |
| [`commitments`](commitments.md) | Aggregated deadlines |
| [`questions`](questions.md) | List active questions |
| [`search`](search.md) | Full-text search |
| [`review`](review.md) | Guided weekly/monthly review |
| [`project`](project.md) | Manage project maps |
| [`action`](action.md) | Manage next actions |
| [`portfolio`](portfolio.md) | Manage evidence portfolios |
| [`file`](file.md) | File evidence into a portfolio |
| [`question`](question.md) | Manage question notes |
| [`stewardship`](stewardship.md) | Manage stewardships |
| [`track`](track.md) | File a tracking entry |
| [`commit`](commit.md) | Manage standalone commitments |
| [`lint`](lint.md) | Validate the vault |
| [`reindex`](reindex.md) | Rebuild the index |
| [`normalise`](normalise.md) | Reorder frontmatter |
| [`completions`](completions.md) | Shell-completion scripts |
