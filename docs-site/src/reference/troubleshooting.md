# Troubleshooting

Common situations and how to resolve them.

## "not inside a Cuaderno vault"

A command can't find a vault. Cuaderno discovers one by walking up from the current directory for a
`.cuaderno/` folder, then falls back to `CUADERNO_VAULT_PATH`. Fix one of:

```bash
cd ~/notebook                      # run from inside the vault, or
cdno --vault ~/notebook <command>  # point at it explicitly, or
export CUADERNO_VAULT_PATH=~/notebook
```

See [Initialise a vault](../getting-started/initialise-a-vault.md).

## Search or links look out of date

The index is a cache; it's reconciled automatically each run, but a large external edit or a sync
conflict can occasionally confuse it. Rebuild it:

```bash
cdno reindex
```

The Markdown files are always authoritative, so a rebuild is safe. See
[Business rules](../concepts/business-rules.md).

## `lint` is failing

Run it to see the specifics:

```bash
cdno lint            # errors fail; warnings are listed but non-fatal
cdno lint --strict   # warnings fail too
```

Errors are usually an unknown `type:` or invalid frontmatter; warnings are typically broken
wikilinks. Fix the reported file, or run [`cdno normalise`](cli/normalise.md) if the issue is field
ordering. See [Frontmatter fields](frontmatter.md).

## A prompt appears when I wanted automation (or vice versa)

Write commands prompt for missing required flags **only** in an interactive terminal. In scripts,
pipes, or CI they error instead. Force non-interactive behaviour explicitly:

```bash
cdno project create --title "X" --context work --no-interactive
```

Conversely, if a command errors about a missing flag when you expected a prompt, your stdout probably
isn't a TTY (it's piped or redirected). See [CLI overview](cli/overview.md#interactive-vs-scripted).

## Can't create a sixth project

That's the [five-project cap](../concepts/business-rules.md#the-five-project-cap). Park an active
project first:

```bash
cdno project park --slug some-active-project
cdno project activate --slug the-one-you-want
```

(New projects created while at the cap are created **parked** rather than rejected.)

## `--json` output won't parse

`--json` is only honoured by read verbs and the write verbs that emit a result; maintenance and
interactive commands (`init`, `lint`, `reindex`, `normalise`, `triage`, `review`, `weekly`) ignore
it. Under `--json`, write verbs run non-interactively, so there are no prompts mixed into the output.
See [JSON output](json-output.md).

## Claude doesn't see the tools

For the MCP server (`cdno-mcp`):

- Make sure `cdno-mcp` is on the client's `PATH`, or use an absolute path in the config `command`.
- Set `CUADERNO_VAULT_PATH` (or rely on working-directory discovery).
- Restart the client after editing its MCP config.

See [Connect to Claude](../getting-started/connect-to-claude.md).
