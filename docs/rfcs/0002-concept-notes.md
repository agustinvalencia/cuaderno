# RFC 0002 — Concept notes

| | |
|---|---|
| **Status** | Draft — for review, nothing decided |
| **Tracked by** | — (no issue yet) |
| **Affects** | `cdno-domain`, `cdno-cli`, `cdno-mcp`, `docs/design.md`, `docs-site`, `examples/` |
| **Related** | RFC 0001 (format precedent); custom note types (`docs-site/src/reference/custom-note-types.md`); #597 (desktop retirement — this RFC targets the CLI and MCP surfaces only); `docs/implementation-plan.md` Phase 7 (the "standalone note" this RFC resolves) |

> **Authorship.** The idea comes from a conversation between the maintainer and Claude (web) on
> 2026-09-02, and the shape here follows a review-panel round on the pull request and the
> maintainer's reframing of the problem. Drafted by Claude Code against the codebase as it stands.
> Nothing in it is a decision yet.

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
- **One concept per note**, linked, with structure emerging from links, tags and hand-written
  hub notes rather than from folders (§5.2, §5.7).
- **Logged refinement**, so the daily log keeps its role as the record of how mutable notes evolve
  (§5.3).
- **A `## Notes` section in the daily note** for substance, so `## Logs` stays a sequence, and **promotion from it** as the default habit, cheap but never enforced (§5.4, §5.5).
- **A staged rollout** on generic tooling: a custom note type plus four small generic operations,
  a trial with exit criteria, and a built-in type only if the trial shows a need no generic
  operation meets (§6, §8).

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
  versions, and the version you find is not necessarily the best one.

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
  section is replaced without a log entry.
- **Custom note types** (`[note_types.<name>]`) give a folder, required and optional fields, a
  template, lint, indexing, full-text search, backlinks and `cdno note`, but no behaviour, and
  they are outside `cdno orient` and the reviews. That last property is exactly right for a
  concept library.
- **Links are indexed generically** (`links` table, `cdno-core/migrations/001_initial.sql`), and
  the domain already answers backlinks for one type (`question_backlinks`,
  `vault/context.rs`). **Tags** are indexed from frontmatter and body. **Heading wikilinks**
  (`[[note#heading]]`) are in use.
- **`set_frontmatter`** writes a field declared `settable = true` under `[schemas.<type>.fields]`
  through the index, optionally logging the change (`vault/set_frontmatter.rs`). Whether that
  path is exercised for a custom type is unverified (§9).
- **Search** ranks title hits ten times above body hits and filters by `note_type`. It has no tag
  filter. **`cdno open`** resolves bare slugs, type-scoped slugs and paths.
- **MCP reading and editing.** `search_notes` returns snippets; the daily, weekly and monthly
  notes and project maps have read tools; nothing else is readable. `create_custom_note` takes
  `type_name`, `title`, `fields` and `vars` but **no body**, and no tool edits a custom note's
  body. The domain's `Vault::read_note` exists and is retained under #597.

---

## 4. Terminology

- **Concept note**: a note of type `concept`, in `concepts/`.
- **Hub note**: a concept note whose content is mostly links: a map of a cluster of concepts.
- **Refinement**: a change to a concept note's content made through the tool.
- **Promotion**: creating a concept note from understanding first worked out in the daily log.

---

## 5. Proposal

### 5.1 The filing test

Ask of the thing you want to keep: *what is it?*

1. **A dated observation, result or source that bears on an open question** → **evidence** in a
   portfolio.
2. **The answer to a question you weighed evidence for** → the **question note**, with
   `status: answered`. A concept may link to it, but does not restate it.
3. **A prescribed practice that tracking entries record against** (a workout plan, a care
   routine) → a **stewardship routine**. Structurally: a note is a routine if and only if a
   tracking entry points at it through `routine:`.
4. **Understanding you will reuse, independent of any deliverable**: a theorem, a definition, a
   technique, a procedure, an idea → a **concept note**.
5. **Not yet settled** (still being worked out) → the **daily log**, until promoted (§5.4, §5.5).

A benchmark result is evidence. What the benchmark taught you about the technique is a concept.
The command that ran it is part of the evidence note, and may also become a concept note if you
will run it again.

**Provenance rule.** Evidence never depends on the *current* text of a concept note. An evidence
note records what was done and observed inline (the exact invocation, the version, the
parameters), and may link to a concept as *"see also"*. Concepts describe understanding **now**;
evidence records what happened **then**. This is what lets a concept note be refined freely.

### 5.2 Shape

```markdown
---
type: concept
title: Woodbury identity
created: 2026-09-25
tags: [linear-algebra, optimisation]
origin: ["[[journal/2026/daily/2026-09-02#Woodbury identity]]", "[[journal/2026/daily/2026-09-24#Low-rank refit]]"]
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

- **Required:** `type`, `title`, `created`.
- **Optional:** `tags` (the subject vocabulary: open, indexed, no config); `origin` (wikilinks to
  where it was worked out, filled in by promotion); `verified` (a date, by convention only for
  procedural concepts: the day you last ran the procedure and it worked; §5.6).
- **Links:** none is required. A concept with no links in or out is not a lint finding. Links to
  projects, questions, stewardships and other concepts go in the body like any other wikilink.
- **Unit:** one concept per note. If a note needs two headings that could each be cited on their
  own, it is usually two notes. Fewer, longer notes are acceptable for a single derivation that
  does not split; sections are then cited with heading links
  (`[[concepts/woodbury-identity#Statement]]`).
- **Body:** free-form. The default template offers *Statement / Why it matters / See also*, and a
  procedure or a definition simply uses different headings.

### 5.3 Refinement

A concept note is **mutable in place**. A better explanation replaces the worse one; a correction
replaces the error. The note is always the current best account.

Every refinement made through the tool writes **one line to today's daily log**:

```
- **14:32**: revised [[concepts/woodbury-identity#Why it matters]] — added the cost comparison
```

- The **reason is required**. It is the part of the history worth keeping.
- The section anchor is present when a single section was revised.
- There is **no `was:`/`now:` block**. The log records *when* and *why*, not the previous text.
  Users who want full diffs keep the vault under git. `docs/design.md` §7 and `CLAUDE.md` name
  this as an explicit exception to the `was:`/`now:` shape.
- `revised [[` is a fixed prefix, like `state on [[`, that the context reader may parse. It is
  never hand-written in any other shape.
- There is **no append operation** for concept notes. Refinement replaces the whole body or
  upserts one section, and section upsert is the default.

Edits made outside cuaderno (in an editor) are legitimate, since markdown is the source of truth,
but they leave no log line. That is stated as a limit of the invariant, not enforced by lint.

### 5.4 Where a concept starts: the daily note's `## Notes`

Today everything written during the day lands in `## Logs`, and substance (a derivation, a
meeting, a worked-out procedure) ends up as a block of text inside what is meant to be a
*sequence*. The daily module already anticipates a second history section, `## Notes`, that is
append-only like `## Logs` and outside the `upsert_daily_section` allow-list
(`vault/daily.rs`); it is not yet written. This RFC gives it its job:

- **`## Logs` is the sequence.** One line per event, pointers only.
- **`## Notes` is the substance.** Append-only. Each entry is its own `### ` heading, so it can
  be addressed: `### Woodbury identity`, `### Meeting with Erik`. A candidate concept is an
  entry tagged `#concept` in its body; since body tags are indexed, "the same thing on two or
  more dates" is a tag search across daily notes.
- **The log line points at the entry**: `- **14:32**: noted [[2026-09-27#Woodbury identity]]`.
  `noted [[` is a fixed prefix in the family of `state on [[` and `revised [[`.

```markdown
## Logs
- **14:32**: noted [[2026-09-27#Woodbury identity]]

## Notes

### Woodbury identity
#concept
(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1 — cheap when A^-1 is known and the
update is low-rank; today's use was the k=3 refit.
```

The `was:`/`now:` blocks that project state changes write are outside this RFC, but they are the
same problem and would fit the same section later.

**Heading links must resolve.** `[[note#Heading]]` is the Obsidian form and is already used by
`docs/design.md` for milestone links, but the resolver (`resolve_one`,
`cdno-core/src/extractors.rs`) matches the whole target including the anchor, so every heading
link is a dangling link today. Splitting the target on the first `#` before matching is a
prerequisite for this section and for `origin:` below, and is a small fix in its own right.
Nested anchors (`[[note#A#B]]`) are not supported: `MarkdownDocument::section()` addresses
headings by flat text, so entries under `## Notes` use unique `### ` headings instead.

### 5.5 Promotion

The default habit: **work it out in `## Notes` first, and write the concept note the second time
you reach for it.** One use does not justify a note; two do. The tool supports the habit without
enforcing it:

- **Promote** creates a concept note from a `## Notes` entry, with `origin:` pre-filled with the
  entries it came from (`[[journal/2026/daily/2026-09-27#Woodbury identity]]`, naming the entry
  rather than the day) and a drafted body, and logs `promoted to [[concepts/<slug>]]`, in one
  transaction. No past daily note is touched; the frontmatter links count as backlinks.
- **Create** without an origin is always allowed, for understanding you already know you will
  reuse.
- **Triage** can route an inbox capture into a new concept note.
- **Spotting the second use** belongs to the agent. The `search_notes` and `read_note`
  descriptions tell an agent to offer promotion when `#concept` entries on the same subject turn
  up in daily notes on two or more dates, and to search before creating so it extends an existing
  concept rather than minting a duplicate. The tool never counts.

### 5.6 No staleness

Understanding does not go stale. A theorem is as true untouched after two years as on the day it
was written, and an explanation that has not changed is one you were happy with. Concept notes
therefore have **no `verified` requirement, no age signal, no threshold, no staleness lint and no
review chore**. They never appear in `cdno orient`, the weekly or monthly context, or any review
list.

Two things are available on request, and nothing acts on them:

- **Procedural concepts** may carry an optional `verified` date by convention. When the field is
  present, `read_note` returns it; the agent mentions it only when the user is about to follow the
  procedure. When absent, nothing is said.
- **Usefulness** is a question you can ask, not a metric that is kept: `read_note` returns the
  note's backlinks (already in the `links` table), and "mentioned in the log" is a search. An
  unreferenced concept costs nothing and is left alone.

### 5.7 Layout — flat, structured by links

- A flat `concepts/` folder. A concept usually belongs to several subjects at once (a theorem is
  both linear algebra and optimisation), so a folder is the wrong axis; **tags** carry
  multi-membership, and wikilinks carry relationship.
- **Hub notes** carry navigation. When a cluster forms, you write a concept note that is mostly
  links (`concepts/linear-algebra.md`: the theorems, the notation sheet, the techniques). It is
  an ordinary concept note, created by hand when the cluster is real, never a folder decided in
  advance. It is the same pattern as a portfolio's `_index.md`.
- **At most one coarse subfolder level** is permitted (`concepts/maths/`), for people who browse
  the folder on disk once it passes a few hundred files. It is a courtesy to the file browser,
  not a taxonomy; nothing in the tool depends on it, and wikilinks stay `[[concepts/<slug>]]`.
- Wikilinks use the qualified form `[[concepts/<slug>]]`. Duplicate stems across a subfolder are
  left to the existing ambiguous-link resolution.
- No cap on the number of notes.

---

## 6. Detailed design

The concept type needs no behaviour of its own. What it needs is four small **generic**
operations that every custom type lacks today, plus the method text in the places an agent reads.

### 6.1 The type

`examples/note-types/concept/` ships a config snippet and a template:

```toml
[note_types.concept]
folder = "concepts"
required = []
optional = ["tags", "origin", "verified"]
template = "concept.md"
title_field = "title"

[schemas.concept.fields.verified]
type = "date"
settable = true
log_on_change = false
```

Whether `cdno init` writes this into a new vault's config by default, or the docs offer it, is
open (§9). Nothing changes in `cdno-core`: the folder is not reserved, and reconciliation and the
index already handle a custom type.

### 6.2 Generic domain operations (`cdno-domain`)

One file per operation under `src/vault/`, each through `VaultTransaction`:

- **`read_note(path | slug)`** (already exists, retained under #597): extended to return a
  `content_hash`, the note's backlinks, and its frontmatter, so a caller can revise safely and
  answer "is this used".
- **`create_custom_note`** gains an optional **`body`** (today it renders the template only) and an
  optional **`log_line`**, so promotion is one call: body plus `origin` plus the `promoted to`
  entry in one transaction.
- **`revise_note(path, expected_hash, revision, reason)`** for custom types, where `revision` is a
  whole body or a `(section, content)` upsert. It refuses built-in types (their sections are owned
  by behaviour) and append-only types, refuses when `expected_hash` does not match the file
  (lost-update guard), logs `revised [[…]] — <reason>` with a section anchor when one section
  changed, and writes nothing when the text is identical.
- **`search`** gains a **tag filter**, since tags are indexed but not queryable.
- **`note_to_daily(date, heading, body)`** appends a `### heading` entry to `## Notes` (creating
  the section after `## Logs` if absent) and the `noted [[<date>#<heading>]]` line to `## Logs`,
  in one transaction. Append-only, like `log_to_daily_note`; it rejects a heading that already
  exists in that day's `## Notes`, so anchors stay unique.
- **Anchor-aware link resolution** (`cdno-core`, the one core change): `resolve_one` splits the
  target on the first `#` and matches the path part; the anchor is kept on the `LinkEntry` for a
  later lint check that the heading exists.

Verification of a procedural concept is the existing `set_frontmatter` on `verified`, with the
`[schemas.concept.fields.verified]` declaration above; it needs a test proving the path works for
a custom type.

### 6.3 CLI (flags-and-prompts, `docs/cli-ergonomics.md`)

No `cdno concept` verb. The generic verbs cover it:

```
cdno note create concept [--title T] [--field tags=…] [--body-file F] [--origin DATE|PATH]…
cdno note revise         [--note S] [--reason R] (--body-file F | --section H --content-file F | --edit)
cdno note list concept   [--tag K]
cdno search <query> --type concept [--tag K]
cdno open concept:<slug>
```

`--origin` is the promotion path. `--edit` reads the note, releases the lock, opens `$EDITOR` on
a copy, then commits through `revise_note` with the hash it read, prompting for the reason after
the editor closes; a conflicting edit in between is refused rather than overwritten.

### 6.4 MCP

Against the #597 baseline:

- **`read_note`** (new): path or slug; returns frontmatter, body, `content_hash`, backlinks.
- **`revise_note`** (new): as §6.2; the description states the reason requirement and that the
  agent drafts the reason.
- **`note_to_daily`** (new): as §6.2. Its description tells the agent that substance goes here
  and the log line is written for it, and to tag a candidate concept `#concept`.
- **`create_custom_note`** gains `body` and `log_line`; **`search_notes`** gains `tag`. No new
  tool.

That makes net **+3 tools**, not +2, with `note_to_daily` the one that every daily-note writer
benefits from regardless of concepts.

The **method text** lives where an agent reads it:

- One bullet in the server-level instructions (`cdno-mcp/src/server.rs`), alongside "projects
  end, stewardships do not": *understanding you will reuse is a concept, not evidence; evidence is
  dated and never depends on a concept's current text*.
- The `create_custom_note`, `search_notes` and `read_note` descriptions carry the filing test,
  the search-before-create rule and the promotion offer (§5.5).
- `list_note_types` surfaces an optional `description` declared on `[note_types.*]`, so a vault's
  own types can explain themselves to an agent.

---

## 7. Compatibility

- **No migration.** Nothing moves automatically, and the folder fills organically.
- **No hand-over.** Because the type is a custom type, a later built-in (if the trial calls for
  one) is a separate RFC with its own migration, and nothing here is thrown away: the generic
  operations serve every custom type regardless.
- **Tool surface.** Two new MCP tools and two widened inputs; nothing is removed or renamed, so
  the #597 differential probe is unaffected.
- **`docs-site/src/reference/custom-note-types.md`** currently says `[schemas.<custom>]` has no
  effect. Once `set_frontmatter` on a custom type is tested, that sentence is corrected.

---

## 8. Implementation plan — staged

**Stage 0: anchor-aware links.** The `resolve_one` fix, with lint's dangling-link check
updated, so `[[note#Heading]]` resolves. Independent of everything else and fixes the existing
milestone links; it goes first because `## Notes` entries and `origin:` depend on it.

**Stage 1: generic tooling.** `note_to_daily`; `read_note` with hash and backlinks; `body` and
`log_line` on `create_custom_note`; `revise_note`; the `tag` filter; the `description` on custom
types; a test of `set_frontmatter` on a custom type. Each is useful beyond concepts and is a small
PR.

**Stage 2: method text and the example type.** The server-instruction bullet, the tool
descriptions, `examples/note-types/concept/`, the `design.md` §3 table row and §5 filing test,
the `design.md` §7 and `CLAUDE.md` exception, and a docs-site concepts page that also resolves the
Phase 7 "standalone note" wording.

**Stage 3: the trial.** Four to six weeks of use with **exit criteria written before it starts**.
The question the trial answers is narrow: *does promotion-on-second-use produce notes you
actually reopen?* Proposed measures: concept notes created; of those, how many were reopened
(`read_note` or `cdno open`) after creation; refinements through `revise_note` versus edits
detected by mtime with no log line; promotion offers made versus accepted; duplicates found.

**Stage 4: the decision.** If the trial shows a need no generic operation meets, a follow-up RFC
proposes a built-in. If it does not, the custom type is the design, and this RFC is marked
accepted as is.

---

## 9. Questions for reviewers

1. **Name.** `concepts/` and `type: concept` are proposed. Alternatives: `library/`, `ideas/`,
   `apuntes/`. `zettel` is accurate but carries baggage and is a word the design retired.
2. **`cdno init`.** Should a new vault get the `concept` type by default, or should the docs offer
   it? A default makes the method's answer to "where does understanding go" visible from day one;
   an offer keeps `init` minimal.
3. **Promotion as one call.** `log_line` on `create_custom_note` keeps promotion atomic. The
   alternative is a skill that calls create and then `append_to_log`, which loses the transaction.
   Is atomicity worth the wider input?
4. **Section upsert as default.** For a concept note that is mostly prose without stable headings,
   whole-body revision may be the common case. Should the default follow the note's shape?
5. **Subfolder level.** Keep the one permitted coarse level, or forbid subfolders and rely on hub
   notes alone?
6. **`set_frontmatter` on custom types.** The code path looks generic but has no test; if it does
   not work, the optional `verified` convention needs a small fix before stage 2.
7. **Trial measures.** Are "reopened after creation" and "revised through the tool versus edited
   outside it" the right numbers, and what thresholds count as the habit taking?

---

## 10. Verification (stages 1 and 2)

- `cdno-domain` unit tests (memory store): `revise_note` logs exactly one `revised [[` line,
  with the section anchor when one section changed; an identical revision writes nothing; a stale
  `expected_hash` is refused and the file is untouched; built-in and append-only types are refused;
  `create_custom_note` with `body` and `log_line` writes note and log line in one transaction;
  `read_note` returns the hash, backlinks and frontmatter; `set_frontmatter` sets `verified` on a
  custom type; concept notes are absent from the orientation, weekly and monthly contexts.
- `cdno-cli`: `assert_cmd` wiring for `note revise`, including `--no-interactive` missing-flag
  errors, and `--edit` refusing on a concurrent change.
- `cdno-mcp`: handler tests for `read_note` and `revise_note`, the widened inputs, an `e2e_*`
  round-trip, and the #597 differential probe showing no existing tool vanished.
- Docs: `mdbook build docs-site` clean; the new page listed in `SUMMARY.md`.
