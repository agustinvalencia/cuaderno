# RFC 0002 — Reference notes

| | |
|---|---|
| **Status** | Draft — for review, nothing decided |
| **Tracked by** | — (no issue yet) |
| **Affects** | `cdno-core`, `cdno-domain`, `cdno-cli`, `cdno-mcp`, `docs-site` |
| **Related** | RFC 0001 (format precedent); custom note types (`docs-site/src/reference/custom-note-types.md`); #597 (desktop retirement — this RFC targets the CLI and MCP surfaces only) |

> **Authorship.** The idea comes from a conversation between the maintainer and Claude (web) on
> 2026-09-02. This document was drafted by Claude Code against the codebase as it stands, for
> review. Nothing in it is a decision yet.

---

## 1. Summary

Add a **reference note**: settled, reusable knowledge you consult to *do* something or *recall*
something. Examples are a Kubernetes rollout recipe, a GPG key-rotation procedure or a sheet of
definitions. Reference notes live in a flat top-level folder (working name `reference/`).

A reference note is **revised in place** when it goes stale, and every revision leaves a one-line
trace in the daily log. It carries a `verified` date. It has no place in orientation or reviews,
and its age is shown only when it is read.

Five points carry the weight:

- **A filing test** that gives each piece of knowledge exactly one home, with a **provenance rule**
  so that revising a reference note never changes what a piece of evidence meant (§5.1).
- **Promotion on second use** as the default habit: the tool makes promotion cheap but never
  enforces it (§5.4).
- **Logged revisions**, so the daily log keeps its role as the record of how mutable notes evolve
  (§5.3).
- **Staleness shown at the point of use**, never as a review chore (§5.5).
- **A staged rollout.** A zero-code trial as a custom note type comes first, and it becomes a
  built-in type once the habit is proven (§8).

---

## 2. Motivation

Some knowledge is neither evidence nor work. Take *"how to do a rolling restart of a StatefulSet
without losing quorum"*:

- It answers no open question and weighs toward no conclusion, so it is not **evidence**.
  Evidence is also append-only, and this knowledge needs correcting when it goes stale.
- It has no deliverable and never completes, so it is not a **project** or an **action**.
- It belongs to no single perpetual responsibility, so it is not a **stewardship** routine.
- In the **daily log** it can be found, but it ends up scattered across several days in several
  partial versions, and the newest is not necessarily the right one.

The cost is **re-derivation**: working the same thing out again because the last answer is buried
or cannot be trusted. A reference note is the single, current, trusted answer.

---

## 3. Background — what the design builds on

- **Twelve built-in types** (`crates/cdno-domain/src/note_type.rs`). `daily`, `weekly`,
  `evidence` and `tracking` are append-only (`NoteType::is_append_only`), with lint enforcing
  frozen prefixes. The others are mutable in place.
- **History preservation.** Replacing a project's `## Current State` writes a `state on [[slug]]`
  entry to the daily log (`vault/projects/state.rs`). The vault's invariant is that no mutable
  section is replaced without a log entry.
- **Custom note types** (`[note_types.<name>]`) give a folder, required and optional fields, a
  template, lint, indexing, full-text search, backlinks and `cdno note`, but no behaviour. They
  are not part of `cdno orient`.
- **Search** ranks title hits ten times above body hits and filters by `note_type`.
  **`cdno open`** resolves bare slugs, type-scoped slugs and paths.
- **MCP reading.** Agents can find any note (`search_notes` returns snippets) and read the daily,
  weekly and monthly notes and project maps. There is no generic note-read tool; the domain's
  `Vault::read_note` exists and stays in the domain (#597).
- **Stewardship `routines/`** hold prescriptive documents that are bound to one stewardship and
  linked from its tracking entries.
- **Frontmatter `tags:`** are always indexed (`note_tags`).
- **Heading wikilinks** (`[[note#heading]]`) are already in use.

---

## 4. Terminology

- **Reference note**: a note of type `reference` (working name; see §9, Q1).
- **Promotion**: creating a reference note from knowledge first worked out in the daily log.
- **Revision**: a change to a reference note's content made through the tool.
- **Verification**: confirming, without changing anything, that a reference note is still correct.

---

## 5. Proposal

### 5.1 The filing test

Ask of the piece of knowledge: *why would I open this again?*

1. **To weigh an open question** → **evidence** in a portfolio.
2. **To do something, or recall something settled**:
   - if it prescribes the practice of **one stewardship** and tracking entries point at it (a
     workout plan, a care routine) → a **stewardship routine**;
   - otherwise → a **reference note**.
3. **Neither yet** (still being worked out) → the **daily log**, until promoted (§5.4).

For example, a benchmark result is evidence, and the procedure for running the benchmark is a
reference note.

**Provenance rule.** Evidence never depends on the *current* text of a reference note. When a
result was produced by a procedure, the evidence note records the exact invocation (command,
version, parameters) inline, and may link to the reference note as *"see also"*. Reference notes
describe how to do things **now**; evidence records what was done **then**. This is what lets a
reference note be revised freely.

### 5.2 Shape

```markdown
---
type: reference
title: Rolling restart of a StatefulSet
created: 2026-09-25
verified: 2026-09-25
tags: [kubernetes]
origin: ["[[journal/2026/daily/2026-09-02]]", "[[journal/2026/daily/2026-09-24]]"]
---

# Rolling restart of a StatefulSet

## When to use
…

## Procedure
…

## Gotchas
…
```

- **Required:** `type`, `title`, `created`, `verified`.
- **Optional:** `tags` (the subject vocabulary: open, indexed, no config); `origin` (wikilinks to
  where the knowledge was worked out, filled in by promotion).
- **Links:** none is required. Links to projects, questions or stewardships go in the body like
  any other wikilink, and a reference note with no links in or out is not a lint finding. It must
  be findable by title and full-text search alone.
- **Body:** free-form. The default template offers *When to use / Procedure / Gotchas*. A
  definitions sheet simply ignores it.

### 5.3 Revision

A reference note is **mutable in place**. Corrections replace the wrong text, so the note is
always the current answer.

Every revision made through the tool writes **one line to today's daily log**:

```
- **14:32**: revised [[reference/rolling-restart-statefulset]] — quorum check moved before drain
```

- The **reason is required**. It is the part of the history worth keeping.
- There is **no `was:`/`now:` block**. A procedure runs to dozens of lines, and the log records
  *when* and *why*, not the full text. Users who want full diffs keep the vault under git.
- `revised [[` is a fixed prefix, like `state on [[`, that the context reader may parse. It is
  never written in any other shape.
- Revision bumps `verified` to today.
- There is **no append operation** for reference notes. Revision replaces the whole body or
  upserts one section.

Edits made outside cuaderno (in an editor) are legitimate, since markdown is the source of truth,
but they leave no log line. The CLI's `--edit` flow (§6.3) runs an editor session through revision
so that it does.

### 5.4 Promotion

The default habit: **work it out in the log first, and write the reference note the second time
you go looking for it.** A single lookup does not justify a note; two do.

The tool supports the habit without enforcing it:

- **Promote** creates a reference note with `origin:` pre-filled from the daily notes it came
  from, and logs `promoted to [[reference/<slug>]]`, in one transaction.
- **Create** without an origin is always allowed, for knowledge you already know you will reuse.
- **Triage** can route an inbox capture into a new reference note.
- **Spotting the second use** belongs to the agent layer. A skill (`examples/skills/`) that finds
  the same topic in two daily notes can suggest promotion. The tool itself never counts.

### 5.5 Staleness

- `verified` is set on creation and bumped by **verify** (the procedure was followed and still
  works; no log line) and by **revise**.
- Reference notes **never** appear in `cdno orient`, the weekly or monthly context, or any review
  list. There is no staleness lint.
- Whenever a reference note is **read** (MCP `read_note`, or the header `cdno open` shows), its
  age since `verified` is shown with it. Past a threshold (working value 365 days, configurable)
  this becomes a caveat: *"last verified 14 months ago"*. The MCP tool description tells the agent
  to pass the caveat on before the user follows a procedure.

### 5.6 Layout

- A flat `reference/` folder, with at most one coarse subfolder level (`reference/tooling/`) once
  it grows unwieldy (around 40 files). This is a habit, not enforced.
- Wikilinks use the qualified form `[[reference/<slug>]]`. Lint warns when two notes in different
  subfolders share a stem, since that makes a bare `[[slug]]` ambiguous.
- Fewer, larger notes are preferred, especially for mathematics. Sections are cited with heading
  links: `[[reference/linear-algebra-notation#Pseudo-inverse]]`.
- No cap on the number of notes.

---

## 6. Detailed design (built-in stage)

### 6.1 Core

- `paths.rs`: `pub const REFERENCE: &str = "reference";`, added to `RESERVED_TOP_LEVEL_FOLDERS`.
  `cdno init` creates the folder and reconciliation walks it.
- No mutability policy in core. No index migration is expected, since `note_type` is free text and
  `tags` and dates are already indexed. This needs confirming during implementation.

### 6.2 Domain

- `NoteType::Reference`, with `is_append_only() == false`, plus a default template under
  `crates/cdno-domain/templates/`.
- One file per operation under `src/vault/reference/`, each going through `VaultTransaction`:
  - `create_reference(title, body, tags, origin)`: a non-empty `origin` makes it a promotion,
    which also logs `promoted to [[…]]`;
  - `revise_reference(slug, reason, Revision)`, where `Revision` is a whole body or a
    `(section, content)` upsert. It bumps `verified` and logs `revised [[…]] — <reason>`. An
    identical revision writes nothing;
  - `verify_reference(slug)` bumps `verified` only;
  - `list_references(tags, stale)`;
  - a read view carrying `verified` and `days_since_verified`, so every surface reports age the
    same way.

### 6.3 CLI (flags-and-prompts, `docs/cli-ergonomics.md`)

```
cdno reference new     [--title T] [--tag K]… [--origin DATE|PATH]…   # alias: cdno ref
cdno reference revise  [--slug S] [--reason R] (--body-file F | --edit)
cdno reference verify  [--slug S]
cdno reference list    [--tag K] [--stale]
```

- `new --origin` is the promotion path.
- `revise --edit` opens `$EDITOR` on a temporary copy and commits the result through
  `revise_reference`, so an editor session still leaves its log line.
- Reading uses `cdno open` (which shows the verified age) and `cdno search --type reference`.

### 6.4 MCP

- `create_reference`, `revise_reference`, `verify_reference`, `list_references`. Their
  descriptions state the filing test and the provenance rule (§5.1), because tool descriptions are
  the only instruction surface an agent sees.
- A generic **`read_note(path|slug)`** over the retained `Vault::read_note`. It serves every note
  type and is required here, because a reference note must be readable, not just findable. For
  reference notes it adds `verified` and `days_since_verified`.
- `search_notes` needs no change beyond accepting the new type name.

---

## 7. Compatibility

- **No migration.** Nothing moves automatically, and the folder fills organically.
- **Hand-over from the trial.** A vault that declared `[note_types.reference]` during stage 0
  (§8) would be rejected at open once the built-in exists, because custom names may not shadow
  built-ins. For one release, a `[note_types.reference]` whose folder is `reference` is ignored
  with a warning, and lint suggests removing it. Trial notes already carry `type: reference` in the
  right folder, so no note is rewritten. The trial template uses the field names of §5.2.
- **Tool surface.** The new CLI verbs and MCP tools are purely additive, so the #597 no-regression
  baseline is unaffected.

---

## 8. Implementation plan — staged

**Stage 0: trial, zero code.** Ship `examples/note-types/reference/` (a config snippet plus a
template with §5.2's fields) and a docs-site recipe. Use it for four to six weeks and observe:
does promotion happen, how many notes appear and in what shape, is `tags` enough, and how much
the missing behaviours (revision logging, age display) matter in practice.

**Stage 1: `read_note` MCP tool.** Useful for every note type, and it makes the trial usable from
an agent.

**Stage 2: built-in type.** Once stage 0 shows the habit takes: §6.1 to §6.4, the hand-over (§7),
and updates to `CHANGELOG.md`, `STATUS.md`, the design.md type table and a docs-site concepts page.

**Stage 3: skills.** A promotion-suggesting skill in `examples/skills/`, and the filing test added
to the shared linking and capture references.

---

## 9. Questions for reviewers

1. **Name.** `reference/` is clear, but the word already names the docs-site's CLI reference and
   design.md's "stable reference" lifecycle category. Candidates: `handbook/` (fits the *do or
   recall* purpose), `shelf/`, `know-how/`, or a Spanish term (`apuntes/`, `recetario/`). The
   author leans towards **`handbook/`**. `reference` is used here as a working name.
2. **Revision trace.** Is the one-line `revised [[…]] — reason` enough, or should the log also
   keep the previous text of the changed section when a revision touches just one section?
3. **Provenance rule enforcement.** Guidance in tool descriptions only, or also a lint check on
   evidence notes that link to a reference note?
4. **Staleness threshold.** Is 365 days right? Should it be set per vault, or per note (for
   example a `review_after` field)? Or should age always be shown, with no threshold?
5. **Attachments.** Can a reference note own files (diagrams, config templates), for example
   through the portfolio's stub-plus-folder model extended to `reference/`? The author leans
   towards **not in v1**, since code blocks cover most cases.
6. **Routines.** Should stewardship routines stay a separate concept (the author's lean), or
   become reference notes linked from their stewardship?
7. **Built-in type or custom-type capabilities.** Stage 2 adds a thirteenth built-in. An
   alternative is to give custom types opt-in behaviours (`revision_log = true`,
   `verified_field = "verified"`) and ship `reference` as a bundled custom type. The author leans
   towards **built-in**, because the filing test becomes part of the method itself.

---

## 10. Verification (stage 2)

- `cdno-domain` unit tests (memory store): revise logs exactly one `revised [[` line and bumps
  `verified`; an identical revise writes nothing; promote fills `origin` and logs; verify writes
  no log line; reference notes are absent from the orientation, weekly and monthly contexts.
- `cdno-core`: `init` creates `reference/`, reconciliation indexes it, and a custom type cannot
  claim the folder.
- `cdno-cli`: `assert_cmd` wiring for each verb, including `--no-interactive` missing-flag errors.
- `cdno-mcp`: handler tests for the new tools and `read_note`, an `e2e_*` round-trip, and the #597
  differential probe showing no existing tool vanished.
- Lint: a duplicate-stem warning across `reference/` subfolders, and no staleness finding.
- Hand-over: a vault declaring `[note_types.reference]` opens with a warning, not an error.
