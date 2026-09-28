# RFC 0002 — Concept notes

| | |
|---|---|
| **Status** | Accepted — 2026-09-27 (after review-panel rounds on #604 and #610; maintainer rulings in §9). Stages 0 to 2 shipped: #635 (T0), #638 (T1), #647 (T2), #639 (T3), #642 (T4), #643 (T5), #648 (T6), #645 (T7), #650 (T8), #651 (T9), #652 (T10), #653 (T11), #654 (T12), #644 (T13), #649 (T14), #655 (T15), #656 (T16), #657 (T17). Follow-ups shipped: #659 (T3b), #658 (T7b). Stage 3 (T18 trial, T19 decision) is open. |
| **Tracked by** | #612 (epic), #613–#632 (T0–T19); task breakdown in [0002-implementation-plan.md](0002-implementation-plan.md) |
| **Affects** | `cdno-core` (one resolver fix), `cdno-domain`, `cdno-cli`, `cdno-mcp`, `docs/design.md`, `docs/implementation-plan.md`, `docs-site`, `examples/` |
| **Related** | RFC 0001 (format precedent); custom note types (`docs-site/src/reference/custom-note-types.md`); #597 (desktop retirement — CLI and MCP surfaces only); `docs/implementation-plan.md` Phase 7 (the "standalone note" this RFC resolves); #604 (superseded draft) |

> **Authorship.** The idea comes from a conversation between the maintainer and Claude (web) on
> 2026-09-02. The shape here follows two review-panel rounds on pull requests #604 and #610 and the
> maintainer's rulings recorded in §9. Drafted by Claude Code against the codebase as it stands.

---

## 1. Summary

Add a **concept note**: a unit of understanding that is worth keeping and reusing, tied to no
project, question, stewardship or portfolio. A theorem and the intuition behind it, a definition, a
technique and when it applies, a procedure you will run again. Concept notes form a **library of
concepts** in a flat `concepts/` folder, organised by links and tags rather than by hierarchy.

A concept note is **refined in place** as understanding improves, and every refinement leaves a
one-line trace in the daily log. It has no lifecycle, no staleness, no review obligation and no cap.
It must be findable on its own, by title and full-text search.

Five points carry the weight:

- **A filing test** that separates a concept from evidence, with a **provenance rule** so that
  refining a concept never changes what a piece of evidence meant (§5.1).
- **One concept per note**, linked, with structure emerging from links, tags and hand-written hub
  notes rather than from folders (§5.2, §5.7).
- **A `## Notes` section in the daily note** for worked-out substance, so `## Logs` stays a
  sequence of one-line pointers (§5.4), with **promotion from it** as a habit the tool makes cheap
  and never enforces (§5.5).
- **Logged refinement and logged creation**, so the daily log keeps its role as the record of how
  the vault changes (§5.3, §6.2).
- **A custom type on generic tooling.** No thirteenth built-in: the `concept` type ships as a
  declared custom type, and what it needs is a small set of generic operations every custom type
  lacks today (§6), followed by a trial with its exit questions written first (§8).

---

## 2. Motivation

Some knowledge is neither evidence nor work. Take *what the Woodbury identity buys you when
inverting a low-rank update*, or *how to do a rolling restart of a StatefulSet without losing
quorum*:

- It answers no open question and weighs toward no conclusion, so it is not **evidence**. Evidence
  is dated, sourced and append-only; understanding is timeless and improves.
- It has no deliverable and never completes, so it is not a **project** or an **action**.
- It belongs to no perpetual responsibility, so it is not a **stewardship** routine.
- In the **daily log** it can be found, but it is scattered across several days in several partial
  versions, and the version you find is not necessarily the best one. Worse, substance written into
  `## Logs` turns a sequence into a wall of text.

The cost is **re-derivation**: working the same thing out again because the last time is buried,
or because three half-versions disagree. A concept note is the single, current, best account.

The design already knows this gap exists. `docs/implementation-plan.md` Phase 7 maps mdvault's
zettels to "evidence or standalone note", and the framework has never said what a standalone note
is: every indexed note must carry a declared `type` (`cdno-core/src/reconcile.rs`), and lint
reports any type nobody declared. Portfolios turned out to be the right home for *findings*; they
were never the right home for *understanding*. This RFC gives that second thing its type.

---

## 3. Background — what the design builds on

- **Twelve built-in types** (`crates/cdno-domain/src/note_type.rs`). `daily`, `weekly`,
  `evidence` and `tracking` are append-only; the rest are mutable in place.
- **History preservation.** Replacing a project's `## Current State` writes a `state on [[slug]]`
  entry to the daily log (`vault/projects/state.rs`). The vault's invariant is that no mutable
  section is replaced without a log entry. Creation is logged for commitments only
  (`vault/commitments.rs:187`); project, question, portfolio, stewardship and custom-note creation
  write no line today.
- **The daily note** is `# heading` plus `## Logs`. `vault/daily.rs` names `## Notes` as a
  second append-only history section but does not implement it; `DailySection` is the allow-list
  for `upsert_daily_section` (Standup, Intention, Agenda, Meeting), which pins `## Logs` to the
  bottom after every write. Heading lookup in `cdno-core/src/markdown.rs` is by flat text and
  blind to heading level.
- **Custom note types** (`[note_types.<name>]`) give a folder, required and optional fields, a
  template, lint, indexing, full-text search, backlinks and `cdno note`, but no behaviour, and
  they are outside `cdno orient` and the reviews. That last property is exactly right for a
  concept library. `create_custom_note` renders the template only; it takes no body, and no tool
  edits a custom note's body.
- **Links are indexed generically** (`links` table; `target_raw` keeps the raw target), and the
  domain answers backlinks for one type (`question_backlinks`, `vault/context.rs`). Frontmatter
  wikilinks are extracted as edges. **Tags** are indexed from frontmatter and body.
- **Heading links do not resolve.** `resolve_one` (`cdno-core/src/extractors.rs`) matches the
  whole target including any `#anchor`, so `[[note#Heading]]`, the form `docs/design.md:587`
  uses for `milestone:` links, never resolves and never produces a backlink.
- **`set_frontmatter`** writes a field declared `settable = true` under `[schemas.<type>.fields]`
  (`vault/set_frontmatter.rs`); the docs-site sentence "`[schemas.<custom>]` has no effect" is
  wrong against `type_registry.rs` and is corrected separately.
- **Search** ranks title hits ten times above body hits and filters by `note_type`. **`cdno
  open`** resolves bare slugs, type-scoped slugs and paths. The MCP surface has no generic
  note-read tool, although `context.rs:260` already promises one.

---

## 4. Terminology

- **Concept note**: a note of type `concept`, in `concepts/`.
- **Hub note**: a concept note whose content is mostly links: a map of a cluster of concepts. By
  convention its slug is the tag name it maps.
- **Refinement**: a change to a concept note's content made through the tool.
- **Promotion**: creating a concept note from a `## Notes` entry, recorded in `origin:`.

---

## 5. Proposal

### 5.1 The filing test

Ask of the thing you want to keep: *what is it?*

1. **A dated observation, result or source that bears on an open question** → **evidence** in a
   portfolio.
2. **The answer to a question you weighed evidence for** → the **question note**, with
   `status: answered`. Its `## Current Thinking` may be promoted into a concept that the question
   links to; the question keeps the record of *how* it was answered.
3. **A prescribed practice belonging to one stewardship** (a workout plan, a care routine) → a
   **stewardship routine** (`design.md` §5.7). The `routine:` pointer on tracking entries is how
   you know one is in use.
4. **Understanding you will reuse, independent of any deliverable**: a theorem, a definition, a
   technique, a procedure, an idea → a **concept note**.
5. **Not yet settled** (still being worked out) → the daily note's **`## Notes`** (§5.4), until
   promoted (§5.5).

A benchmark result is evidence. What the benchmark taught you about the technique is a concept.
The command that ran it is part of the evidence note, and may also become a concept note if you
will run it again.

**Provenance rule.** Evidence never depends on the *current* text of a concept note. An evidence
note records what was done and observed inline (the exact invocation, the version, the
parameters), and may link to a concept as *"see also"*. A concept is never the `origin:` of an
evidence note. Concepts describe understanding **now**; evidence records what happened **then**.
This is what lets a concept note be refined freely.

### 5.2 Shape

```markdown
---
type: concept
created: 2026-09-25
tags: [linear-algebra, optimisation]
origin: "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]"
---

# Woodbury identity

## Statement
…

## Why it matters
…

## See also
- [[concepts/sherman-morrison]]
- [[concepts/low-rank-updates]]
```

- **Required:** `type`, `created`. The title is the body H1, as for every note (§6.1); there is
  no `title` field.
- **Optional:** `tags` (the subject vocabulary: open, indexed, no config); `origin` (one string
  holding one or more qualified wikilinks to the `## Notes` entries it came from; the frontmatter
  extractor indexes each as a separate edge). The shipped template carries only `type`, `created`
  and `tags: []`; `origin` is written, as a quoted YAML string, only when it is supplied at
  creation.
- **One link form.** Concept notes and their `origin` links use qualified wikilinks:
  `[[journal/2026/daily/<date>#Heading]]`, `[[concepts/<slug>]]`. The text an agent writes to the
  daily note through `append_to_log`, `upsert_daily_section` and `note_to_daily` bodies keeps the
  bare `[[slug]]` form of the pre-RFC convention (as in `state on [[slug]]`); the `noted`,
  `revised` and `created` lines the tool writes carry the qualified path.
- **Links:** none is required. A concept with no links in or out is not a lint finding. Links to
  projects, questions, stewardships and other concepts go in the body like any other wikilink.
- **Unit:** one concept per note. If a note needs two headings that could each be cited on their
  own, it is usually two notes. A single derivation that does not split may stay one note, with
  sections cited by heading link (`[[concepts/woodbury-identity#Statement]]`).
- **Body:** free-form. The default template offers *Statement / Why it matters / See also*, and a
  procedure or a definition simply uses different headings. A procedural concept records "last run
  <date> on <version>" in its body if that matters; there is no field for it.

### 5.3 Refinement

A concept note is **mutable in place**. A better explanation replaces the worse one; a correction
replaces the error. The note is always the current best account.

Every refinement made through the tool writes **one line to today's daily log**:

```
- **14:32**: revised [[concepts/woodbury-identity#Why it matters]] — added the cost comparison
```

- The **reason is required**. It is the part of the history worth keeping.
- The section anchor is present when a single section was revised.
- There is **no `was:`/`now:` block**. The log records *when* and *why*; git records *what*.
  `docs/design.md` §7 and `CLAUDE.md` name this as an explicit exception to the `was:`/`now:`
  shape and state that trade-off.
- `revised [[` is a fixed prefix in the family of `state on [[`. No reader parses it today; it is
  reserved so one can. (`current_focus` requires the `started` verb, so this line cannot be
  mistaken for a focus line.)
- Whole-body replacement and single-section upsert are both available; the call shape decides
  (`section` present means upsert). There is **no append operation** for concept notes.

Edits made outside cuaderno (in an editor) are legitimate, since markdown is the source of truth,
but they leave no log line. That is stated as a limit of the invariant, not enforced by lint.

### 5.4 Where substance goes: the daily note's `## Notes`

Today everything written during the day lands in `## Logs`, and substance (a derivation, a worked
procedure, a page of reasoning) sits as a block of text inside what is meant to be a *sequence*.
This RFC gives the anticipated `## Notes` section its job:

- **`## Logs` is the sequence.** One line per event, pointers only.
- **`## Notes` is the substance.** Append-only, like `## Logs`. Each entry is its own `### `
  heading so it can be addressed. It holds worked-out material belonging to no project or question
  yet; meetings stay in `## Meeting`, and project reasoning stays on the project map.
- **The log line points at the entry** and carries the entry's own wikilinks, so the day's
  sequence shows that a derivation happened and what it touched:
  `- **14:32**: noted [[journal/2026/daily/2026-09-27#Woodbury identity]] ([[projects/surrogate-model]])`.
  `noted [[` joins the reserved prefix family.
- **Placement.** `## Notes` sits immediately **before** `## Logs`; the existing anchor logic in
  `upsert_daily_section` (`ensure_section` then pin `## Logs` to the bottom) puts it there with
  no template change.
- **Headings.** Heading lookup is flat and level-blind, so an entry heading must not reuse any
  section name (`Logs`, `Notes`, `Standup`, `Intention`, `Agenda`, `Meeting`) or an existing
  entry heading in that day's note. The tool enforces this; a hand-written entry follows it as a
  convention.
- **Candidates.** A reusable entry ends with the body tag `#concept`. Body tags are indexed, so
  "the same subject on two or more dates" is a tag search across daily notes.

```markdown
## Notes

### Woodbury identity
(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1 — cheap when A^-1 is known and the
update is low-rank; today's use was the k=3 refit on [[projects/surrogate-model]]. #concept

## Logs
- **14:32**: noted [[journal/2026/daily/2026-09-27#Woodbury identity]] ([[projects/surrogate-model]])
```

**The trail.** `## Notes` is not read by the project, weekly or monthly context (those readers
consume `## Logs` only), so an entry that concerns a project is visible to that project through
the pointer line's wikilinks and through the entry's own wikilinks, which reconciliation extracts
from the whole body as backlinks. The reviews see that a derivation happened; they do not see the
derivation. Whether that is enough is one of the trial's questions (§8).

**Heading links must resolve.** `[[note#Heading]]` is the Obsidian form and is already used by
`docs/design.md` for milestone links, but `resolve_one` matches the whole target including the
anchor, so every heading link is unresolved today: the `links` table has no edge and backlinks are
silently missing. Splitting the target before the first `#` and resolving the path part is a
standalone bug fix (stage 0, §8) that `## Notes` pointers and `origin:` depend on. The anchor is
opaque to the resolver; raw heading text is canonical. Nested anchors (`[[note#A#B]]`) are not
supported.

The `was:`/`now:` blocks that project state changes write are outside this RFC, but they are the
same problem and would fit the same section later.

### 5.5 Promotion

Write a concept when you already know you will reuse it. When unsure, note it in the day and
promote it the second time it comes up; `origin:` records the entries it came from. Promotion is
**create with `origin`**: nothing else, no separate operation, no counting.

- The agent offers promotion when `#concept` entries on the same subject appear in daily notes on
  two or more dates, and searches before creating so it extends an existing concept rather than
  minting a duplicate. Both instructions live in tool descriptions (§6.4).
- The promotion search is full-text: it matches the word `concept` (and its stem) in daily notes,
  not the `#concept` tag, so it also finds days that merely created or linked a concept, and the
  agent reads each hit for entries ending in `#concept`. That holds until a tag query reaches the
  MCP surface (the deferred tag filter, §8).
- Creation itself is logged (§6.2), so the log shows `concept created [[concepts/<slug>]] —
  <title>` and `origin:` shows where it came from. No `promoted to` line is needed.

### 5.6 No staleness

Understanding does not go stale. A theorem is as true untouched after two years as on the day it
was written, and an explanation that has not changed is one you were happy with. Concept notes
therefore have **no verified date, no age signal, no threshold, no staleness lint and no review
chore**. They never appear in `cdno orient`, the weekly or monthly context, or any review list.

Usefulness is a question you can ask, not a metric that is kept: `read_note` returns the note's
backlinks (already in the `links` table), and "mentioned in the log" is a search. An unreferenced
concept costs nothing and is left alone.

### 5.7 Layout — flat, structured by links

- A flat `concepts/` folder. A concept usually belongs to several subjects at once (a theorem is
  both linear algebra and optimisation), so a folder is the wrong axis; **tags** carry
  multi-membership, and wikilinks carry relationship.
- **Hub notes** carry navigation. When a cluster forms, you write a concept note that is mostly
  links (`concepts/linear-algebra.md`: the theorems, the notation sheet, the techniques). It is an
  ordinary concept note, created by hand when the cluster is real, never a folder decided in
  advance; its slug is the tag name by convention. It is the same pattern as a portfolio's
  `_index.md`.
- **At most one coarse subfolder level** is permitted (`concepts/maths/`), for people who browse
  the folder on disk once it passes a few hundred files. The tool is blind to it: no `subfolder`
  argument, no prompt, and `[[concepts/<slug>]]` still resolves after a hand move through the
  existing stem match.
- No cap on the number of notes.

---

## 6. Detailed design

### 6.1 The type

`cdno init` writes the declaration into a new vault's config with a comment header saying it is
an ordinary custom type and may be deleted, and installs the template as
`.cuaderno/templates/concept.md`. `examples/note-types/concept/` carries the same snippet and
template for existing vaults.

```toml
# A declared custom type: the concept library (RFC 0002). Delete this block (and
# any notes under concepts/) if you do not want one; nothing else depends on it.
[note_types.concept]
folder = "concepts"
required = ["created"]
optional = ["tags", "origin"]
template = "concept.md"
```

The title is the body H1, as for every note. Nothing else changes in `cdno-core` for the type:
the folder is not reserved, and reconciliation and the index already handle a custom type. Whether
`concept` ever becomes a built-in is stage 4's question (§8).

### 6.2 Generic domain operations (`cdno-domain`)

One file per operation under `src/vault/`, each through `VaultTransaction`:

- **`read_note(path | slug)`** (exists, retained under #597): extended to return the
  `content_hash` of the raw bytes read (never the index's `notes.content_hash`, which lags editor
  writes), the note's backlinks, its frontmatter and its `headings`. No body cap: a capped body
  with a full-file hash would let a whole-body revision drop the tail.
- **`note_to_daily(date, heading, body)`**: appends a `### heading` entry to `## Notes`
  (creating the section before `## Logs` if absent) and the `noted [[…]]` pointer line, with the
  entry's wikilinks copied onto it, to `## Logs`, in one transaction. Append-only, like
  `log_to_daily_note`. Rejects a heading that matches any existing heading in that day's note.
  `DailySection` gains a `Notes` arm with append forced, so `upsert_daily_section` can also reach
  the section for callers that want no pointer line.
- **`create_custom_note`** gains an optional **`body`** (with a template, it fills a
  `{{body}}` placeholder, else it is inserted after the H1) and an optional **`origin`** string.
  **Every custom-note creation stages a log line** in its transaction:
  `<type> created [[<folder>/<slug>]] — <title>`, matching the commitment line that exists today.
- **`revise_note(path, expected_hash, revision, reason, at)`** for custom types, where `revision` is
  a whole body or a `(section, content)` upsert. `expected_hash` is `Option<&str>` in the domain,
  compared inside the transaction lock. It refuses built-in types (their sections are owned by
  behaviour) and append-only types, refuses a hash mismatch (lost-update guard), logs
  `revised [[…]] — <reason>` with a section anchor when one section changed, and writes nothing
  when the text is identical. It is the transactional successor of `write_note_raw`.
- **Anchor-aware link resolution** (`cdno-core`, the one core change): `resolve_one` splits the
  target before the first `#`, before the last-segment rule, and resolves the path part. No new
  field on `LinkEntry`, no migration (`target_raw` already holds the anchor). Defined and tested:
  `[[note#]]`, `[[note#^block]]`, `[[folder#Heading]]`.
- **Creation lines for the built-ins.** Project, question, portfolio and stewardship creation
  gain the same `<type> created [[…]] — <title>` line. This is a gap in the history invariant
  independent of concepts and is filled alongside, as its own PR.

### 6.3 CLI (flags-and-prompts, `docs/cli-ergonomics.md`)

No `cdno concept` verb. The generic verbs cover it:

```
cdno note create concept [--title T] [--field tags=…] [--body-file F] [--origin LINKS]
cdno note revise <slug>  [--body-file F | --section H --content-file F] [--reason R]
cdno note list concept
cdno search <query> --type concept
cdno open concept:<slug>
cdno log note            [--heading H] [--body-file F] [--date YYYY-MM-DD]  # ## Notes + pointer line
```

`log note --date` writes to that day's note, stamped at the current time, mirroring the
`note_to_daily` tool's `date`.

`note revise` follows the convention: in an interactive run with `--body-file` absent, it prompts
for the body through `prompt_editor` pre-seeded with the current text, then for `--reason`, and
makes one `revise_note` call with the hash from its own read; non-interactive runs fail with
`missing_flag`. No lock is held while the editor is open; a concurrent change is refused at commit.

### 6.4 MCP

Net **+3 tools** against the #597 baseline (55 → 58): `read_note`, `revise_note`,
`note_to_daily`. `create_custom_note` gains `body` and `origin`; nothing is removed or renamed.

- **`read_note`** (new, in `context_router` so the read-only server has it): path or slug;
  returns frontmatter, body, `content_hash`, backlinks, headings. A reference that matches no note
  is refused with code `not_found`; a slug shared by several notes with `ambiguous_slug`.
- **`revise_note`** (new): as §6.2. At MCP, `expected_hash` is required when `body` is given and
  ignored for `section`. The description states the reason requirement and that the agent drafts
  the reason. A changed note is refused with code `stale_revision`, a malformed section heading or
  a restructuring heading in `content` with `revision_invalid`, and a built-in or append-only
  type with `note_not_revisable`.
- **`note_to_daily`** (new): as §6.2. Its description says substance goes here and the pointer
  line is written for it, that entry headings must not reuse section names, and to end a reusable
  entry with `#concept`.
- **`upsert_daily_section`** accepts `notes` (append forced) and its description says so;
  `operations.rs:604` is corrected.

The **method text** lives where an agent reads it:

- One bullet in the server-level instructions (`cdno-mcp/src/server.rs`), worded conditionally:
  *if the vault declares a `concept` type, understanding you will reuse is a concept, not
  evidence; evidence is dated and never depends on a concept's current text.*
- The `create_custom_note`, `search_notes` and `read_note` descriptions carry the filing test,
  search-before-create, and the promotion offer (§5.5).

---

## 7. Compatibility

- **No migration.** Nothing moves automatically, and the folder fills organically.
- **Existing vaults** gain nothing until they declare the type or write to `## Notes`; the daily
  template is unchanged.
- **Every custom type starts logging its creations**, and the built-ins gain the same line
  (§6.2). Both are `CHANGELOG.md` entries.
- **Tool surface.** Three new MCP tools and one widened input; the #597 differential probe is
  unaffected. The pinned count in `crates/cdno-mcp/tests/server.rs` moves to 58; `README.md` and
  `STATUS.md` follow.
- **Docs-site.** The sentence "`[schemas.<custom>]` has no effect" in `custom-note-types.md` is
  wrong today and is corrected in its own PR.

---

## 8. Implementation plan — staged

The task-level plan, with one issue per task and a verification probe for each, is the companion
document [0002-implementation-plan.md](0002-implementation-plan.md). The stages are:

**Stage 0: anchor-aware links** (T0). The `resolve_one` fix with its tests, and `design.md:587`
rewritten to heading-text form. Independent of everything else; repairs the existing milestone
backlinks.

**Stage 1: generic tooling** (T1 to T12), each a small PR: `DailySection::Notes` and
`note_to_daily`; `read_note` with hash, backlinks and headings; `body` and `origin` on
`create_custom_note` with the creation line for every custom type; creation lines for the
built-ins; `revise_note`; CLI `note revise` and `log note`.

**Stage 2: method text and the type** (T13 to T17). `cdno init` writing the declaration, the
example type, the conditional server bullet and tool descriptions, `design.md`,
`implementation-plan.md` Phase 7 (`zettel → evidence or concept`), `CLAUDE.md`, and the
docs-site pages.

**Stage 3: the trial** (T18). Four to six weeks of use, measured from disk, no telemetry:
backlinks to each concept from notes dated after its `created`; `created [[` and `revised [[`
lines in the log; a hand-read `git log -- concepts/`. Target, not counter: more than half of
concept notes carry a later backlink after six weeks. Three prose questions at the end:

1. Did you re-derive anything the library already had?
2. Were most promotions same-day, or did second-use promotion happen?
3. Did a weekly or monthly review, or a project, miss a derivation that lived only in `## Notes`?

**Stage 4: the decision** (T19). If the trial shows a need no generic operation meets, a
follow-up RFC proposes a built-in. If it does not, the custom type is the design.

**Deferred**, named so they are not forgotten: a tag filter on search (an index-layer join, not a
post-filter); `description` on `[note_types.*]` surfaced through `list_note_types`; headings on
search hits; a heading-rename warning in `revise_note`; ordered-insert on `set_frontmatter`; a
"heading exists" lint for anchors; `was:`/`now:` blocks moving to `## Notes`.

---

## 9. Decisions and open questions

Recorded rulings (maintainer, 2026-09-27, after panel round 2), on which the RFC was accepted:

1. **`## Notes` keeps its pointer line and its tool.** The lean `DailySection::Notes` alone was
   the panel's floor; the maintainer keeps `note_to_daily` in stage 1 because agents have been
   observed editing notes directly when no tool exists for the write they need.
2. **`verified` is cut.** A hand-set date an agent surfaces before a procedure is staleness in
   disguise, and it would force ordered-insert into stage 1.
3. **All creations are logged.** Custom types start now; the built-ins that lack a creation line
   are a gap to fill alongside, not an asymmetry to accept.
4. **`cdno init` writes a live, deletable `concept` declaration.**

Settled by the panel and adopted: name `concept` / `concepts/` (Q1); no revision default, the
call shape decides (Q4); one coarse subfolder level, tool-blind (Q5); the `set_frontmatter`
question is moot with `verified` gone (Q6); disk-only trial measures plus three prose questions
(Q7).

Still open for reviewers:

1. **Pointer-line shape.** `noted [[…#Heading]] ([[links]])` copies the entry's wikilinks in
   parentheses. Is that the right amount, or should the line carry the heading only and leave the
   links to backlinks?
2. **`origin` as one string.** Chosen because list fields are reserved (`list = true` is a load
   error today). Revisit if list fields ship first.
3. **Heading uniqueness.** `note_to_daily` rejects a duplicate heading; should it instead suffix
   (`Woodbury identity (2)`) so a second same-day entry never fails?

---

## 10. Verification (stages 0 to 2)

- `cdno-core`: anchor split cases (`[[note#Heading]]`, `[[note#]]`, `[[note#^block]]`,
  `[[folder#Heading]]`) resolve to the path part; a milestone link now yields a `links` edge.
- `cdno-domain` unit tests (memory store): `note_to_daily` creates `## Notes` before `## Logs`,
  never replaces it, writes entry and pointer line in one transaction, copies the entry's
  wikilinks onto the pointer line, and refuses a heading that matches any existing heading;
  `upsert_daily_section("notes")` appends and never overwrites; `revise_note` logs exactly one
  `revised [[` line with the section anchor when one section changed, writes nothing on identical
  text, refuses a stale hash on whole-body revision, and refuses built-in and append-only types;
  `create_custom_note` with `body` and `origin` stages the `<type> created [[…]]` line in the same
  transaction, and so does every existing custom type; project and question creation log their
  line; `read_note` returns hash-from-bytes, backlinks, frontmatter and headings; concept notes are
  absent from the orientation, weekly and monthly contexts.
- `cdno-cli`: `assert_cmd` wiring for `note revise` and `log note`, including `--no-interactive`
  missing-flag errors, and `note revise` refusing on a concurrent change.
- `cdno-mcp`: handler tests for the three new tools and the widened input, an `e2e_*` round-trip,
  the pinned tool count at 58, and the #597 differential probe showing no existing tool vanished.
- Docs: `mdbook build docs-site` clean; the new page listed in `SUMMARY.md`.
