# `cdno completions`

Print a shell-completion script. Source it in your shell's rc file. The script wires **vault-aware**
dynamic suggestions — `--project`, `--portfolio`, `--stewardship`, `--slug`, etc. are completed by
re-invoking the binary when you press TAB.

```text
cdno completions [OPTIONS] <SHELL>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<SHELL>` | Target shell. One of `bash`, `zsh`, `fish`, `elvish`, `powershell`. |

## Options

Only the [global options](overview.md#global-options).

## Setup per shell

```bash
# zsh — in ~/.zshrc:
source <(cdno completions zsh)

# bash — in ~/.bashrc:
source <(cdno completions bash)

# fish — write it into the completions dir:
cdno completions fish > ~/.config/fish/completions/cdno.fish

# elvish / powershell: emit the script and source it per that shell's convention.
cdno completions powershell
```

After reloading your shell, TAB completes commands, flags, and live vault values (project slugs,
portfolio slugs, …).

## What a Tab press reads

Completion finds the vault only by walking up from the current directory to the nearest
`.cuaderno/`. Unlike a command, it ignores a `--vault` on the line being completed and does not fall
back to `CUADERNO_VAULT_PATH`, so outside a vault the value completers offer nothing (the note-type
arguments offer the built-in types), even when the command itself would find a vault. Each
completer then reads the notes themselves:

| Completes | Offers |
|-----------|--------|
| `--project` on `action start` / `action switch`, `--slug` on `project park` | active project slugs |
| `--slug` on `project activate` | parked and closed project slugs, each labelled with where it is |
| every other `--project` / project `--slug` | active and parked project slugs |
| `--portfolio` | portfolio slugs |
| `--stewardship`, `--slug` on `stewardship show` | stewardship slugs |
| `--slug` on the `question` verbs, `--question` on `portfolio link` | question slugs |
| the note type on the `cdno templates` verbs, `--type` on `cdno search` | built-in and custom note types |
| `[REFERENCE]` on `cdno open` | every note's slug, plus `today`, `yesterday`, `tomorrow` |

A completion never touches the search index: it does not open, create or update
`.cuaderno/index.db` or its `-wal` and `-shm` files (nor an index that `CUADERNO_INDEX_PATH` moves
elsewhere), and does not take the write lock. It reads your config and the note files, and decides
which files are notes, and of which type, by the same rules reconciliation uses, so it offers what
an up-to-date index would list, including a note you added by hand since the last command. So
pressing Tab is safe beside a server that keeps the same vault's index open, and a vault with no
index yet still completes.

The price is that a Tab press reads every note in the vault in full, so its cost grows with the
vault: a few tens of milliseconds for a vault of thousands of notes.

## See also

- [Installation](../../getting-started/installation.md#shell-completions-optional).
