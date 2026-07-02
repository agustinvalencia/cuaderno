# `cdno init`

Create a new vault: the folder tree, a default `.cuaderno/config.toml`, and a starter `daily.md` template (every other type uses an in-binary default until you [eject](templates.md#cdno-templates-eject-type) one).

```text
cdno init [OPTIONS] [PATH]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[PATH]` | Target directory. Defaults to the current working directory. |

## Options

Only the [global options](overview.md#global-options) apply. `init` ignores `--json`.

## Examples

```bash
# Create a vault in a new directory:
cdno init ~/notebook

# Initialise the current directory as a vault:
cdno init
```

It fails if the target already contains a `.cuaderno/` directory (it won't clobber an existing
vault).

## See also

- [Initialise a vault](../../getting-started/initialise-a-vault.md) — what the tree contains and how
  discovery works.
- [Vault structure](../../concepts/vault-structure.md).
