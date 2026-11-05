# RFC 0007 — Renaming a note: a slug is an address, and addresses can be corrected

| | |
|---|---|
| **Status** | Draft, amended after review — 2026-11-04 (four-seat review, three rounds, on PR #846; no seat holds a blocking point; the maintainer settled D3 and D7 on 2026-11-05) |
| **Tracked by** | PR #846; #579 (created notes cannot be given an explicit slug, so a truncated one is permanent) |
| **Affects** | `cdno-core` (`extractors.rs`, a new `git.rs`, `store.rs`, `index.rs`, one migration), `cdno-domain` (a new `vault/rename.rs`, `vault/slug.rs`, `note_type.rs`, `vault/note_ref.rs`, the create paths, `vault/lint.rs`), `cdno-cli` (`cdno note rename`, `--slug` on the create verbs), `cdno-mcp` (one tool, one field on the create tools, rejection codes, server instructions), `docs/design.md`, `CLAUDE.md`, `docs-site` (including the method page) |
| **Related** | #529 (slug collisions from the word-boundary retreat), #524 (mid-word cut, closed), #225 (globally unique stems), #215 (last-segment resolution), #111 (append-only lint for archived actions), #211 (`cdno lint --fix`), #540 (the post-write sync nudge), RFC 0004 (one locator, every move restages), RFC 0005 (the focus fold) |

> **Amended after review.** The first draft was reviewed on the PR by four seats (invariants and
> data integrity; method and user; CLI and MCP surface; implementation and layering), three
> rounds, seats replying to each other until they agreed. The review widened the exception from
> wikilink addresses to *references*, adding the bare-slug fields the draft missed (§3.5, §5.1);
> replaced index backlinks with a disk scan under the lock (§5.4); moved aliases from a new table
> to the existing frontmatter JSON and made them resolve at every selector (§5.5); replaced the
> frozen-prefix re-stamp with a masked hash and took archived actions out of v1 (§5.3, §5.4);
> made the freshness check local and narrow (§5.8); changed the log line (§5.6); bound preview
> and write by a plan hash (§6.2); and restaged the work (§8). The text below is the amended
> version; the record is on the PR.

---

## 1. Summary

Every created note takes its filename from `slugify`, capped at six words and fifty characters.
A long title gives a slug that stops mid-thought, and the slug is what every later `[[wikilink]]`
types, what the daily log records, what other notes' frontmatter names, and what an editor
follows. Today it is permanent: changing it means renaming the file by hand and leaving every
reference to it dangling, including the ones in the append-only log.

This RFC adds two things:

- **An explicit slug at creation** (#579): `--slug` on the CLI create verbs and a `slug` field on
  the MCP create tools, so a good name can be chosen up front. It ships first and alone.
- **A rename verb** for the notes that already exist: `cdno note rename` and the `rename_note`
  tool. It moves the note, rewrites every **reference** to it — wikilink addresses and the
  bare-slug fields of §3.5 — in every note, append-only ones included, records the old slug in the
  note's `aliases:`, and writes one line to today's log.

The rename needs one deliberate change to the vault's invariants, argued in §5.1: **a reference
is an address, not content.** The append-only rule protects what was recorded and when; correcting
the address a record points at changes neither. The exception is narrow — one operation,
references only, every other byte untouched — and it is written into the invariants and the
method page rather than left implicit.

### 1.1 How you will use it

```sh
cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs"
# -> projects/calibrate-the-surrogate-model-against-the.md

cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration --dry-run
#   move     projects/calibrate-the-surrogate-model-against-the.md -> projects/surrogate-calibration.md
#   rewrite  journal/2026/daily/2026-10-12.md          4 links
#   rewrite  journal/2026/daily/2026-10-14.md          2 links
#   rewrite  actions/run-the-baseline-sweep.md          1 field (project:)
#   rewrite  portfolios/surrogate-error-budget/_index.md  1 link
#   8 references in 4 notes · plan 3f9c…

cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration \
  --reason "the derived slug stopped mid-title"
```

Today's log gains:

```markdown
- **18:02**: renamed [[surrogate-calibration]] — Calibrate the surrogate model against the high-fidelity thermal runs
  was: calibrate-the-surrogate-model-against-the
  reason: the derived slug stopped mid-title
```

The project map gains `aliases: [calibrate-the-surrogate-model-against-the]`. Every
`state on [[…]]`, `started [[…]]` and `during: [[…]]` line in the journal now names
`surrogate-calibration`, and the attached actions' `project:` fields do too, so `gd` in an editor
and a click in Obsidian both land on the note, and the project's history is still one search over
the log. `cdno open calibrate-the-surrogate-model-against-the` still finds it, through the alias.

Better still, the next long title gets its name at birth:

```sh
cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs" \
  --slug surrogate-calibration
```

## 2. Motivation

The six-word, fifty-character cap exists so a filename stays typeable. Its cost lands on exactly
the notes that matter most: projects and questions are named by a sentence, and a sentence cut at
six words usually ends on a preposition or an article (`…-against-the`, `…-on`, `…-of-the`). The
word-boundary retreat from #524 stopped mid-word cuts, but it cannot make a truncated sentence
mean anything, and #529 showed it can collapse two titles onto one slug.

The slug cannot be fixed after the fact because of where it lives:

- **in the filename**, which is what a bare `[[slug]]` resolves against (§3.3);
- **in other notes' frontmatter**, as bare values: an attached action's `project:`, an evidence
  note's `portfolio:`, an inbox item's `captured_during:` (§3.5);
- **in mutable bodies**: project-map `## Links`, a question's related portfolios, custom notes;
- **in the append-only journal**, on nearly every lifecycle line cdno writes — `state on [[slug]]`,
  `started [[slug]]`, `action done on [[slug]]`, `milestone done on [[slug]]`,
  `holder on [[slug]]`, the `during: [[slug]]` continuation;
- **in the frozen prefix of archived actions**, whose bytes lint re-hashes (§3.4).

Renaming by hand leaves all of these dangling. The editors cannot follow them, `cdno lint` reports
each as a warning, the history readers match the slug as text and stop seeing the project's past,
and the readers that compare bare fields as text silently detach a project's actions and deadlines.

## 3. Background

### 3.1 How a slug is made

`slugify` (`crates/cdno-domain/src/vault/slug.rs:51`) lowercases, keeps alphanumerics, joins words
with `-`, and caps at `SLUG_MAX_WORDS = 6` and `SLUG_MAX_CHARS = 50`, retreating to a word boundary
unless that leaves fewer than `SLUG_MIN_AFTER_RETREAT` characters. Most creates pass the result
through `unique_slug` (`slug.rs:138`), which suffixes `-2`, `-3`… against every stem in the vault
(#225). Portfolios do not: a portfolio reuses its question's raw slug as the correlation key
(`portfolios.rs:131`, `find_question_path(&slug)`), and collides loudly instead (#529).

No create surface accepts a slug from the caller.

### 3.2 How links are parsed

`extract_wikilinks` (`crates/cdno-core/src/extractors.rs:230`) yields every `[[target#anchor|label]]`
and `![[…]]` embed in a body, skipping code blocks, inline code and HTML.
`extract_frontmatter_wikilinks` (`extractors.rs:295`) walks every string value of the frontmatter
the same way. Neither sees a bare slug that is not wrapped in `[[…]]`.

### 3.3 How links resolve

`resolve_one` (`extractors.rs:347`) tries, in order: an exact path, the folder index
(`[[portfolios/x]]` → `portfolios/x/_index.md`), then the **last segment** against note stems,
requiring a unique match (#215). The last-segment rule is why a link survives an archival move
and a park: the stem does not change. A rename changes the stem, so no rule rescues the old link.
A link whose last segment matches two stems resolves to nothing and is not a backlink of either.

Editors resolve by filename and path: Obsidian by the shortest unique filename, `gd` in a typical
Neovim setup by filename and path, marksman by filename and H1. **None of them resolves through a
frontmatter `aliases:` field.** In Obsidian, aliases feed search and link autocomplete only;
clicking `[[old-slug]]` creates a new empty note. This rules out an alias-only design (§9 D1).

### 3.4 What "append-only" enforces today

`NoteType::is_append_only` is true for `daily`, `weekly`, `evidence` and `tracking`. For those the
rule is a convention every writer keeps, and nothing re-checks their bytes. The one place cdno
*verifies* immutability is archived actions: `stage_action_archival` records an
`ArchivalSnapshot { frozen_size, frozen_hash, archived_at_ns }` (`crates/cdno-core/src/index.rs:384`)
and lint re-hashes the first `frozen_size` bytes, reporting a divergence as an **Error**.

The snapshot is a row in the SQLite index and nothing else. Reconciliation does not rebuild it,
`remove_note` cascades it away, and `cdno reindex` erases it. Each clone of a synced vault has its
own. Any design that changes a frozen prefix and re-stamps the snapshot on one clone leaves the
other clone reporting a permanent Error (§5.4).

### 3.5 References that are not wikilinks

Several writers record a slug as a bare frontmatter value. The readers compare them as text, and
`find_backlinks` never sees them:

| Source type | Key | Names a | Writer | Reader |
|---|---|---|---|---|
| action | `project:` | project | `vault/actions.rs:83`, `templates/action.md:4` | `commitments.rs:911`, `context.rs:326`, `projects/actions.rs:161` |
| commitment | `project:` | project | `commitments.rs:197` | `commitments.rs:810` |
| commitment | `stewardship:` | stewardship | `commitments.rs:198` | `commitments.rs:800` |
| tracking | `stewardship:` | stewardship | `tracking.rs:544` | tracking readers |
| evidence | `portfolio:` (required) | portfolio | `portfolios.rs:810`, `:874` | `frontmatter/evidence.rs:51` |
| inbox | `captured_during:` | project | `capture.rs:177` | triage copies it into `during: [[…]]` |

`#action/<slug>` tags in bodies also carry a slug. They have no writer in the domain and no
resolver behind them; they are a user convention queried through `find_by_tag`.

## 4. Terminology

- **Slug** — a note's filename stem, or a folder note's folder name.
- **Reference** — a wikilink address (everything between `[[` and the first `#` or `|`) or a value
  under one of the keys in the §3.5 table.
- **Referrer** — any note holding a reference to the note being renamed.
- **Alias** — a former slug of a note, kept in its `aliases:` frontmatter.

## 5. Proposal

### 5.1 The principle: content is immutable, references are maintained

The vault's invariant today reads, in effect, "an append-only note is never edited, only extended".
This RFC narrows what *edited* means:

> The **content** of a note — every byte outside a reference — is never changed by this operation,
> and the content of an append-only note is never changed by anything. A **reference** in any note
> may be rewritten by exactly one family of operations, `rename_note` and its straggler repair
> (§8 S8), and only to follow the note it already names.

The argument: a log line records *that something happened to a note, and when*. Rewriting
`[[calibrate-the-surrogate-model-against-the]]` to `[[surrogate-calibration]]` changes neither
the event nor the time, nor which note it was about — it corrects a pointer whose referent moved.
The alternatives keep the old bytes at the cost of breaking the pointer in every tool the user
reads the log with (§9 D1). Nothing is lost: the rename itself is logged (§5.6), the old slug stays
on the note (§5.5), and the vault's own version control holds the previous bytes.

What the exception does **not** permit: changing a label, an anchor, prose, a time stamp, a tag,
or a reference that does not name the renamed note. It is not a general "fix up the journal" path.
`docs/design.md` §7, the `CLAUDE.md` invariants, the server instructions' append-only bullet and
the method page (`docs-site/src/concepts/rlm.md`, "past entries are never edited") each say so in
one sentence. Under the method's versioning rule this is a minor method version: every existing
page still reads correctly, and a rename only ever changes addresses.

### 5.2 The verb

`Vault::rename_note(target, new_slug, reason, at)`, one operation in its own file
(`crates/cdno-domain/src/vault/rename.rs`), surfaced as `cdno note rename` and `rename_note`.

Renamable in v1: **project** (active, parked or closed), **question** (with its portfolio, §5.3),
**action note** while active, **portfolio** (on its own, when no question shares its slug), and
**custom notes** (`concept` and any declared type). Not renamable: `daily`, `weekly`, `monthly`
(named by date), `evidence` and `tracking` (named by their entry), `inbox` items (short-lived),
**archived action notes** (their own frozen prefix would change; §5.4), and stewardships and
commitments (deferred, §9 D6).

The new slug is validated by the same rule as #579 (§5.7). It must not be the stem of any existing
note, a folder note's folder, nor an alias of another note (§5.5). A rename to its own current
slug is a no-op that writes nothing. A rename refuses with `stem_shared` when another note shares
the old stem, because links resolving to neither cannot be told apart and would change referent.

The rename changes the slug only; the title, the H1 and the `title:` field are left to the
revision verbs (§9 D3).

### 5.3 What moves

- **A flat note** moves within its current folder: `projects/_parked/a.md` →
  `projects/_parked/b.md`. A rename never changes lifecycle state.
- **A folder note** (a portfolio) moves its whole folder. Its notes are written at the new path
  and deleted at the old one; non-markdown artefacts use the `Move` op. Links into the folder —
  `[[portfolios/a/2026-10-12-baseline]]`, attachment embeds — are referrers; their folder segment
  is rewritten, and each evidence note's `portfolio:` field follows (§3.5). Emptied source
  directories are pruned after commit, best-effort.
- **A question with a portfolio of the same slug** renames both in the one transaction, so the
  correlation `find_question_path(&slug)` relies on survives. Renaming that portfolio on its own is
  refused with a message pointing at the question.

### 5.4 Rewriting the referrers

**Planning under the lock.** The rename opens its `VaultTransaction` first, so the advisory lock is
held, then scans every note from disk. The index is not trusted for planning: the long-running
server does not reconcile between requests, and the index has no edge for a bare-field reference,
an ambiguous stem or some embeds. A substring prefilter on the old slug keeps parsing proportional
to the referrers. The same walk supplies the path set the plan resolves against, so resolution and
referrers come from one snapshot.

**The rewrite.** Two core functions, neither of which knows any key or type:

```rust
// cdno-core/src/extractors.rs — walks the same spans extract_wikilinks walks
pub fn rewrite_wikilink_addresses(
    text: &str,
    names_target: impl Fn(&str) -> bool,
    rewrite: impl Fn(&str) -> String,
) -> (String, usize);

// cdno-core — rewrites one scalar value under `key` in a frontmatter block, as text
pub fn rewrite_frontmatter_scalar(frontmatter: &str, key: &str, from: &str, to: &str) -> (String, usize);
```

Wikilink addresses keep their form; code blocks and code spans are skipped exactly as indexing
skips them:

| Before | After |
|---|---|
| `[[old]]` | `[[new]]` |
| `[[projects/old]]` | `[[projects/new]]` |
| `[[projects/_parked/old]]` | `[[projects/_parked/new]]` |
| `[[old#Current State]]` | `[[new#Current State]]` |
| `[[old\|the calibration]]` | `[[new\|the calibration]]` |
| `![[old]]` | `![[new]]` |
| `project: "[[old]]"` (frontmatter) | `project: "[[new]]"` |
| `project: old` (§3.5 field) | `project: new` |
| `` `[[old]]` `` (code span) | unchanged |
| `#action/old` (tag) | unchanged |

The frontmatter block is rewritten as text, not re-serialised, so key order, quoting and comments
stay put. After the rewrite it is re-parsed and the plan refuses if any value other than the
rewritten references changed. Which keys are references is the §3.5 table, which lives in the
domain (`note_type.rs`, beside `required_fields`) because it is policy; the same table is what the
readers use.

**Frozen prefixes.** The snapshot gains a second, *masked* hash: computed over the frozen prefix
with every wikilink address and every §3.5 field value masked, and stamped with a `mask_version`.
Lint verifies the masked hash. A reference rewritten inside a frozen prefix therefore leaves the
snapshot valid on every clone, with nothing re-stamped and nothing to sync. One append-only
migration adds the columns, and a backfill fills them for every row whose plain hash still
verifies. The stated cost: a hand edit that only re-targets a reference inside a frozen prefix is
no longer detected; the broken-link lint still catches one that dangles. A frozen prefix that
already fails its hash before the rename is reported as a warning in the preview and is still
rewritten: the rename neither repairs nor hides it. A markdown-backed snapshot, which would survive
`cdno reindex`, is a separate issue.

### 5.5 `aliases:` on the renamed note

The renamed note's frontmatter gains (or extends) `aliases: [<old-slug>]`. The field name is
Obsidian's own, so the quick switcher and search find the note by its former name.

- **Storage.** Aliases are read from the `notes.frontmatter` JSON the index already keeps, through
  `VaultIndex::list_aliases()`. No new table, no migration, and nothing that can disagree with the
  frontmatter. Only slug-shaped values enter the alias map; free-text Obsidian display aliases are
  ignored.
- **Resolution.** `resolve_one` takes the alias map as an input beside the path set, so resolution
  stays a pure function of target, paths and aliases. A final rule, after the last-segment rule and
  only on zero stem matches, resolves a target whose last segment equals exactly one note's alias.
  Re-resolution of stored edges fires when the path set *or* the alias set changes, at commit and
  in reconciliation.
- **Selectors.** One domain resolver (`resolve_note_ref`, with the per-type selectors routed
  through it) returns a canonical `ResolvedNote` carrying `resolved_via_alias`. Old names work for
  reads and writes alike. Log-line builders accept only a `ResolvedNote`, so no log line is ever
  composed from a typed old slug.
- **Stragglers.** A reference that resolves only through an alias is a lint **Warning**, "`[[old]]`
  names a renamed note — now `new`": editors cannot follow it. Straggler repair is its own stage
  (§8 S8).
- **Tags.** `find_by_tag` queries for an action note's `#action/<slug>` also match its aliases.
- **Reserved names.** No create and no rename may take a slug that is another note's alias. A
  derived slug that hits one is suffixed by `unique_slug`; an explicit one is rejected. Otherwise an
  old reference would silently change referent. Aliases are reserved for as long as they are listed.

### 5.6 The log line

The rename writes one line to today's daily log:

```markdown
- **HH:MM**: renamed [[<new>]] — <title>
  was: <old-slug>
  reason: <reason>
```

`<new>` uses the link form the type's other log lines use (a bare slug for projects,
`actions/<slug>` for action notes, `portfolios/<slug>` for portfolios). `was:` is a copied value,
never a link: as a link it would dangle or be rewritten by the very operation it records, and no
enrichment pass may linkify it. `reason:` follows #564 and is present only when given. `renamed`
joins the prefix list `cdno lint` shares; it is not a focus marker. The line is folded into the
same planned content as today's daily note's own rewrites (§6.2), so the two can never overwrite
each other.

The history readers need no change: after the rewrite every earlier line names the new slug, so
`state on [[surrogate-calibration]]` finds the project's whole past, and an open `started [[…]]`
focus follows the rename.

### 5.7 Choosing the slug at creation (#579)

Every create surface takes an optional explicit slug: `--slug` on `cdno project create`,
`cdno question create`, `cdno portfolio create`, `cdno action add --note`, `cdno action promote`,
`cdno note create`, `cdno stewardship create` and `cdno commit create`; `slug` on the matching MCP
tools. Inbox capture is not included: an inbox item is short-lived and its name is not typed later.

The explicit slug is validated, not rewritten: lowercase ASCII alphanumerics joined by single `-`,
no leading or trailing `-`, at most `SLUG_MAX_CHARS`, and not a value YAML would read as something
other than a string (`2026`, `true`, `null`, `no`). On a collision it is rejected, not suffixed —
the caller asked for that exact name. With no slug given, behaviour is unchanged. `--slug` is never
prompted. Each create tool's description says that the slug is the name every later reference
types, so an agent proposes a short one for a long title.

`cdno portfolio create` gains `--question <ref>`, which takes that question's slug, so a question
created with an explicit slug keeps its correlation — the correlation half of #529.

### 5.8 Vaults synced between clones

Some vaults are git repositories with two working copies converging through a remote, synced by an
external actor that merges (never rebases), uses `merge=union` for the journal, and takes cdno's
advisory write lock while it merges. A rename touches many files at once, which widens the window
for a conflict with an edit made on the other clone and not yet pushed. No pre-write pull can close
that window, because pulling cannot see what the other clone has not pushed. The design makes the
window small and its failures visible and repairable:

1. **Facts, not policy, in core.** A mechanics-only `cdno-core/src/git.rs` reports whether the vault
   is a work tree, whether a git operation is paused mid-way, and ahead/behind as of the last fetch.
   `git_operation_in_progress` moves there from `cdno-mcp/src/checkpoint.rs`, so there is one copy.
   It never touches the network.
2. **Policy in the interfaces.** The CLI fetches before taking the lock (no credential prompt, a
   short timeout; a failed fetch warns and proceeds). Dry runs and the servers never fetch; they use
   the upstream as last fetched. A rename refuses with `behind_upstream` when the vault is known to be
   behind, overridable by `--skip-sync-check` / `skip_sync_check: true` and by nothing else. A paused
   git operation refuses and cannot be overridden; it is re-checked after the lock is taken. When the
   state cannot be determined, the rename warns and proceeds. A vault that is not a git repository
   skips the check. A dirty tree is not a reason to refuse: on the server it is the normal state
   between checkpoints.
3. **Nudge after.** A successful rename fires the post-write sync nudge (#540) where one is
   configured, so it reaches the remote in seconds.
4. **Backstop.** A true conflict stops the sync actor, which never resolves conflicts itself. The
   quiet case is the journal: `merge=union` keeps both sides of a hunk, so a rewritten line next to
   one appended on the other clone can come back duplicated, the duplicate still holding the old
   slug. §5.5 makes it resolve and warn; S8 repairs it. Frozen prefixes need nothing on the other
   clone (§5.4).

## 6. Detailed design

### 6.1 `cdno-core`

- `extractors.rs`: a `wikilink_spans` refactor shared by `extract_wikilinks` and
  `rewrite_wikilink_addresses`; `rewrite_frontmatter_scalar`; `resolve_one` and
  `note_link_names` take the alias map.
- `git.rs`: the probe of §5.8 (1).
- `store.rs`: `VaultStore::remove_dir_if_empty`.
- `index.rs`: `list_aliases()` (a `json_each` query; a scan in `MemoryIndex`); the masked-hash
  columns on `archived_action_snapshots` with their migration and backfill; re-resolution triggered
  by alias changes as well as path changes.

### 6.2 `cdno-domain`

- `vault/slug.rs`: `validate_explicit_slug`, shared by §5.2 and §5.7; `unique_slug` and the taken
  check treat folder names and aliases as taken.
- `note_type.rs`: the §3.5 reference table.
- `vault/note_ref.rs`: the alias tier and `ResolvedNote`.
- `vault/rename.rs`: `plan_rename` builds a `RenamePlan` — a list of
  `PlannedEdit { path, before, after, changes }`, where `changes` are `Address`, `Field`, `LogLine`
  or `Alias` — with **exactly one `after` per path**. `rename_preview` renders the plan without the
  lock; `rename_note` builds it again under the lock and stages it. The preview carries a plan hash
  over every path's `after`, excluding the log line's time and today's date; the write takes
  `expected_plan` and refuses with `stale_plan` on a difference (a day rollover counts as stale).
  Index restaging covers every touched path, including the milestone rows of the moved project and
  of every project that links to it (RFC 0004 found moves dropping them).
- The create paths take `slug: Option<&str>`.
- `vault/lint.rs`: the straggler warning, the masked-hash check and the `renamed` prefix.

### 6.3 `cdno-cli`

`cdno note rename [<note>] --to <slug> [--reason <text>] [--dry-run] [--skip-sync-check]`. The
positional selects the note, as for `note revise` (`docs/cli-ergonomics.md`), and its picker lists
only renamable notes. `--to` is flags-and-prompts; `--reason` is optional and never prompted. The
confirmation, when anything was prompted, shows the preview. A failed rollback is reported per path.
`--slug` joins each create verb of §5.7 as an optional flag that is never prompted.

### 6.4 `cdno-mcp`

`rename_note { note, to, reason?, dry_run?, expected_plan?, skip_sync_check? }` returns the plan
(moves, referrers with counts per kind, warnings, `plan_hash`) whether or not it wrote, plus
`written: bool`. The description tells the agent to preview, show the person, then write with
`expected_plan`, and states §5.1 in one sentence so the rewrite is not mistaken for a breach of the
append-only rule. The server instructions' append-only bullet gains the same sentence.

Rejections: `invalid_slug`, `slug_taken` (naming the holder, and whether it is a stem, a folder or
an alias), `stem_shared`, `note_not_renamable { reason }` (naming the type, or `archived`),
`behind_upstream` (naming the remedy), `git_operation_in_progress`, `stale_plan`. A failed rollback
is `rename_incomplete` with the paths. An `IndexStale` after a good file commit is success with
`index_stale: true`.

### 6.5 Documentation

Documentation ships with each stage: `docs/design.md` §7, the `CLAUDE.md` invariants and the
logged-change family; `docs-site/src/concepts/rlm.md` and the method version line (§5.1);
`docs/cli-ergonomics.md`; a docs-site "Renaming a note" page and the `--slug` flag on each create
page.

## 7. Compatibility

Existing vaults need nothing beyond the snapshot backfill, which runs once and only fills columns.
No note has a slug-shaped `aliases:` entry until it is renamed, unless the person already uses them
in Obsidian; such an alias then resolves in cdno, and one equal to another note's stem is reported
by lint as a conflict rather than resolved (the stem wins). S3 is visible on its own for that
reason, and the changelog says so.

## 8. Implementation plan — staged

| Stage | Content | Ships alone? |
|---|---|---|
| S1 | Explicit slug at creation (#579): `validate_explicit_slug`, `--slug` / `slug`, `--question` on portfolio create, `invalid_slug` / `slug_taken`, tool-description hint | yes, first |
| S2 | Core mechanics: `wikilink_spans`, `rewrite_wikilink_addresses`, `rewrite_frontmatter_scalar` with the re-parse check, `git.rs`, `remove_dir_if_empty`; property tests that bytes outside rewritten references are identical | no behaviour change |
| S3 | Aliases: `list_aliases`, the resolver rule and triggers, the alias tier at every selector, `ResolvedNote`, tag queries, the straggler warning | yes |
| S4 | Masked snapshot hash: migration and backfill | yes |
| S5 | `rename_note` for flat notes, domain only (project, question without portfolio, active action note, custom notes): plan under lock, the reference table, one content per path, milestone restaging, `stem_shared`, a bench on a generated 2 000-note vault, one SQLite-backed test of the index-op order | — |
| S6 | Folder notes and the question–portfolio pair; attachment embeds; directory pruning | — |
| S7 | CLI and MCP surfaces: codes, `expected_plan`, freshness, nudge, server instructions | completes the feature |
| S8 | Straggler repair: `cdno note rename <new> --relink` reusing the plan, or #211 sized on its own | yes |

## 9. Decisions and open questions

- **D1 — rewrite, not redirect.** Considered: (a) aliases only — old links resolve in cdno but not
  in any editor (§3.3); (b) a redirect stub at the old path — navigable everywhere at one hop, but
  a note per rename that every listing must skip; (c) rewriting references keeps every tool working
  with no new note kind and no change to the history readers. Chosen: (c), with aliases as the
  backstop. Agreed by the review.
- **D2 — `aliases` as the field name**, for Obsidian's benefit. Agreed by the review.
- **D3 — a rename changes the slug only.** Three seats chose slug only for v1; one would add
  `--title`, logged as a `title:` continuation, and accepts slug only if retitling gets its own
  issue. **Decided by the maintainer: slug only.** Retitling gets an issue of its own.
- **D4 — freshness.** Refuse only when known to be behind; warn and proceed when unknown; never
  refuse for a dirty tree (§5.8). Agreed by the review.
- **D5 — an alias is reserved while listed.** Releasing it would let an old link change referent
  silently. Uncontested.
- **D6 — stewardships, commitments and archived actions later.** Stewardships have the
  flat-or-expanded exclusivity (#529); commitments are named by bare slug from projects and
  milestones; an archived action's own frozen prefix would change. None is needed for the
  truncated-title problem that motivates this RFC.
- **D7 — the method version.** The review agrees the change is a minor method version with one
  sentence on the method page. **Decided by the maintainer: a minor method version.**
- **D8 — #529.** Explicit slugs (S1) and `--question` on portfolio create give a remedy for both
  collision cases; whether derived portfolio slugs should also disambiguate stays open there.

## 10. Verification

- A rename of a project referenced from a wikilink in frontmatter, a bare `project:` field on an
  attached action and a commitment, a mutable body, three daily notes, an open focus and an archived
  action's frozen prefix: afterwards `cdno lint` is clean on this clone **and on a second clone that
  only pulled**, the project's `state on` history and `current_focus` are unchanged in substance,
  the attached actions and deadlines are still the project's, and the archived action's masked hash
  verifies.
- A byte comparison of every referrer before and after shows changes only inside rewritten
  references.
- A rename refused at any step (taken slug, shared stem, behind upstream, stale plan) leaves the
  vault byte-identical.
- A failure injected mid-commit rolls back, and a failed rollback is reported per path.
- An index deleted after a rename is rebuilt with the same aliases and edges.
- A straggler `[[old]]` written after the rename resolves, warns, and is repaired by S8.
- The preview and the write list the same paths, from the same plan, and the write refuses a stale
  plan.
- Empty directories left by a folder move are pruned, or harmless if pruning fails.
