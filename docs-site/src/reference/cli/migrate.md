# `cdno migrate`

Record the version of the Research Logbook Method this cdno implements in the vault
(`[vault] method_version` in `.cuaderno/config.toml`), running any file migrations that version
needs and logging the move in today's daily note.

```text
cdno migrate [OPTIONS]
```

## Options

| Flag | Meaning |
|---|---|
| `--dry-run` | Print what would change, and write nothing. |

Plus the [global options](overview.md#global-options). `--json` prints the same report as an
object: `dry_run`, `outcome` (`planned`, `migrated` or `nothing_to_do`), `vault` (`null` when
unstamped), `vault_described`, `binary`, `migrations`, `config_line`, `log_entry` and
`read_through`.

`migrate` asks nothing and confirms nothing: run it with `--dry-run` first to see exactly what
will change.

## What it does

- A vault without a recorded version is **unstamped** and read as RLM 1.0. `cdno migrate`
  stamps it with this cdno's version even when that is 1.0.
- File migrations, when a version needs any, never touch the journal or any append-only note.
  When there are none the report says `none: additive`.
- The daily log gains one entry:

  ```text
  vault migrated
    was: unstamped (read as 1.0)
    now: 1.0
  ```

- A vault already at this cdno's version prints `vault is at RLM X; nothing to do`.
- A vault on a **newer** version than this cdno is refused with a non-zero exit: upgrade cdno
  instead.

`migrate` runs only when you run it: never when a vault opens, and never over MCP.

## The notice

When a vault records an older method version than this cdno, other commands print one line on
stderr after they finish, naming both versions and pointing at `cdno migrate --dry-run`. A vault
on a newer minor version gets a line saying writes proceed and cdno can be upgraded. `migrate`,
`init`, `now --line` and shell completion never print it, and stdout (including `--json`) is
never touched.

## Examples

```bash
cdno migrate --dry-run
cdno migrate
```

## See also

- [`init`](init.md), [`lint`](lint.md).
- [Configuration](../configuration.md).
