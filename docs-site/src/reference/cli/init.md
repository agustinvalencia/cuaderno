# `cdno init`

Create a new vault: the folder tree, a default `.cuaderno/config.toml` (which declares the `concept` custom type), and starter `daily.md` and `concept.md` templates (every other built-in type uses an in-binary default until you [eject](templates.md#cdno-templates-eject-type) one).

```text
cdno init [OPTIONS] [PATH]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[PATH]` | Target directory. Defaults to `--vault` when that is given, otherwise to the current working directory. |

## Options

Only the [global options](overview.md#global-options) apply. `init` ignores `--json`.

`--vault <PATH>` names the directory to create the vault in, exactly as `[PATH]` does, so a script
that passes `--vault` to every command creates the vault where it says. Give both only if they name
the same directory: when they differ, `init` refuses and writes nothing.

`init` never looks for an existing vault: unlike every other command it does not walk up from the
current directory, and it ignores `CUADERNO_VAULT_PATH`. Only `[PATH]` and `--vault` move it away
from the current directory.

## Examples

```bash
# Create a vault in a new directory:
cdno init ~/notebook

# Initialise the current directory as a vault:
cdno init

# The same as the first, as a script that passes --vault everywhere writes it:
cdno --vault ~/notebook init
```

It fails if the target already contains a `.cuaderno/` directory (it won't clobber an existing
vault).

## See also

- [Initialise a vault](../../getting-started/initialise-a-vault.md) — what the tree contains and how
  discovery works.
- [Vault structure](../../concepts/vault-structure.md).
