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
| `note-type install` | Install a note type that ships with `cdno` (such as `concept`) into this vault. See [Installing a bundled note type](#installing-a-bundled-note-type). |
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

## Installing a bundled note type

`cdno` ships with note types that `cdno init` declares in every new vault. Today there is one,
[`concept`](../../concepts/concept-library.md). `note-type install` adds one to a vault created
before it existed, so the older vault ends up exactly as a new one would:

```bash
cdno config note-type install --list            # what ships, and whether this vault has it
cdno config note-type install --name concept --dry-run
cdno config note-type install --name concept
```

It takes three steps, in this order, and each writes only what is absent:

1. **Template.** Creates `.cuaderno/templates/` if needed and writes the type's template, but
   only when no file of that name exists. A template you have edited is never overwritten; the
   report says whether it matches the bundled one. If your declaration names a different template
   file, the bundled one is not installed.
2. **Folder.** Creates the type's folder (`concepts/`) if absent.
3. **Declaration.** Appends the `[note_types.<name>]` block, comment included, through the same
   save gate as every other verb. If the config already declares the type, the declaration is
   never modified: it is compared with the bundled one, key by key, and the report names the
   `note-type set` command that would adopt each bundled value you might want.

```text
declaration  written
template     written .cuaderno/templates/concept.md
folder       created concepts/
```

Re-running is safe. When there is nothing left to write, the report ends with
`concept: already installed, nothing to do` and the command exits 0.

A config the vault would not open (check it with `cdno config validate`) is refused before anything
is written, even when the type is already declared.

Two refusals are phrased in plain words, and in both nothing is left behind:

- a config that declares `note_types` as an inline table cannot take an appended block; add it with
  `cdno config edit`;
- another custom type already using the folder (`concepts`) is refused by the gate.

`--list` prints each bundled type's purpose, folder, fields, template and its section headings,
with this vault's state: `not installed`, `installed (matches)`, `installed (declaration differs)`,
`installed (template customised)` or `installed (template missing)`. `--dry-run` prints the exact
block and template that would be written, checks the result would validate, and writes nothing.
With `--json`, the report is one object with `changed`, as the other verbs carry it:

```json
{"changed": true, "note_type": "concept",
 "declaration": "written",
 "template": {"path": "templates/concept.md", "action": "written"},
 "folder": {"path": "concepts", "action": "created"}}
```

Once installed, a bundled type is an ordinary custom declaration: edit it with `note-type set`, or
delete it with `note-type remove`.

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

# Add the concept library to a vault created before it existed.
cdno config note-type install --name concept

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
