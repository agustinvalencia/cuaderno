# RFC 0007 — Renaming a note: a slug is an address, and addresses can be corrected

| | |
|---|---|
| **Status** | Draft — 2026-10-31 |
| **Tracked by** | #579 (created notes cannot be given an explicit slug, so a truncated one is permanent) |
| **Affects** | `cdno-core` (`extractors.rs`, `transaction.rs`, `index.rs`, one migration), `cdno-domain` (a new `vault/rename.rs`, `vault/slug.rs`, the create paths, `vault/lint.rs`), `cdno-cli` (`cdno note rename`, `--slug` on the create verbs), `cdno-mcp` (one tool, one field on the create tools, rejection codes), `docs/design.md`, `CLAUDE.md`, `docs-site` |
| **Related** | #529 (slug collisions from the word-boundary retreat), #524 (mid-word cut, closed), #225 (globally unique stems), #215 (last-segment resolution), #111 (append-only lint for archived actions), #211 (`cdno lint --fix`), #540 (the post-write sync nudge), RFC 0004 (one locator, every move restages), RFC 0005 (the focus fold) |

---

## 1. Summary

Every created note takes its filename from `slugify`, capped at six words and fifty characters.
A long title gives a slug that stops mid-thought, and the slug is what every later `[[wikilink]]`
types, what the daily log records, and what an editor follows. Today it is permanent: changing it
means renaming the file by hand and leaving every link to it dangling, including the ones in the
append-only log.

This RFC adds two things:

- **An explicit slug at creation** (#579): `--slug` on the CLI create verbs and a `slug` field on
  the MCP create tools, so a good name can be chosen up front.
- **A rename verb** for the notes that already exist: `cdno note rename` and the `rename_note`
  tool. It moves the note, rewrites **every** wikilink that resolves to it, in every note,
  append-only and frozen ones included, records the old slug in the note's `aliases:`, and writes
  one line to today's log.

The second point needs one deliberate change to the vault's invariants, argued in §5.1: **a
wikilink target is an address, not content.** The append-only rule protects what was recorded and
when; correcting the address a record points at changes neither. The exception is narrow — one
operation, wikilink targets only, every other byte untouched — and it is written into the
invariants rather than left implicit.

### 1.1 How you will use it

```sh
cdno project create --title "Calibrate the surrogate model against the high-fidelity thermal runs"
# -> projects/calibrate-the-surrogate-model-against-the.md

cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration --dry-run
#   move     projects/calibrate-the-surrogate-model-against-the.md -> projects/surrogate-calibration.md
#   rewrite  journal/2026/daily/2026-10-12.md       4 links
#   rewrite  journal/2026/daily/2026-10-14.md       2 links
#   rewrite  actions/_done/2026/run-the-baseline-sweep.md  1 link (frozen prefix, re-stamped)
#   rewrite  portfolios/surrogate-error-budget/_index.md   1 link
#   9 links in 4 notes

cdno note rename calibrate-the-surrogate-model-against-the --to surrogate-calibration \
  --reason "the derived slug stopped mid-title"
```

Today's log gains:

```markdown
- **18:02**: renamed [[surrogate-calibration]] — was `calibrate-the-surrogate-model-against-the`
  reason: the derived slug stopped mid-title
```

The project map gains `aliases: [calibrate-the-surrogate-model-against-the]`. Every
`state on [[…]]`, `started [[…]]` and `during: [[…]]` line in the journal now names
`surrogate-calibration`, so `gd` in an editor and a click in Obsidian both land on the note, and the
project's history is still one search over the log.

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
- **in frontmatter** of other notes: `project: "[[slug]]"` on portfolios and attached actions;
- **in mutable bodies**: project-map `## Links`, a question's related portfolios, custom notes;
- **in the append-only journal**, on nearly every lifecycle line cdno writes — `state on [[slug]]`,
  `started [[slug]]`, `action done on [[slug]]`, `milestone done on [[slug]]`,
  `holder on [[slug]]`, the `during: [[slug]]` continuation;
- **in the frozen prefix of archived actions**, whose bytes lint re-hashes (§3.4).

Renaming by hand leaves all of these dangling. The editors cannot follow them, `cdno lint` reports
each as a warning, and the history readers — the project-evolution search and the focus fold in
`vault/context.rs` — match the slug as text and stop seeing the project's past.

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
the same way, so `project: "[[slug]]"` is an edge like any other. Both are already the single source
of truth for what counts as a link; the rename reuses them.

### 3.3 How links resolve

`resolve_one` (`extractors.rs:347`) tries, in order: an exact path, the folder index
(`[[portfolios/x]]` → `portfolios/x/_index.md`), then the **last segment** against note stems,
requiring a unique match (#215). The last-segment rule is why a link survives an archival move
(`actions/x.md` → `actions/_done/2026/x.md`) and a park (`projects/x.md` → `projects/_parked/x.md`):
the stem does not change. A rename changes the stem, so no rule rescues the old link.

Editors resolve the same way: Obsidian by the shortest unique filename, `gd` in a typical Neovim
setup by filename and path, marksman by filename and H1. **None of them resolves through a
frontmatter `aliases:` field.** In Obsidian, aliases feed search and link autocomplete only;
clicking `[[old-slug]]` creates a new empty note. This rules out an alias-only design (§9 D1).

### 3.4 What "append-only" enforces today

`NoteType::is_append_only` is true for `daily`, `weekly`, `evidence` and `tracking`. For those the
rule is a convention every writer keeps — cdno only ever appends — and nothing re-checks their
bytes. The one place cdno *verifies* immutability is archived actions: `stage_action_archival`
records an `ArchivalSnapshot { frozen_size, frozen_hash, archived_at_ns }`
(`crates/cdno-core/src/index.rs:383`) and lint re-hashes the first `frozen_size` bytes on every
run, reporting a divergence as an **Error** (`vault/lint.rs:165`). The snapshot is documented as
"set once and never overwritten".

## 4. Terminology

- **Slug** — a note's filename stem, or a folder note's folder name. What a bare `[[…]]` names.
- **Address** — the target part of a wikilink: everything between `[[` and the first `#` or `|`.
- **Referrer** — any note holding a link whose address resolves to the note being renamed.
- **Alias** — a former slug of a note, kept in its `aliases:` frontmatter.

## 5. Proposal

### 5.1 The principle: content is immutable, addresses are maintained

The vault's invariant today reads, in effect, "an append-only note is never edited, only extended".
This RFC narrows what *edited* means:

> The **content** of an append-only note — every byte outside a wikilink address — is never
> changed. A wikilink **address** in any note may be rewritten by exactly one operation,
> `rename_note`, and only to follow the note it already resolves to.

The argument: a log line records *that something happened to a note, and when*. Rewriting
`[[calibrate-the-surrogate-model-against-the]]` to `[[surrogate-calibration]]` changes neither
the event nor the time, nor which note it was about — it corrects a pointer whose referent moved.
The alternative designs all keep the old bytes at the cost of breaking the pointer in every tool
the user reads the log with (§9 D1). Nothing is lost: the rename itself is logged (§5.6), the old
slug stays on the note (§5.5), and the vault's own version control holds the previous bytes.

What the exception does **not** permit: changing a label, an anchor, prose, a time stamp, or any
link that does not resolve to the renamed note. It is not a general "fix up the journal" path, and
the invariant text in `docs/design.md` §7 and `CLAUDE.md` says so next to the existing raw-write
exceptions.

### 5.2 The verb

`Vault::rename_note(target, new_slug, reason, at)`, one operation in its own file
(`crates/cdno-domain/src/vault/rename.rs`), surfaced as `cdno note rename` and `rename_note`.

Renamable in v1: **project**, **question** (with its portfolio, §5.3), **action note**, **portfolio**
(on its own, when no question shares its slug), and **custom notes** (`concept` and any declared
type). Not renamable: `daily`, `weekly`, `monthly` (named by date), `evidence` and `tracking`
(date-prefixed, named by their entry), `inbox` items (short-lived), stewardships and commitments
(deferred, §9 D6).

The new slug is validated by the same rule as #579: lowercase ASCII alphanumerics joined by single
`-`, no leading or trailing `-`, at most `SLUG_MAX_CHARS`. Anything else is rejected, not
rewritten. It must not be the stem of any existing note, nor an alias of another note (§5.5); a
rename to its own current slug is a no-op that writes nothing.

### 5.3 What moves

- **A flat note** moves within its current folder: `projects/_parked/a.md` →
  `projects/_parked/b.md`, `actions/_done/2026/a.md` → `actions/_done/2026/b.md`. A rename never
  changes lifecycle state.
- **A folder note** (a portfolio) moves its whole folder, evidence and attachments included.
  Links into the folder — `[[portfolios/a/2026-10-12-baseline]]` — are referrers too; their
  folder segment is rewritten.
- **A question with a portfolio of the same slug** renames both in the one transaction, so the
  correlation `find_question_path(&slug)` relies on survives. Renaming that portfolio on its own is
  refused with a message pointing at the question.
- **An archived action** moves its `ArchivalSnapshot` row to the new path.

### 5.4 Rewriting the referrers

The referrer set is the index's backlinks of the note (`VaultIndex::find_backlinks`), plus, for a
folder note, the backlinks of every note inside the folder. Because `Vault::new` reconciles, the
index matches the disk on entry. Each referrer is re-read from disk inside the transaction and
re-parsed; the index only says *where to look*.

In each referrer, a new core function rewrites addresses by byte span:

```rust
// cdno-core/src/extractors.rs
pub fn rewrite_wikilink_addresses(
    text: &str,
    resolves_to_target: impl Fn(&str) -> bool,
    rewrite: impl Fn(&str) -> String,
) -> (String, usize);
```

It walks the same events `extract_wikilinks` walks — so it skips code blocks, code spans and HTML
exactly as indexing does — and for each link whose address resolves to the renamed note replaces
**only the address**, keeping its form:

| Before | After |
|---|---|
| `[[old]]` | `[[new]]` |
| `[[projects/old]]` | `[[projects/new]]` |
| `[[projects/_parked/old]]` | `[[projects/_parked/new]]` |
| `[[old#Current State]]` | `[[new#Current State]]` |
| `[[old\|the calibration]]` | `[[new\|the calibration]]` |
| `![[old]]` | `![[new]]` |
| `project: "[[old]]"` (frontmatter) | `project: "[[new]]"` |
| `` `[[old]]` `` (code span) | unchanged |

The frontmatter block is rewritten as text with the same function, not re-serialised, so key order,
quoting and comments are untouched. A slug is ASCII alphanumerics and `-`, so it never needs YAML
quoting that the old one did not.

**Frozen prefixes.** When a referrer is an archived action whose rewrite falls inside
`frozen_size`, the transaction stages a new `ArchivalSnapshot` for it: `frozen_size` adjusted by the
length difference of the rewrites inside the prefix, `frozen_hash` recomputed over the new prefix,
`archived_at_ns` unchanged. This is the only writer allowed to re-stamp a snapshot; the "set once"
comment on `ArchivalSnapshot` is amended to say so. A frozen prefix that already fails its hash
before the rename (an earlier hand edit) is not re-stamped: the rename refuses and names the note,
so a re-stamp can never launder an unrelated edit.

### 5.5 `aliases:` on the renamed note

The renamed note's frontmatter gains (or extends) `aliases: [<old-slug>]`. The field name is
Obsidian's own, so the quick switcher and search find the note by its former name for free.

In cdno the alias does three things:

1. **Resolution, as a last resort.** `resolve_one` gains a final rule after the last-segment match:
   a target whose last segment equals exactly one note's alias resolves to that note. A straggler
   `[[old]]` — typed later from memory, or reintroduced by a sync race (§5.8) — still yields a
   backlink and still counts in the history readers.
2. **Lint.** Such a link is reported as a **Warning**, "`[[old]]` names a renamed note — now
   `new`", and `cdno lint --fix` (#211) rewrites it with the same function as §5.4. Editors cannot
   follow an alias, so a straggler is a defect even when cdno resolves it.
3. **A reserved name.** No create and no rename may take a slug that is another note's alias; a
   derived slug that hits one is suffixed by `unique_slug` as for any other taken stem, an explicit
   one is rejected. Otherwise an old link would silently change referent.

Aliases are indexed in a new table (migration `003_note_aliases.sql`, append-only per the schema
rule) rebuilt by reconciliation from frontmatter. Markdown stays the source of truth.

### 5.6 The log line

The rename writes one line to today's daily log, a new member of the logged-change family:

```markdown
- **HH:MM**: renamed [[<new-path-or-slug>]] — was `<old-slug>`
  reason: <reason>
```

The old slug is in a code span on purpose: as a link it would either dangle or be rewritten by the
very operation it records. The `reason:` continuation follows #564 and is omitted when no reason
is given. The link uses the same form the type's other log lines use (a bare slug for projects,
`actions/<slug>` for action notes, `portfolios/<slug>` for portfolios). `cdno lint` learns the
prefix with the others.

The history readers need no change: after the rewrite every earlier line names the new slug, so
`state on [[surrogate-calibration]]` finds the project's whole past, and an open
`started [[…]]` focus follows the rename.

### 5.7 Choosing the slug at creation (#579)

Every create surface takes an optional explicit slug: `--slug` on `cdno project create`,
`cdno question create`, `cdno portfolio create`, `cdno action add --note`, `cdno action promote`,
`cdno note create`, `cdno capture`; `slug: Option<String>` on the matching MCP tools. It is
validated as in §5.2, rejected rather than suffixed on a collision (the caller asked for that exact
name), and leaves the title alone. With no slug given, behaviour is unchanged.

For a question created with an explicit slug, a portfolio created later from the same question
text would derive a different slug and lose the correlation. `create_portfolio` therefore gains an
optional `question` reference that, when given, takes that question's slug — which also answers
the correlation half of #529.

### 5.8 Vaults synced between clones

Some vaults are git repositories with two working copies converging through a remote, synced by an
external actor that merges (never rebases), uses `merge=union` for the journal, and takes cdno's
advisory write lock while it merges. A rename touches many files at once, which widens the window
for a conflict with an edit made on the other clone and not yet pushed.

No pre-write pull can close that window: pulling here cannot see what the other clone has not
pushed, and a lock across machines is out of proportion. The design instead makes the window small
and its failures visible and repairable:

1. **Freshness check before.** When the vault is a git work tree with an upstream, the CLI and MCP
   surfaces refuse a rename unless the tree is clean and, after a fetch, not behind its upstream.
   The refusal names the remedy (sync first) and `--force` / `force: true` overrides it, including
   when offline. A vault that is not a git repository skips the check. This lives in the interface
   layers next to the git handling `cdno-mcp` already has (`checkpoint.rs`), never in the domain.
2. **Nudge after.** A successful rename fires the post-write sync nudge (#540) where one is
   configured, so it reaches the remote in seconds.
3. **Backstop.** A true conflict with the other clone stops the sync actor, which never resolves
   conflicts itself. The quiet case is the journal: `merge=union` keeps both sides of a hunk, so a
   rewritten line next to a line appended on the other clone can come back duplicated, the
   duplicate still holding `[[old]]`. §5.5 makes that straggler resolve, warn, and fix with
   `lint --fix`.

## 6. Detailed design

### 6.1 `cdno-core`

- `extractors.rs`: `rewrite_wikilink_addresses` (§5.4); `resolve_one` gains the alias rule after
  the last-segment rule, taking the alias map alongside `vault_paths`.
- `transaction.rs`: a `MoveArchivalSnapshot(src, dest)` index op, and `RecordArchivalSnapshot`
  documented as overwriting (it already upserts). A folder move uses the existing `Move` op per file,
  so rollback stays reverse-order.
- `index.rs` + `migrations/003_note_aliases.sql`: a `note_aliases(note_path, alias)` table,
  `ReplaceAliases(path, Vec<String>)`, `find_by_alias(alias)`, populated by reconciliation.

### 6.2 `cdno-domain`

- `vault/slug.rs`: `validate_explicit_slug`, shared by §5.2 and §5.7; `unique_slug` treats aliases
  as taken.
- `vault/rename.rs`: `rename_note` and `rename_preview` (the dry run: moves, referrers, link counts,
  frozen re-stamps), both from one plan so the preview cannot drift from the write. One
  `VaultTransaction`, lock held throughout: moves, referrer rewrites, the alias, snapshot ops, the
  log line, index restaging for every touched path.
- The create paths take `slug: Option<&str>`.
- `vault/lint.rs`: the alias warning, the `renamed` prefix, and `--fix` for stragglers.

### 6.3 `cdno-cli`

`cdno note rename [<note>] --to <slug> [--reason <text>] [--dry-run] [--force]`. The positional
selects the note, as for `note revise` (`docs/cli-ergonomics.md`); `--to` and `--reason` are
flags-and-prompts. The confirmation, when anything was prompted, shows the preview. `--slug` joins
each create verb as an optional flag that is never prompted.

### 6.4 `cdno-mcp`

`rename_note { note, to, reason?, dry_run?, force? }` returns the preview shape (moves, referrers
with link counts, re-stamped snapshots) whether or not it wrote, plus `written: bool`. Rejection
codes: `invalid_slug`, `slug_taken` (naming the holder, and whether it is a stem or an alias),
`not_renamable` (naming the type), `frozen_prefix_modified` (naming the note), `vault_not_fresh`
(naming the remedy). The description states the principle of §5.1 in one sentence, so an agent
does not mistake the rewrite for a breach of the append-only rule.

### 6.5 Documentation

`docs/design.md` §7 and the `CLAUDE.md` invariants gain the address exception (§5.1) and the
`renamed` line in the logged-change family; `docs/cli-ergonomics.md` lists `note rename`; the
docs-site gains a "Renaming a note" page and the `--slug` flag on each create page.

## 7. Compatibility

Existing vaults need nothing: no note has `aliases:` until it is renamed, and the migration creates
an empty table. A vault that already uses `aliases:` for Obsidian display names gains resolution
through them; a display alias that equals another note's stem is reported by lint as a conflict
rather than resolved (the stem wins, §5.5 rule order).

## 8. Implementation plan — staged

1. **T1 — explicit slug at creation (#579).** `validate_explicit_slug`, `--slug` and `slug` on every
   create surface, the `question` reference on `create_portfolio`. Independent; ships first.
2. **T2 — the address rewriter.** `rewrite_wikilink_addresses` with a table test per row of §5.4
   and a property test that the bytes outside rewritten addresses are identical.
3. **T3 — aliases.** Migration, index ops, reconciliation, the resolver rule, reserved names.
4. **T4 — `rename_note` for flat notes.** Plan, preview, transaction, log line, snapshot moves and
   re-stamps.
5. **T5 — folder notes and the question–portfolio pair.**
6. **T6 — CLI and MCP surfaces**, the freshness check and the nudge.
7. **T7 — lint**: the alias warning, the prefix, `--fix` for stragglers.
8. **T8 — documentation.**

## 9. Decisions and open questions

- **D1 — rewrite, not redirect.** Considered: (a) aliases only — old links resolve in cdno but not
  in any editor (§3.3); (b) a redirect stub left at the old path — navigable everywhere at one hop,
  but it leaves a note per rename, every listing must skip them, and the history readers still
  need teaching. (c) rewriting addresses keeps every tool working with no new note kind, and the
  history readers need no change at all. Chosen: (c), with aliases as the backstop.
- **D2 — `aliases` as the field name**, for Obsidian's benefit, over a cdno-only `renamed_from`.
  To confirm.
- **D3 — a rename changes the slug only.** The title, the H1 and the `title:` field are left to the
  existing revision verbs. A `--title` on rename is possible later. To confirm.
- **D4 — the freshness check is on by default** for git vaults with an upstream. Alternative: opt
  in through config. To confirm.
- **D5 — an alias is reserved forever.** Releasing it would let an old link change referent
  silently. The cost is a name that can never be reused; `lint` can list them.
- **D6 — stewardships and commitments later.** Stewardships have the flat-or-expanded exclusivity
  (#529) and commitments are linked by bare slug from projects and milestones; both are tractable
  but not needed for the truncated-title problem that motivates this RFC.
- **D7 — #529.** Explicit slugs (T1) and the `question` reference on `create_portfolio` give a
  remedy for both collision cases; whether derived portfolio slugs should also disambiguate stays
  open there.

## 10. Verification

- A rename of a project referenced from frontmatter, a mutable body, three daily notes, an open
  focus and an archived action's frozen prefix: afterwards `cdno lint` is clean, the project's
  `state on` history and `current_focus` are unchanged in substance, and the archived action's
  snapshot verifies.
- A byte comparison of every referrer before and after shows changes only inside rewritten
  addresses.
- A rename refused at any step (taken slug, modified frozen prefix, not fresh) leaves the vault
  byte-identical.
- An index deleted after a rename is rebuilt with the same aliases and edges.
- A straggler `[[old]]` written after the rename resolves, warns, and is fixed by `lint --fix`.
- The preview and the write list the same paths, from the same plan.
