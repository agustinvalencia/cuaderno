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

## See also

- [Installation](../../getting-started/installation.md#shell-completions-optional).
