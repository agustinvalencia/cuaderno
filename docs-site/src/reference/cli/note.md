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
it), active action notes, portfolios and custom notes kept as single files. A portfolio with a
same-slug question is renamed through the question. Dailies, weeklies, monthlies, evidence,
tracking, inbox items, archived action notes, stewardships, commitments, a custom note kept as a
folder and a note with no `type` are refused (`note_not_renamable`).

A Typst note moves with its assets folder: `<old>.typ` and every file under `<old>/` go to the new
name, and the note's own relative path literals into that folder follow. Any other path into the
folder (another note's, a non-literal or a `/`-rooted one) is left as written and breaks. Evidence
is not renamable in either format.

With [`--relink`](#relinking-the-stragglers---relink) the note keeps its slug, and the links that
still reach it through a former name are rewritten to the current one. An archived action note can
be relinked, though not renamed.

### Arguments

| Argument | Description |
|----------|-------------|
| `[NOTE]` | The note to rename, resolved as `cdno open` resolves a reference: a path, a slug, a former slug or `type:slug`. Omitted in an interactive session, a picker offers the renamable notes only; so does the picker an ambiguous or mistyped reference opens (a question and its same-slug portfolio resolve to the question). |

### Options

| Flag | Description |
|------|-------------|
| `--to <SLUG>` | The new slug. Prompted in an interactive session; required under `--no-interactive` (unless `--relink`). |
| `--relink` | Keep the slug and rewrite every link that reaches the note only through a former name to it. Nothing moves. Cannot be combined with `--to` or `--reason` (a usage error, exit `2`); composes with `--dry-run` and `--skip-sync-check`. See [relinking the stragglers](#relinking-the-stragglers---relink). |
| `--reason <STRING>` | Why, in a short clause; it becomes the `reason:` line of the log entry. Never prompted. |
| `--dry-run` | Print the preview, writing nothing. Refused exactly as the write would be (see [the sync check](#the-sync-check)); exits 0 only when the write would proceed. |
| `--skip-sync-check` | Rename even when the vault is behind its upstream. A git operation paused mid-way still refuses. See [the sync check](#the-sync-check). |

### The preview

A long title makes a slug that stops mid-thought. This is a real run against a throwaway vault
(see the [tutorial](../../tutorials/renaming-a-note.md) for the whole story):

```text
$ cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs" --context work
Created projects/calibrate-the-surrogate-model-against-the.md

$ cdno action add --project calibrate-the-surrogate-model-against-the --title "Run the baseline sweep" --note --energy deep
Action added to projects/calibrate-the-surrogate-model-against-the.md with note actions/run-the-baseline-sweep.md

$ cdno action start --project calibrate-the-surrogate-model-against-the --query baseline
Started on calibrate-the-surrogate-model-against-the, logged to journal/2026/daily/2026-11-06.md

$ cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration --dry-run
Dry run: nothing written.
  move     projects/calibrate-the-surrogate-model-against-the.md -> projects/surrogate-calibration.md
  rewrite  actions/run-the-baseline-sweep.md                      1 field (project:)
  rewrite  journal/2026/daily/2026-11-06.md                       3 links
  alias    projects/surrogate-calibration.md gains `calibrate-the-surrogate-model-against-the`
  log      journal/2026/daily/2026-11-06.md
             renamed [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
               was: calibrate-the-surrogate-model-against-the
  4 references in 2 notes · plan 2109…

$ cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration \
    --reason "the derived slug stopped mid-title"
```

The last command prints the same preview, with the `reason:` line (the reason is part of the plan,
so its hash differs), and then ``Renamed `calibrate-the-surrogate-model-against-the` to
`surrogate-calibration`.`` With the note and `--to` given on the command line the write runs straight
away, in a terminal or not: nothing is prompted, so nothing is confirmed. Use `--dry-run` first to
see the plan. Leave out `--to` in a terminal if you want to be asked for it, shown the preview and
asked to confirm.

A `rewrite` line reads `N links`, `M fields (project:, …)` or both (`2 links / 1 field (project:)`).
When the reference matched a former name, the first line is ``found by its former name `old` ``.
The `log` block is the entry as it will be appended, without its time stamp. Warnings follow the totals line. In an interactive session, anything that was prompted (the note, the
slug) brings the preview and a confirm. That confirmed write is given the preview's plan hash within
the same command: it writes exactly what was shown, or, if a referrer changed while the confirm
waited, refuses with `stale_plan` and writes nothing. The hash does not join a `--dry-run` to a later
run, so a change made between the two is not caught: a dry run is a look, not a lock. A rename to the note's current slug writes nothing.

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
| `note_not_renamable` | The note is of a kind the rename does not take (see [which notes can be renamed](../../concepts/note-types.md#which-notes-can-be-renamed)). The `reason` is one of seven, below. | See below. |
| `stem_shared` | Another note has the same file name, so a link to it cannot be told apart from a link to this one. | Rename or move the other note first (or fix the duplicate), then rename. |
| `reference_drift`, `reference_unaddressable` | Rewriting a referrer's frontmatter would change more than the reference, or a field names the note in a form the rewrite cannot address. The message names the note and the key. | Fix that note by hand, then run the rename again. |
| `stale_plan` | A referrer changed while a confirm waited (any log append does it), so the plan you confirmed is no longer the plan. Over MCP the same code answers a write whose `expected_plan` no longer matches. | Preview again and confirm the new plan. |
| `behind_upstream` | The vault is behind its upstream. The text is `the vault is N commits behind its upstream; sync the vault first (pull the upstream's commits, e.g. with `git pull`), or pass `--skip-sync-check` to rename anyway`. | Sync first, or pass `--skip-sync-check`. See [the sync check](#the-sync-check). |
| `git_operation_in_progress` | A merge, rebase or cherry-pick was left paused. Text: `a git operation is in progress in the vault (…); finish or abort it, then rename`. | Finish or abort it. `--skip-sync-check` does not override this. |
| `rename_incomplete` | A write failed and the rollback could not restore every file; the paths are listed. | Check those paths by hand, then run the rename again. |

The seven `note_not_renamable` reasons, and what to do for each. In text mode the code itself is not
printed, only the message (``note '…' cannot be renamed: <reason>``); `--json` carries both.

| `reason` | Meaning | What to do |
|----------|---------|------------|
| `type <name>` (`type none` for a note with no `type:`) | The kind never renames: daily, weekly, monthly, commitment, stewardship, evidence, tracking, inbox; or a note without a `type`. | Nothing: the note keeps its name. For a note with no `type`, add one first. |
| `archived` | An archived action note, whose frozen prefix would change. A `--relink` is not refused: it changes nothing on the note. | Nothing: the note keeps its name. `--relink` still repairs the links to it. |
| `folder` | A folder note other than a portfolio's `portfolios/<slug>/_index.md`, of any type (a custom note kept as a folder, `projects/<slug>/_index.md`); or a portfolio kept as a single file. A portfolio on its own, in its folder, renames fine. | Nothing: the note keeps its name. |
| `rename the question` | A portfolio whose slug a question shares (the portfolio alone is refused). A `--relink` of it is refused too, and relinking the question covers the question's former names only; a rename never gives the pair's portfolio aliases, so this matters only for one listed by hand. | Rename the question: the pair moves together. |
| `it has no frontmatter to record its old slug in` | The note has no frontmatter block. | Repair the note (add frontmatter), then rename. |
| ``its `aliases:` is not a list the old slug can be added to`` | `aliases:` is a scalar or some other shape. | Make `aliases:` a list, then rename. |
| ``its `aliases:` lists the new slug in a form it cannot be removed from`` | The note takes back a former name that `aliases:` lists twice or with a tag. | Tidy the `aliases:` list by hand, then rename. |

A rename that would write over a file the vault already holds is refused as `slug_taken`. That
includes a rename that changes only the case of a slug (`Alpha` to `alpha`): it is refused where the
filesystem cannot tell the two names apart (macOS and Windows by default) and allowed where it can
(a default Linux filesystem). The message names the note itself as the holder (``slug 'alpha' is taken
by 'projects/Alpha.md' (as its stem)``), which is expected. To change only the case, rename through a
temporary slug (`Alpha` to `alpha-tmp`, then `alpha-tmp` to `alpha`); the note ends up with both old
names as aliases. The old spelling (`Alpha`) stays listed but nothing resolves it, because aliases
must be slug-shaped; it can be deleted by hand.

### Renaming back, and former names

The old slug is kept on the note as `aliases: [<old>]`, and no other note may take it while it is
listed. Renaming `old` to `new` and then `new` back to `old` is allowed, and leaves `aliases: [new]`.
In the log, the first `renamed` line's head link is rewritten to `[[old]]`, like any other link to the
note, while its `was: old` line stays as written: `was:` is a copied value, never a link, so it is the
record of what the note was called at the time. The second line reads `renamed [[old]] — … was: new`.

A command that is given a former name still finds the note. `cdno open`, `cdno note revise` and the
rename preview say so (``found by its former name `old` ``). `cdno project show` does not: it prints the
note's current slug in its header, which is how to learn the new name there.

A link written with an old name still works in cdno, so leaving it is harmless. Editing it by hand
to the new name is allowed too, even in a past log entry or an archived action note: it is the same
address correction a rename makes.

### Relinking the stragglers: `--relink`

A link written with a former name after the rename is a straggler: cdno still resolves it through
the alias, and [`cdno lint`](lint.md) reports it, but an editor cannot follow it. The usual way one
appears is a vault synced between two machines with a journal merged line by line (`merge=union`):
the rename rewrote a log line on one machine, the other appended a line beside it, and the merge kept
both versions, so the old line comes back holding the old name (the merge keeps the whole hunk, so
usually several lines come back, not one). `--relink` rewrites every straggler in one go:

```text
$ cdno lint
[warning] journal/2026/daily/2026-11-06.md: `[[calibrate-the-surrogate-model-against-the]]` names a renamed note — now `surrogate-calibration`

$ cdno note rename surrogate-calibration --relink --dry-run
Dry run: nothing written.
  rewrite  journal/2026/daily/2026-11-06.md  1 link
  log      journal/2026/daily/2026-11-06.md
             relinked [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
               references: 1
  1 reference in 1 note · plan 1c09…

$ cdno note rename surrogate-calibration --relink
```

The last command prints the same preview and ``Relinked 1 reference to `surrogate-calibration`.``
It goes through the same preview, plan hash, sync check and refusals as a rename, with these
differences:

- **Only a reference through a former name is rewritten,** links and fields alike. A link is rewritten when it resolves to the note through one of
  its aliases (the links `cdno lint` warns about), and only to an address that resolves to
  the same note. A link that already names the note by its slug or path is left alone, and so is a
  link a former name no longer reaches (another note took that file name since). A reference can
  only ever move to where it already points. A field such as `project:` that holds one of the note's
  former names is rewritten too (lint does not report these, since a field is not a link): a field
  is read by its value alone, so until the relink it named no note at all.
- **It corrects addresses; it does not tidy the merge.** The lines the union merge duplicated stay
  after the relink, now with identical text twice. They are the merge's, not journal entries, so
  deleting the extra copy by hand is yours to do if you want it.
- **An archived action note can be relinked.** A rename refuses it (`archived`, its frozen prefix
  would change), but a relink changes nothing on the target, so the links to a completed action
  that was renamed before it was archived can be repaired. Every other refusal of a rename applies:
  `note_not_renamable` for the other reasons, `stem_shared`, the referrers' own and the sync check.
- **Nothing moves and no alias changes.** The note stays where it is, byte for byte unless it links
  to itself by a former name, and its `aliases:` stay listed, so an old link anywhere else (a file
  `ignore` hides, the other machine) keeps working.
- **The log line is `relinked`.** It reads `relinked [[<slug>]] — <title>` with an indented
  `references: <n>` line and no `was:`, since the name did not change. With no straggler there is
  nothing to write: ``Nothing to relink: no link reaches `<slug>` through a former name.``, and
  nothing is logged.

With `--json` the shape is a rename's, with `moves` empty and `new_slug` equal to `from`.

### On the other machine

Some vaults live in a git repository with a working copy on more than one machine, synced by a tool
that merges. Three things apply:

- **Rename on a machine that is up to date.** The sync check above refuses a rename while the vault is
  behind its upstream. Your sync tool carries the result to the other machine on its next run. Use `--skip-sync-check` only when you know the other machine has nothing unpushed that
  touches these notes, for instance when the upstream is unreachable and you accept the risk.
- **A conflict is the other machine's edit meeting the rename.** The sync stops rather than resolve
  it. Resolve it as you would any merge, then run [`cdno lint`](lint.md): a link the merge brought
  back with the old name is reported as a straggler (`[[old]]` names a renamed note — now `new`).
  It still resolves in cdno, through the alias, but an editor cannot follow it: run
  [`cdno note rename <note> --relink`](#relinking-the-stragglers---relink) to rewrite every one to
  the current name (or fix one by hand).
- **Run any `cdno` command in the vault once on every clone, on a cdno that stamps RLM 1.2 (one that
  has `note rename`), before renaming anything an archived action references.** (Starting the MCP
  server on it counts too.) That records each archived action note in a form that ignores link
  addresses, which is how `cdno lint` tells a corrected link from an edit. A clone that receives a
  renamed archived note before it has done so reports an archived action note error from `cdno
  lint` (`modified an existing line`, or `truncated below its frozen prefix` after a shorter slug)
  that names [`cdno reindex`](reindex.md). `cdno reindex` clears it, at the cost of the stored
  records: every archived action note then loses that check.

## Examples

```bash
cdno note create person --title "Ada Lovelace" --field name=Ada --field role=advisor
cdno note create person --title "Ada Lovelace, programmer and mathematician" --field name=Ada --slug ada-lovelace
cdno note list person
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --dry-run
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --reason "the derived slug was too long"
cdno note rename calibrate-the-surrogate-model --to surrogate-calibration --dry-run --skip-sync-check   # plan a vault that is behind
cdno note rename surrogate-calibration --relink --dry-run   # the links still under a former name
cdno note rename surrogate-calibration --relink
cdno note revise concept:woodbury-identity --section Proof --content-file proof.md \
  --reason "shorter proof via push-through"
```

## Related

- [Renaming a note](../../tutorials/renaming-a-note.md) — the long-title case end to end.
- [Custom note types](../custom-note-types.md) — declaring a type and the full feature.
- [The concept library](../../concepts/concept-library.md) and
  [Building a concept library](../../tutorials/concept-library.md) — `note create --origin` and
  `note revise` in use.
