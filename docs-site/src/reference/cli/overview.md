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
| `--no-interactive` | Never prompt; a missing required argument is an error. Implicit unless both stdin and stdout are TTYs. Also turns off interactive reports. |
| `--color <WHEN>` | `auto` (default), `always`, or `never`. See [Colour and interactivity](../colour-and-interactivity.md). Spelled `--colour` too. |
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
- **Non-interactive** (either stream not a terminal — piped, redirected, `< /dev/null`, or running
  in CI, which is the usual reason neither is — or `--no-interactive`) → a missing required flag is
  an error. Supply every flag and the command runs unattended.

This means the same command serves a human at a prompt and a script with no changes.

Read commands add one thing on top. In a terminal, several listings offer to open one of the rows
they just printed — `cdno project list` asks which project you want to see, prints it, and asks
again until you press Esc. The listing itself is printed first either way, so piping, `--no-interactive`,
and `--json` all behave exactly as they did before. See
[Colour and interactivity](../colour-and-interactivity.md).

## JSON output

`--json` makes any supported verb emit structured output:

- **Read verbs** (`commitments`, `questions`, `status`, `now`, `orient`, `search`, `open`, and the `list`/`show`
  verbs of `project`/`portfolio`/`stewardship`, plus `action list`) emit their listing or detail
  object.
- **Write verbs** (`log`, `capture`, `file`, `track`, and the create/update verbs of `project`,
  `action`, `portfolio`, `stewardship`, `question`, `commit`) emit a `{ "path": ..., "message": ... }`
  result, and **run non-interactively** (so prompts can't corrupt the JSON on a terminal).
  `project complete` and `drop` add what was let go, and print a refusal object and exit `1` while
  items are open (see [JSON output](../json-output.md)).
- **`migrate`** emits its report, the plan with `--dry-run` (see [`migrate`](migrate.md)).
- **Maintenance / interactive / bootstrap commands** (`init`, `lint`, `reindex`, `normalise`,
  `triage`, `review`, `weekly`, `monthly`) ignore `--json`.

The CLI's JSON shapes match the [MCP server](../mcp/overview.md) DTOs. See
[JSON output](../json-output.md) for every shape.

## Naming a note: `--slug`

Every verb that creates a note derives its filename, the slug, from the title. For a long title the
derived slug can stop mid-thought (`rerun-the-surrogate-model-ablation-on`), and it is what every later
`[[wikilink]]` has to type. The create verbs therefore take an optional `--slug <SLUG>` to name the
file yourself: [`project create`](project.md#cdno-project-create),
[`question create`](question.md#cdno-question-create),
[`portfolio create`](portfolio.md#cdno-portfolio-create), [`action add --note`](action.md#cdno-action-add),
[`action promote`](action.md#cdno-action-promote), [`note create`](note.md),
[`stewardship create`](stewardship.md#cdno-stewardship-create) and
[`commit create`](commit.md#cdno-commit-create). The title is unchanged; only the filename is.

An explicit slug must be:

- lowercase `a-z`, `0-9` and single `-`, with no `-` at either end;
- at most 50 characters;
- not something YAML would read as a number, a boolean, a null or a date (`2026`, `yes`, `null`,
  `2026-11-05`): add a word to it.

An explicit slug is used exactly or refused, never rewritten. If another note already holds the name
(as its filename, as the folder of a folder note such as `portfolios/surrogate-model/`, or as a
declared alias) the create is refused and nothing is written; pick another name. The `-2` suffix
belongs to derived slugs only. `--slug` is never prompted for, so omit it and the behaviour is exactly
as before. Under [`--json`](../json-output.md#slug-refusals) a refusal is a coded object.

A derived slug also steps past a folder note's name now: a question titled "Surrogate model" created
beside the portfolio `portfolios/surrogate-model/` gets `surrogate-model-2`, since the bare name would
shadow the portfolio's `_index` in a `[[surrogate-model]]` link.

```bash
cdno action add --project surrogate-model --note --energy deep \
  --title "Rerun the surrogate model ablation on the held-out split with feature set B" \
  --slug feature-set-b-ablation
# -> actions/feature-set-b-ablation.md
```

Renaming a note after the fact is a separate verb, planned under RFC 0007.

## The commands

| Command | What it does |
|---------|--------------|
| [`init`](init.md) | Create a new vault |
| [`log`](log.md) | Append a line to today's daily note, or (`log note`) a worked-out entry under `## Notes` |
| [`capture`](capture.md) | Drop a quick note into the inbox |
| [`triage`](triage.md) | Process inbox captures |
| [`orient`](orient.md) | Morning orientation |
| [`status`](status.md) | Active projects + top actions |
| [`now`](now.md) | What you are in the middle of |
| [`open`](open.md) | Resolve a note reference to its path |
| [`weekly`](weekly.md) | Show the weekly note |
| [`monthly`](monthly.md) | Show the monthly note |
| [`commitments`](commitments.md) | Aggregated deadlines |
| [`questions`](questions.md) | List active questions |
| [`search`](search.md) | Full-text search over titles and bodies |
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
| [`migrate`](migrate.md) | Record this cdno's method version in the vault |
| [`normalise`](normalise.md) | Reorder frontmatter |
| [`completions`](completions.md) | Shell-completion scripts |
