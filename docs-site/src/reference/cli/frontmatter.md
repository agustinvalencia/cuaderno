# `cdno frontmatter`

Set typed frontmatter fields on a note **through the index**. Every other write
surface (`cdno log`, section upserts) is body-oriented; this is the one that
writes a frontmatter field, so a flag like the daily `meds: true` can be toggled
without a hand-edit that would desync `.cuaderno/index.db`.

The field must be declared in `.cuaderno/config.toml` under
`[schemas.<type>.fields.<key>]` and marked `settable = true` — the write is
driven entirely by that spec (see the [configuration
reference](../configuration.md#typed-schema-fields)).

## `cdno frontmatter set <note> <key> <value>`

Set a declared, settable field to a new value.

```text
cdno frontmatter set [OPTIONS] <NOTE> <KEY> <VALUE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<NOTE>` | The note to edit: `today`, a `YYYY-MM-DD` date (both resolve to the daily note), or a vault-relative note path (e.g. `projects/foo.md`). |
| `<KEY>` | The frontmatter field to set. Must be declared `settable = true` under `[schemas.<type>.fields.<key>]`. |
| `<VALUE>` | The new value, as a string. It is coerced to the field's declared `type` (`bool`/`int`/`string`/`date`) and checked against any `values` allowed-set. |

Takes only the [global options](overview.md#global-options). With `--json`,
emits the `{ path, message }` write result.

### Rules

- **Declared + settable, default-deny.** An undeclared key is rejected; a
  declared field without `settable = true` (absent or `false`) is rejected.
- **Type-checked.** A value that doesn't parse as the declared type — or isn't
  one of a `string` field's `values` — is rejected and nothing is written.
- **Reserved keys are blocked.** `type`, `status`, and a calendar type's period
  key (`date`/`week`/`month`) are engine-owned regardless of config — use the
  lifecycle commands (`cdno project park/activate`, `cdno question set-status`,
  …) for those, so their auto-logging and index invariants are never bypassed.
- **No-op on no change.** Setting a field to the value it already holds writes
  nothing and logs nothing.
- **Optional auto-log.** When the field declares `log_on_change = true`, a real
  change stamps a `key: old → new` line into today's daily note in the same
  commit.
- **Strict-exists (v1).** The key must already be present in the note's
  frontmatter (the daily flags exist via the template default). A missing key
  errors rather than being appended; ordered-insert is a planned follow-up.
  Slug resolution for projects/questions is likewise a follow-up — v1 resolves
  `today`/dates and explicit note paths.

### Examples

```bash
cdno frontmatter set today meds true          # toggle today's daily meds flag
cdno frontmatter set today workout true        # if log_on_change, also logs it
cdno frontmatter set 2026-07-09 closed true    # a specific day's daily note
cdno frontmatter set projects/surrogate.md phase review
```

## Related

- [Configuration reference](../configuration.md#typed-schema-fields) — declaring `[schemas.<type>.fields]`, including `settable` and `log_on_change`.
- [Frontmatter fields](../frontmatter.md) — the built-in per-type frontmatter shapes.
