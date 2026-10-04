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

## `cdno now` says nothing is started, but I started something

Three causes, and [`cdno lint`](cli/lint.md) tells the first apart from the rest.

If you wrote the log line **by hand**, it is almost certainly not in the shape the readers accept.
It has to be exactly:

```text
- **09:30**: started [[surrogate-model]] — Draft the methods section (deep)
```

The `- ` bullet and the `**HH:MM**: ` stamp are what make it a log entry at all, and the separator
must be a real em dash (U+2014), not a hyphen. A near-miss is skipped in silence by design — prose
beginning "started something" must never register as a focus — so `cdno lint` reports it instead,
naming the line and the likely cause. Run it and fix what it points at. The same goes for `paused`,
`resumed` and `action promoted on` lines.

Otherwise, check the date. The focus is read from today's daily note and the
`[focus] carry_over_days` days before it (default 1, so yesterday's too); a start logged earlier than
that and never closed has expired. Pick the work up with
[`cdno action resume`](cli/action.md#cdno-action-resume) (it also finds a recent pause), or start it
again.

The third cause surprises people who remember the older behaviour: **focus is one slot**. If you
started X, then started Y, and finished Y, nothing is in focus. X was displaced the moment Y began,
and closing Y does not bring it back, because nobody chose to go back to it. Start X again, or, if
you paused it along the way, `cdno action resume`. Logs written before the slot rule read the same
way, so a day that used to report X as the focus now reports none.

## `cdno now` says I'm still on yesterday's thing

The focus survives midnight on purpose: a start left open yesterday is still the focus this morning,
with the day named (`since Saturday 14:05`). That is the one-day window,
[`carry_over_days`](configuration.md#focus). You have three ways out:

- It is still what you are doing: `cdno action resume` re-anchors it to today (`picked up 08:50
  today (started Saturday 14:05)`) and keeps it from expiring tomorrow.
- You stopped but will come back: `cdno action pause --next "…"`, and `resume` brings the hint back.
- You do not want anything to carry overnight: set `carry_over_days = 0` in `.cuaderno/config.toml`
  and the focus is read from today's note only.

Finishing it with `cdno action complete` or `drop` clears it as ever.

## `action start` refuses with `focus_open`

Something is already in focus, and starting a second thing would leave the log silent about the
move. The message names what is open and the command to run:

```text
Error: Draft the methods section is already in focus on surrogate-model (since 09:30).
To move on to this instead: cdno action switch --project other-project --query CI
Or complete or pause it first.
```

`cdno action switch` pauses what is open and starts the new action in one step; `cdno action pause`
then `start` does the same in two. If the open focus is the action you are trying to start, `start`
says so: nothing to do when it was started today, and `cdno action resume` when it was carried over
from an earlier day.

This is most often noticed the first morning after upgrading. A start left open yesterday and never
closed is now carried into today, so the first `start` of the day is refused where it used to stack
silently. Resume it, pause it, complete it, or switch. If you would rather keep the old behaviour,
set `carry_over_days = 0`.

Over MCP the refusal is a `focus_open` rejection with a `remedy`; see
[Write tools](mcp/writes.md#rejections-focus_open-and-no_focus).

## A prompt appears when I wanted automation (or vice versa)

Write commands prompt for missing required flags **only** in an interactive terminal. In scripts,
pipes, or CI they error instead. Force non-interactive behaviour explicitly:

```bash
cdno project create --title "X" --context work --no-interactive
```

Conversely, if a command errors about a missing flag when you expected a prompt, your stdout probably
isn't a TTY (it's piped or redirected). See [CLI overview](cli/overview.md#interactive-vs-scripted).

## Can't create a sixth project

That's the [five-project cap](../concepts/business-rules.md#the-five-project-cap). Free a slot first:
park an active project, or close one that has finished or is not going to happen.

```bash
cdno project park --slug some-active-project       # or: project complete / project drop
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
