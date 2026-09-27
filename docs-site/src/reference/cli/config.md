# `cdno config`

Inspect, check and edit `.cuaderno/config.toml` — the vault's configuration file.

```text
cdno config <COMMAND> [OPTIONS]
```

These verbs deliberately **do not open the vault**. Opening one validates the config first, so a
config that will not parse means no vault at all — and that is exactly when you need to read, check
and fix it. Every other command would fail with a bare "loading config.toml"; these report the line,
the column and the reason.

## Commands

| Command | What it does |
|---|---|
| `show` | Print the config verbatim — comments, key order and spacing exactly as on disk. `--json` adds the content hash. |
| `validate` | Run the exact check `Vault::new` performs. Exits non-zero on a bad config. `--file <PATH>` checks a candidate without putting it in place. |
| `edit` | Open the config in `$EDITOR`, then save it through the validate-first, compare-and-swap gate. |
| `note-type set\|remove` | Add, change or remove a custom note type (`[note_types.<name>]`). |
| `field set\|remove` | Add, change or remove a schema field (`[schemas.<type>.fields.<field>]`). |
| `plot set` | Set how a declared tracking metric is plotted. |
| `var set\|remove` | Add, change or remove a static template variable (`[variables]`). |
| `prompt set\|remove` | Add, change or remove a prompted template variable (`[variables.prompt]`). |

## The save gate

Every write — `edit` and all the structured setters alike — goes through the same three steps:

1. **Validate first.** A config that would not reopen is never written.
2. **Compare and swap.** If the file changed on disk since it was read, the save is refused rather
   than clobbering the other edit.
3. **Write verbatim.** Comments and key order survive.

So a refused save always leaves the file byte-identical. `edit` additionally keeps your rejected
buffer and prints its path — a refused save must not also lose the work.

`edit` round-trips a scratch copy rather than opening `config.toml` itself: handing an editor the
real file would route the write around all three steps. It needs a terminal, and refuses an editor
that detaches instead of waiting, because there is then no moment at which the buffer is known to be
written.

## Setting a value merges, it does not replace

`note-type set` and `field set` change only the flags you pass. An omitted flag keeps its current
value, so a one-flag edit cannot silently drop the rest of the entry.

To clear something, pass it empty (`--template ''`, `--required ''`), or use the paired negative flag
for a boolean (`--no-append-only`, `--no-settable`, `--no-log-on-change`, `--no-required`).

Changing a field's `--type` is the one exception: `default` and `values` are declared against the old
type, so they are dropped and the command says so. Re-set them in the same command or afterwards.

## Options

Promptable arguments are flags. Omit one in a terminal and you are asked for it; omit one with
`--no-interactive` (or off a TTY) and you get a named error rather than a usage dump.

Common to every verb: the [global options](overview.md#global-options).

## Examples

```bash
cdno config show                       # verbatim, pipeable back into the file
cdno config validate                   # exit 0 iff the vault would open
cdno config validate --file draft.toml # check a candidate first
cdno config edit                       # $EDITOR round trip through the gate

# Declare a custom note type, then extend it.
cdno config note-type set --name people --folder people --required name,email
cdno config note-type set --name people --folder humans          # keeps required + template
cdno config field set --note-type people --field email --type string --required

# Template variables.
cdno config var set --name author --value "Your Name"
cdno config prompt set --name subject --message "What is this note about?"

# Plot a declared tracking metric.
cdno config plot set --activity gym --metric weight --plot line
```

## See also

- [Configuration reference](../configuration.md) — every key the file accepts.
- [Custom note types](../custom-note-types.md).
- [`templates`](templates.md) — the templates those note types render through.
