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
renamed through the question. Dailies, weeklies, monthlies, evidence, tracking, inbox items, archived action
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

A long title makes a slug that stops mid-thought. This is a real run against a throwaway vault
(see the [tutorial](../../tutorials/renaming-a-note.md) for the whole story):

```text
$ cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs" --context work
Created projects/calibrate-the-surrogate-model-against-the.md

$ cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration --dry-run
Dry run: nothing written.
  move     projects/calibrate-the-surrogate-model-against-the.md -> projects/surrogate-calibration.md
  rewrite  actions/run-the-baseline-sweep.md                      1 field (project:)
  rewrite  journal/2026/daily/2026-11-06.md                       3 links
  alias    projects/surrogate-calibration.md gains `calibrate-the-surrogate-model-against-the`
  log      journal/2026/daily/2026-11-06.md
             renamed [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
               was: calibrate-the-surrogate-model-against-the
  4 references in 2 notes · plan 0a13…

$ cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration \
    --reason "the derived slug stopped mid-title"
```

The second command prints the same preview, with the `reason:` line, and then
`Renamed `calibrate-the-surrogate-model-against-the` to `surrogate-calibration`.` Under
`--no-interactive` the write is not asked to confirm; the preview it prints is what it wrote.

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

### Refusals

Every refusal writes nothing. With `--json` each is a [rejection object](../json-output.md#rename-refusals)
with the `code` below; in text the message is printed and the exit status is `1`.

| Code | Why | What to do |
|------|-----|------------|
| `invalid_slug` | `--to` breaks [the slug rule](overview.md#naming-a-note---slug) (upper case, spaces, a leading `-`, a value YAML reads as a number or boolean, too long). | Pick a slug that follows the rule. It is refused, never rewritten. |
| `slug_taken` | Another note, a folder note's folder or another note's former name already holds the slug; the message names the holder. | Pick another name. Nothing is suffixed. |
| `note_not_renamable` | The note is of a kind the rename does not take (see [which notes can be renamed](../../concepts/note-types.md#which-notes-can-be-renamed)). `reason` is `type <name>`, `archived`, `folder`, or `rename the question`. | For a portfolio whose slug a question shares, rename the question: the pair moves together. For the rest, the note keeps its name. |
| `stem_shared` | Another note has the same file name, so a link to it cannot be told apart from a link to this one. | Rename or move the other note first (or fix the duplicate), then rename. |
| `reference_drift`, `reference_unaddressable` | Rewriting a referrer's frontmatter would change more than the reference, or a field names the note in a form the rewrite cannot address. The message names the note and the key. | Fix that note by hand, then run the rename again. |
| `stale_plan` | A referrer changed between the preview and the write (any log append does it), so the plan you confirmed is no longer the plan. | Preview again and confirm the new plan. The MCP tool has the same code with `expected_plan`. |
| `behind_upstream` | The vault is behind its upstream. | Sync first, or pass `--skip-sync-check`. See [the sync check](#the-sync-check). |
| `git_operation_in_progress` | A merge, rebase or cherry-pick was left paused. | Finish or abort it. `--skip-sync-check` does not override this. |
| `rename_incomplete` | A write failed and the rollback could not restore every file; the paths are listed. | Check those paths by hand, then run the rename again. |

A rename that would write over a file the vault already holds is refused as `slug_taken`. That
includes a rename that changes only the case of a slug (`Alpha` to `alpha`): it is refused where the
filesystem cannot tell the two names apart (macOS and Windows by default) and allowed where it can
(a default Linux filesystem).

### Renaming back, and former names

The old slug is kept on the note as `aliases: [<old>]`, and no other note may take it while it is
listed. Renaming `old` to `new` and then `new` back to `old` is allowed, and leaves `aliases: [new]`.
In the log, the first `renamed` line's head link is rewritten to `[[old]]`, like any other link to the
note, while its `was: old` line stays as written: `was:` is a copied value, never a link, so it is the
record of what the note was called at the time. The second line reads `renamed [[old]] — … was: new`.

A command that is given a former name still finds the note. `cdno open`, `cdno note revise` and the
rename preview say so (``found by its former name `old` ``). `cdno project show` does not: it prints the
note's current slug in its header, which is how to learn the new name there.

### On the other machine

Some vaults live in a git repository with a working copy on more than one machine, synced by a tool
that merges. Three things apply:

- **Rename on a machine that is up to date.** The sync check above refuses a rename while the vault is
  behind its upstream, and the sync nudge after a successful rename pushes the result within
  seconds. Use `--skip-sync-check` only when you know the other machine has nothing unpushed that
  touches these notes, for instance when the upstream is unreachable and you accept the risk.
- **A conflict is the other machine's edit meeting the rename.** The sync stops rather than resolve
  it. Resolve it as you would any merge, then run [`cdno lint`](lint.md): a link the merge brought
  back with the old name is reported as a straggler (`[[old]]` names a renamed note — now `new`).
  It still resolves in cdno, through the alias, but an editor cannot follow it; a command that sweeps
  these (`--relink`) is planned and not yet available, so fix such a link by hand for now.
- **Open the vault once on this version on every clone before renaming anything an archived action
  references.** Opening it records a masked snapshot of each archived action note, which is what
  lets the append-only check accept the corrected references. A clone that receives a renamed
  archived note before it has opened the vault on this build reports an `archived action note
  modified` error from `cdno lint` that names [`cdno reindex`](reindex.md). `cdno reindex` clears it,
  at the cost of the archival snapshots: every archived action note then loses its append-only
  check.

## Examples

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note create person --title "Ada Lovelace, programmer and mathematician" --field name=Ada --slug ada-lovelace
cdno note list person
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --dry-run
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --reason "the derived slug was too long"
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --dry-run --skip-sync-check   # plan a vault that is behind
cdno note revise concept:woodbury-identity --section Proof --content-file proof.md \
  --reason "shorter proof via push-through"
```

## Related

- [Renaming a note](../../tutorials/renaming-a-note.md) — the long-title case end to end.
- [Custom note types](../custom-note-types.md) — declaring a type and the full feature.
- [The concept library](../../concepts/concept-library.md) and
  [Building a concept library](../../tutorials/concept-library.md) — `note create --origin` and
  `note revise` in use.
