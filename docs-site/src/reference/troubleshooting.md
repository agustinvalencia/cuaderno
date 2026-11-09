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

## My `.typ` file is not in the index

A `.typ` file is a note only when all three hold: it lies in the folder of a type that admits
Typst (`portfolios/<slug>/` for evidence, `concepts/` for a concept, and only if `[typst]
note_types` lists the type); it is not inside another note's assets folder (`<stem>/` beside
`<stem>.typ`); and it carries a `#metadata((…)) <cdno>` block. [`cdno lint`](cli/lint.md#typst-notes)
says which one fails:

```text
[warning] portfolios/surrogate-error-budget/loose.typ: Typst file is neither a note nor an attachment: it has no `<cdno>` block and no note owns its folder; add a block to make it a note, or move it into the assets folder of the note that uses it
[error] portfolios/surrogate-error-budget/typo.typ: metadata present but the `<cdno>` label is missing, mistyped or nested (line 1); put `<cdno>` directly after the `#metadata((…))` call
[warning] portfolios/surrogate-error-budget/2026-11-09-review-deck/inner.md: this Markdown note sits in the assets folder of Typst note `portfolios/surrogate-error-budget/2026-11-09-review-deck.typ`, so it is not indexed; move it out of `portfolios/surrogate-error-budget/2026-11-09-review-deck/`
```

A labelled `.typ` owns the folder `<stem>/` beside it, so a Markdown note placed there is not
indexed (the third row above). The rows apply only where the file is seen: "labelled Typst file
outside the configured types" fires for a labelled `.typ` in the folder of a type that `[typst]
note_types` leaves out, and "neither a note nor an attachment" for an unlabelled `.typ` in a folder
that admits Typst. A labelled `.typ` anywhere else (outside every admitting folder, say in
`journal/`, or inside another note's assets folder) gets **no row**: it is silently an attachment, so
if a note you expected is missing and lint is quiet, check where the file sits. To write a note
rather than attach a file, see [Typst notes](typst-notes.md).

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
  and the focus is read from today's note only. A running MCP server keeps the config it started
  with, so restart it (reconnect the client) for an agent to see the change.

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
set `carry_over_days = 0` (and restart a running MCP server, which keeps the config it started with).

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

That's the [five-project cap](../concepts/business-rules.md#the-five-project-cap): five projects
count, that is, are active and name no one else as their holder. Free a slot first: park an active
project, or close one that has finished or is not going to happen.

```bash
cdno project park --slug some-active-project       # or: project complete / project drop
cdno project activate --slug the-one-you-want
```

(New projects created while at the cap are created **parked** rather than rejected.)

## My handed-off project still counts

A project you handed off is counted again when its `held_by:` does not read as a name. Run
`cdno project show <slug>`: it ends with `Counts toward the cap: yes` or `no`, and prints
`Held by: <name>` only when the holder is honoured. `cdno lint` names the cause. The three usual
ones:

- **A sentinel.** The value names you or nobody: `me`, `myself`, `(mine)`, `someone`, `tbd`, `?`,
  … A map with such a value is yours, so it counts, and lint warns. Name who really has it with
  `cdno project hand-off --slug <slug> --to "<name>"`. The list is the vault's
  [`[projects] holder_sentinels`](configuration.md#projects); if the holder's real name is on it
  (added when you replaced the default list), take it off.
- **A placeholder.** A blank `held_by:` names nobody under any list, whatever the config says.
  Fill it in, or delete the line.
- **An unquoted wikilink.** `held_by: [[people/ana]]` is a YAML list, not a name, so the holder is
  not honoured and lint reports an error. Quote it: `held_by: "[[people/ana]]"`. The same goes for
  a number or a date.

If you cannot name who has the project, it is yours, and counting it is right. Handing it off is
not a way to make room at the cap: park, complete or drop a project instead.

The reverse also happens: `held_by: "<your own name>"` does **not** count, because cdno cannot know
your name. Take the project back (`cdno project take-back --slug <slug>`), and add your name to
`holder_sentinels` if you want it refused in future.

## Writes are refused with a method version error

The vault records the version of the Research Logbook Method it follows (`[vault] method_version`,
see [configuration](configuration.md#method-version)), and every write checks it under the write
lock. Reads keep working; only writes are refused, when the major version differs:

- **"…an older major version than the RLM … this cdno implements"**: the vault needs migrating.
  Run `cdno migrate --dry-run` to see what changes, then
  [`cdno migrate`](cli/migrate.md). Migration is CLI-only, so an MCP client hitting this error
  needs you to run it in a terminal.
- **"…a newer major version than the RLM … this cdno implements — upgrade cdno to write to it"**:
  another machine, or a newer cdno, migrated the vault. Upgrade this cdno (and restart any MCP
  server running the old binary).

A **minor** difference never refuses a write; it prints a one-line notice on stderr instead. Do not
edit `method_version` by hand to silence either error: the stamp goes with the migration's log
entry and any file steps.

## A link to an old name still works in cdno but not in my editor

After [`cdno note rename`](cli/note.md#cdno-note-rename-note) every link in the vault is rewritten to
the new name, and the old name is kept on the note as an alias so cdno keeps resolving it. A link
that was written with the old name afterwards (typed from memory, or brought back by a sync merge
from a machine that had not seen the rename) still works in cdno, but an editor such as Obsidian
or Neovim follows the file name, not the alias, and cannot open it.

`cdno lint` finds them: ``[[old]] names a renamed note — now `new` ``. Leaving them is harmless,
since they still work in cdno. To rewrite every one to the current name in one go, run
[`cdno note rename new --relink`](cli/note.md#relinking-the-stragglers---relink) (`--dry-run` first
to see them; over MCP, `rename_note` with `relink: true`): nothing moves, the old name stays listed,
and a `relinked` line is logged. This works for an archived action note too, which cannot be renamed.
A union merge brings back the whole hunk, so the lines it duplicated stay after the relink, as
identical text twice: the relink corrects addresses, it does not tidy the merge. Changing one link
by hand is fine too, even in a past log entry: it is the same address correction.

## Rename refused: behind upstream

```text
Error: the vault is 2 commits behind its upstream; sync the vault first (pull the upstream's commits, e.g. with `git pull`), or pass `--skip-sync-check` to rename anyway
```

`cdno note rename` will not rewrite many notes in a vault whose other clone has already changed them.
Sync the vault (`git pull`, or your sync tool) and run the rename again. If you are sure nothing
unpushed on the other side touches these notes, pass `--skip-sync-check` (over MCP, `skip_sync_check`
is for the person to decide, not the agent). `cdno note rename --dry-run --skip-sync-check` shows the
plan of a vault that is behind without writing.

If the message is instead `a git operation is in progress in the vault (…); finish or abort it, then
rename` (code `git_operation_in_progress` with `--json`; text mode prints no code), a merge, rebase
or cherry-pick was left paused in the vault. Finish or abort it (`git status` says which); `--skip-sync-check` does not
override this one.

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
