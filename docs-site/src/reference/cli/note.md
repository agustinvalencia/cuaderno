# `cdno note`

Create, list and revise notes of a [config-defined custom type](../custom-note-types.md) (declared under
`[note_types.<name>]` in `.cuaderno/config.toml`). Built-in types have their own verbs
(`cdno project create`, `cdno question create`, …); this is the generic surface for custom types.

## `cdno note create <type>`

Create a note of custom type `<type>`, written to `<folder>/<slug(title)>.md`.

```text
cdno note create [OPTIONS] <TYPE> --title <TITLE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<TYPE>` | A config-defined custom type (e.g. `person`). A built-in type is refused — use its own create command. |

### Options

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Required. The note's title; its slug becomes the filename. |
| `--slug <SLUG>` | Name the file yourself instead of deriving it from the title. Refused if invalid or taken; see [the slug rule](overview.md#naming-a-note---slug). Not combinable with `--field slug=…`: naming the note twice is refused (`slug_given_twice`), since the two could disagree. |
| `--field <NAME=VALUE>` | A frontmatter field, repeatable. Each key must be a declared `required`/`optional` field of the type; every `required` field must be supplied. |
| `--var <NAME=VALUE>` | A value for the type's template [prompted variable](../../tutorials/templates-and-frontmatter.md#prompted-variables), repeatable. |
| `--body-file <PATH>` | The note's body, read from a file, without the title heading (the engine writes the `# <title>` H1; a leading `# <title>` line in the file is dropped). Written verbatim apart from leading and trailing blank lines and trailing whitespace. It fills the template's `{{body}}` placeholder, or is inserted after the H1 when the template has none. When the template has `{{body}}` and nothing else fills it (`--field body=`, `--var body=` or a `[variables] body` in the config), it is opened in an editor if omitted interactively, and required under `--no-interactive`. |
| `--origin <STRING>` | Where the note came from: one string of wikilinks, e.g. `[[journal/2026/daily/2026-09-02#Woodbury identity]]`. Written, trimmed, as the `origin` frontmatter string even when the template omits or does not quote `{{origin}}`, so the type must declare `origin` (the `concept` type does); otherwise the command is refused. |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result. If the type ships no template (`.cuaderno/templates/<type>.md`), a minimal note is
synthesised from the declared fields plus a `# <title>` heading.

Creation is logged to today's daily note as `<type> created [[…]] — <title>`; do not log it again
by hand.

## `cdno note list <type>`

List every note of custom type `<type>`, by path.

```text
cdno note list <TYPE>
```

## `cdno note revise [note]`

Refine a mutable custom note (such as a concept) in place. The revision is logged to today's daily
note as `revised [[<path>]] — <reason>`, or `revised [[<path>#<Section>]] — <reason>` for a
section, in the same write; do not log it again by hand. Built-in note types (project, action,
daily, …) and custom types declared `append_only = true` are refused.

```text
cdno note revise [OPTIONS] [NOTE]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[NOTE]` | The note to revise, resolved as `cdno open` resolves a reference: a vault path with or without `.md`, a slug, or `type:slug`. An unknown or ambiguous reference is reported as `cdno open` reports it. Omitted in an interactive session, a picker offers every note. |

### Options

| Flag | Description |
|------|-------------|
| `--body-file <PATH>` | Replace the whole body (everything after the frontmatter, which is kept as it is) with the file's contents, written verbatim. Cannot be combined with `--section`. |
| `--section <STRING>` | The heading text of a section to upsert, without the `#` markers. Requires `--content-file`. An existing section is replaced together with its sub-sections; a missing one is appended as `## <section>`. |
| `--content-file <PATH>` | The section's new text, without its heading. Requires `--section`. |
| `--reason <STRING>` | Why the note was revised, in a short clause; it becomes the daily-log line. |

Plus the [global options](overview.md#global-options). With neither `--body-file` nor `--section`,
an interactive session opens the note's current body in your editor, then asks for the reason and
confirms before writing; under `--no-interactive` both are required. A note chosen in a picker
(an omitted, ambiguous or unmatched reference) also brings the confirm, even when every flag was
given. The note is read before
anything is gathered, and a note that changed on disk after that read (an edit saved while the
editor was open, say) is refused rather than overwritten: read it again and redo the revision.

Text identical to the note's current content writes and logs nothing and prints
`No change to <path>`. That includes closing the editor without changing anything: the reason and
confirm are still asked for, and the result is `No change to <path>`. With `--json`, emits `{path, message, changed, new_hash, log_line,
section_target}`.

## `cdno note rename [note]`

Rename a note's slug and follow every reference to it (RFC 0007): the wikilinks that resolve to it,
the `project:` / `portfolio:` style fields that name it, an `aliases:` entry for the old slug on the
note itself, and one `renamed` line in today's daily log. The old name keeps resolving through the alias.

Renamable: projects, questions (a question whose slug a portfolio shares moves the portfolio with
it), active action notes, portfolios and custom notes. A portfolio with a same-slug question is
renamed through the question. Dailies, weeklies, evidence, tracking, inbox items, archived action
notes, stewardships and commitments are refused (`note_not_renamable`).

### Arguments

| Argument | Description |
|----------|-------------|
| `[NOTE]` | The note to rename, resolved as `cdno open` resolves a reference: a path, a slug, a former slug or `type:slug`. Omitted in an interactive session, a picker offers the renamable notes only; so does the picker an ambiguous or mistyped reference opens (a question and its same-slug portfolio resolve to the question). |

### Options

| Flag | Description |
|------|-------------|
| `--to <SLUG>` | The new slug. Prompted in an interactive session; required under `--no-interactive`. |
| `--reason <STRING>` | Why, in a short clause; it becomes the `reason:` line of the log entry. Never prompted. |
| `--dry-run` | Print the preview, writing nothing. Refused exactly as the write would be (see [the sync check](#the-sync-check)); exits 0 only when the write would proceed. |
| `--skip-sync-check` | Rename even when the vault is behind its upstream. A git operation paused mid-way still refuses. See [the sync check](#the-sync-check). |

### The preview

```text
  move     projects/calibrate-the-surrogate-model.md -> projects/surrogate-calibration.md
  rewrite  journal/2026/daily/2026-11-06.md           2 links
  alias    projects/surrogate-calibration.md gains `calibrate-the-surrogate-model`
  log      journal/2026/daily/2026-11-06.md
             renamed [[surrogate-calibration]] — Calibrate the surrogate model
               was: calibrate-the-surrogate-model
               reason: the derived slug was too long
  2 references in 1 note · plan 31d6…
```

A `rewrite` line reads `N links`, `M fields (project:, …)` or both (`2 links / 1 field (project:)`).
When the reference matched a former name, the first line is ``found by its former name `old` ``.
The `log` block is the entry as it will be appended, without its time stamp. Warnings follow the totals line. In an interactive session, anything that was prompted (the note, the
slug) brings the preview and a confirm. The write is then given the preview's plan hash: it writes
exactly what was shown, or, if a referrer changed in between, refuses with `stale_plan` and writes
nothing. A rename to the note's current slug writes nothing.

With `--json`: the plan (`target`, `from`, `new_slug`, `moves`, `edits` with their `changes`, `references`,
`notes`, `log_line`, `warnings`, `plan_hash`) plus `written` (a bool), `index_stale`
and `dry_run`. Refusals are [rejection objects](../json-output.md#rename-refusals) and exit `1`; a
rollback that itself failed (`rename_incomplete`) lists the paths to check by hand, one per line in text.

### The sync check

A rename rewrites many notes at once, so in a vault that is a git repository synced with another
clone it first checks that it is not about to rewrite notes the other clone has already changed.
Unless `--dry-run`, it fetches the upstream (no credential prompt, at most 5 seconds; a fetch that
fails or times out prints one `warning:` line and the check reads the upstream as last fetched).
Then:

| The vault | The rename |
|---|---|
| is not a git repository | goes ahead; no check |
| is a repository git cannot read (a timeout, a refused `safe.directory`) | warns and goes ahead |
| has a git operation paused mid-way (a conflicted merge, a rebase, a cherry-pick) | refuses with `git_operation_in_progress`; `--skip-sync-check` does not override it: finish or abort the operation first |
| is behind its upstream | refuses with `behind_upstream`, naming the remedy: sync first (`git pull`, or your sync), or pass `--skip-sync-check` |
| has no upstream, or it cannot be read | warns and goes ahead |
| has uncommitted changes | goes ahead: a dirty tree is never a reason to refuse |

The whole check runs again once the rename holds the vault's write lock, with the same override
and no fetch, so a merge started, or a fetch landed, while the confirm prompt waits is still refused,
and nothing is written. A dry run is checked exactly as the write is, against the upstream as last
fetched (it never fetches): it refuses with the same code and exit code, so `--dry-run
--skip-sync-check` is how to see the plan of a vault that is behind. Warnings go to stderr; under
`--json` they are `warnings` entries of kind `sync_check` in the plan instead (on stderr only before
a refusal).

## Examples

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note create person --title "Ada Lovelace, programmer and mathematician" --field name=Ada --slug ada-lovelace
cdno note list person
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --dry-run
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --reason "the derived slug was too long"
cdno note revise concept:woodbury-identity --section Proof --content-file proof.md \
  --reason "shorter proof via push-through"
```

## Related

- [Custom note types](../custom-note-types.md) — declaring a type and the full feature.
- [The concept library](../../concepts/concept-library.md) and
  [Building a concept library](../../tutorials/concept-library.md) — `note create --origin` and
  `note revise` in use.
