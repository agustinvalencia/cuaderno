# RFC 0002 — Reference notes

| | |
|---|---|
| **Status** | Draft — for review, nothing decided |
| **Tracked by** | — (no issue yet) |
| **Affects** | `cdno-core`, `cdno-domain`, `cdno-cli`, `cdno-mcp`, `cdno-tauri`, `ui`, `docs-site` |
| **Related** | RFC 0001 (format precedent); custom note types (`docs-site/src/reference/custom-note-types.md`) |

> **Authorship and provenance.** The idea started in a conversation between the maintainer and
> Claude (web) on 2026-09-02 ("Organizing general reference information in RLM portfolios").
> There, Claude proposed a `reference/` container, the filing test and the promotion-on-second-use
> rule. The maintainer asked for it to be written up, and it ended as a draft that was never filed
> or ruled on. This document is a **critical re-draft** by Claude Code against the codebase as it
> stands. It keeps what holds up, changes what does not (§4), and lists what reviewers need to
> settle (§9). Nothing in it is a decision yet.

---

## 1. Summary

Add a **reference note**: settled, reusable knowledge you consult to *do* something or *recall*
something. Examples are a Kubernetes rollout recipe, a GPG key-rotation procedure or a sheet of
definitions. Such a note is neither evidence for an open question nor part of a project, and it
lives in a flat top-level folder (working name `reference/`).

It differs from everything else in the vault in how it changes. It is **revised in place** when it
goes stale, and every revision leaves a one-line trace in the daily log. It carries a `verified`
date. It has no place in orientation or reviews, and its age is shown only when you open it.

Five points carry the weight:

- **A filing test** that puts a piece of knowledge in exactly one home, with a **provenance rule**
  so that rewriting a reference note never changes what a piece of evidence meant (§5.1).
- **Promotion on second use** stays the default habit, but it is a **convention, not a gate**. The
  tool makes promoting cheap and never stops you creating a note directly (§5.4).
- **Revisions are logged.** This keeps the vault invariant that a mutable note's history can be
  traced through the daily log (§5.3).
- **Staleness is shown at the point of use, never as a chore** (§5.5).
- **A staged rollout.** A zero-code trial as a custom type comes first. Only once the habit is
  proven does it become a built-in type (§8).

---

## 2. Motivation

Some knowledge has no good home in the vault today. Take *"how to do a rolling restart of a
StatefulSet without losing quorum"*:

- It is not **evidence**. It answers no open question and weighs toward no conclusion. Filing it
  into a portfolio pollutes the dossier, and evidence is append-only, so the day the procedure
  changes you end up appending a correction under a wrong recipe.
- It is not a **project** or an **action**. It has no deliverable and never completes.
- It is not a **stewardship** routine unless it belongs to one perpetual responsibility (§5.1).
- Left in the **daily log**, it is findable (`cdno search … --type daily`) but scattered. The
  third time you need it you are reading four half-versions across three months, and the newest is
  not necessarily the right one.

The common symptom is **re-derivation**: working the same thing out again because the last answer
is buried or untrustworthy. A reference note is meant to be the single, current, trusted answer.

---

## 3. Background — what exists today

The following affects the design:

- **Twelve built-in types** (`crates/cdno-domain/src/note_type.rs`). Of these, `daily`, `weekly`,
  `evidence` and `tracking` are append-only (`NoteType::is_append_only`), with lint enforcing
  frozen prefixes. `project`, `action`, `question`, `stewardship`, `portfolio` and `monthly` are
  mutable in place.
- **History preservation.** Replacing a project's `## Current State` writes a `state on [[slug]]`
  entry with `was:` / `now:` lines to the daily log (`vault/projects/state.rs`). The stated
  invariant is that no mutable section is replaced without emitting its log entry.
- **Custom note types** (`[note_types.<name>]`) are schema-only. They get a folder, required and
  optional fields, a template, lint, indexing, full-text search, backlinks and `cdno note`. They
  get **no behaviour**, and they are already invisible to `cdno orient` and the reviews. Their
  `append_only` flag is accepted but not yet enforced.
- **Search** ranks title hits ten times above body hits, filters by `note_type` (built-in or
  custom) and returns snippets. **`cdno open`** resolves bare slugs, type-scoped slugs and paths.
- **The MCP surface has no generic "read a note" tool.** `Vault::read_note` exists in the domain
  and the desktop app uses it, but an agent can only *find* a note (`search_notes`, snippet only)
  and read the daily, weekly and monthly notes and project maps. This is a real gap for a note
  type whose whole purpose is to be consulted (§6.4).
- **Stewardship `routines/`** already hold "prescriptive reference documents" (design §4), which
  tracking entries link to. They are the nearest existing concept.
- **The frontmatter `tags:` list** is always indexed into `note_tags` (`cdno-core/SCHEMA.md`).
- **Heading wikilinks** (`[[note#heading]]`) are already used, for example by action notes
  pointing at milestones.
- **The desktop free-edit save** (`write_note_raw`) bypasses both the transaction and the log. It
  is a documented exception that must not be extended.

---

## 4. Critique of the 2 September draft

| Draft position | Assessment | This RFC |
|---|---|---|
| A fourth container, `reference/`, serving both tracks | **Keep.** The gap is real (§2). | §5 |
| Filing test: open it to answer a question → evidence; to do or recall → reference | **Keep, but incomplete.** It does not settle routines, and it ignores provenance: "the command that produced the benchmark is reference" is dangerous if the evidence only *links* to a note that is later rewritten. | §5.1 adds a tie-break and a provenance rule |
| Maintained in place: overwrite, don't append corrections | **Keep the intent.** As written it breaks the history-preservation invariant, because a silent overwrite leaves no trace in the log. | §5.3: every revision logs one line |
| "Append is rejected at the core layer" | **Change.** Business rules do not belong in `cdno-core` (layering rule: core is mechanics, not policy). There is also no generic append operation to reject. The real point, *no correction-by-accretion*, is better served by what the tool offers (revise) than by what it forbids. | §5.3, §6.2 |
| Promotion on second use, the load-bearing rule | **Keep as the default habit, not as a gate.** A second lookup that fails to find the first silently resets the counter. Some knowledge (an onboarding setup, a one-off migration you *know* you will repeat) deserves a note on first use. The tool should make promotion cheap and never enforce the count. | §5.4 |
| No review obligation; staleness shows itself on failure | **Keep, and refine.** "Fails in use" is costly for procedures that fail *dangerously* (key rotation, backups). A passive age signal at the point of use costs nothing and adds no chore. | §5.5 |
| No required links or anchor | **Keep.** | §5.2 |
| Flat layout, one coarse level at around 40 files; fewer, larger documents | **Keep.** Add: large notes are cited by heading link, and bare-slug collisions across subfolders are a lint warning. | §5.6 |
| No cap | **Keep.** A cap exists for projects because of attention; reference notes cost none. | — |
| `verified` date stamp | **Keep.** A plain revision bumps it too, so there is one date rather than two. | §5.5 |
| New built-in type straight away, `cdno reference` / `cdno ref`, MCP tools | **Stage it.** The riskiest assumption is behavioural (will the promotion habit take?), not technical. A custom type tests it at zero cost. | §8 |
| Open question: domain vocabulary, open or declared | **Resolve by reuse.** Use the existing `tags:` list. No new vocabulary and no config. | §5.2 |
| No migration | **Keep.** | §7 |

---

## 5. Proposal

### 5.1 What is a reference note — the filing test

Ask of the piece of knowledge: *why would I open this again?*

1. **To weigh an open question** → **evidence** in a portfolio.
2. **To do something, or recall something settled**:
   - if it prescribes the practice of **one stewardship** and tracking entries point at it (a
     workout plan, a care routine) → a **stewardship routine**;
   - otherwise → a **reference note**.
3. **Neither yet** (you are still working it out) → the **daily log**. It stays there until
   promoted (§5.4).

**Provenance rule.** Evidence must never depend on the *current* text of a reference note. When a
result was produced by a procedure, the evidence note records the exact invocation (command,
version, parameters) inline. It may also link to the reference note, but only as *"see also"*.
Reference notes describe how to do things **now**; evidence records what was done **then**.
Without this rule, fixing a recipe silently rewrites the history of every result that cited it.

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
- **Optional:** `tags` (the domain vocabulary; open and indexed, no config), `origin` (wikilinks
  to where it was worked out, filled in by promotion). Links to projects, questions or
  stewardships go in the body like any other wikilink. None is required, and a reference note with
  no links in or out is not a lint finding.
- **Body:** free-form. The default template offers *When to use / Procedure / Gotchas*, and a
  definitions sheet simply ignores it.

### 5.3 How it changes — revise, and leave a trace

A reference note is **mutable in place**. Corrections replace the wrong text; they are not
appended below it. The point is that the note is always the current answer.

To keep the history invariant, **every revision made through the tool writes one line to today's
daily log**:

```
- **14:32**: revised [[reference/rolling-restart-statefulset]] — quorum check moved before drain
```

The reason is **required**. Unlike a project state change, there is **no `was:`/`now:` block**: a
procedure is dozens of lines, and pasting it into the log twice would bury the day's writing.
Tracing a note's evolution is a log search for `revised [[reference/<slug>]]`, which yields *when*
and *why*. The old text is not kept by cuaderno. Users who want full diffs keep the vault in git,
as many already do.

The line starts with `revised [[`. Like `state on [[`, it is a fixed shape the context reader may
parse, and it must never be hand-written in any other form.

Edits made outside the tool (an editor, Obsidian, the desktop free-edit save) cannot be
intercepted, because markdown is the source of truth. They leave no log line. The CLI therefore
offers an `--edit` flow that routes an editor session through the transaction (§6.3). Whether the
desktop app should do the same is open (§9, Q5).

### 5.4 How it is born — promotion, cheaply

The **default habit** stays the draft's: work it out in the log first, and write the reference
note the second time you go looking for it. The tool supports this without enforcing it:

- **Promote** creates a reference note and pre-fills `origin:` with the daily notes it came from.
  It writes `promoted to [[reference/<slug>]]` to today's log, in one transaction.
- **Create** without an origin is always allowed. It is the escape hatch for knowledge you
  *know* you will reuse.
- **Triage** can route an inbox capture into a new reference note.
- **Noticing the second use** is an agent's job, not the tool's. A skill (`examples/skills/`) that
  finds the same topic in two daily notes can suggest promotion. The tool never counts.

### 5.5 Staleness — shown at the point of use only

- `verified` is set on creation, bumped by **verify** (you followed the procedure and it still
  works; no log line, because that would be noise) and bumped by **revise**.
- Reference notes **never** appear in `cdno orient`, the weekly or monthly context, or any review
  list. There is no staleness lint.
- Whenever a reference note is **read**, through the MCP read tool, `cdno open`'s header or the
  desktop viewer, its age since `verified` is shown alongside it. Past a threshold (working value:
  365 days, configurable) it becomes a caveat: *"last verified 14 months ago"*. The MCP tool
  description tells the agent to pass that caveat on before the user follows a procedure.

This answers the draft's open question 5 (passive age banner) with *yes, but only where you are
already looking*.

### 5.6 Layout

- A flat `reference/` folder. At most one coarse subfolder level (`reference/tooling/`) once the
  list gets unwieldy (around 40 files); this is a habit, not enforced.
- Wikilinks use the qualified form `[[reference/<slug>]]`. Two notes with the same stem in
  different subfolders make a bare `[[slug]]` ambiguous, so lint warns about it.
- Prefer fewer, larger notes, especially for mathematics. Cite a section with a heading link,
  `[[reference/linear-algebra-notation#Pseudo-inverse]]`.
- No cap.

---

## 6. Detailed design (for the built-in stage)

### 6.1 Core

- `paths.rs`: `pub const REFERENCE: &str = "reference";`, added to `RESERVED_TOP_LEVEL_FOLDERS`.
  `cdno init` creates it and reconciliation walks it like any top-level folder.
- **No append/mutability policy in core.** No index migration is expected, since `note_type` is
  already free text and `tags` and the created date are already indexed. This needs confirming
  during implementation.

### 6.2 Domain

- `NoteType::Reference` with `is_append_only() == false`, plus a default template under
  `crates/cdno-domain/templates/`.
- One file per operation under `src/vault/reference/`, each going through `VaultTransaction`:
  - `create_reference(title, body, tags, origin)`: when `origin` is non-empty it is a promotion
    and also logs `promoted to [[…]]`;
  - `revise_reference(slug, reason, Revision)`, where `Revision` is either a whole body or a
    `(section, content)` upsert. It bumps `verified` and logs `revised [[…]] — <reason>`. A no-op
    revision (identical text) writes nothing, mirroring `update_project_state`;
  - `verify_reference(slug)` bumps `verified` only;
  - a read view carrying `verified` and `days_since_verified`, so every interface shows age the
    same way.
- Deliberately **no append operation**. This is the domain-level answer to the draft's "append
  rejected at the core layer".

### 6.3 CLI (flags-and-prompts, `docs/cli-ergonomics.md`)

```
cdno reference new     [--title T] [--tag K]… [--origin DATE|PATH]…   # alias: cdno ref
cdno reference revise  [--slug S] [--reason R] (--body-file F | --edit)
cdno reference verify  [--slug S]
cdno reference list    [--tag K] [--stale]
```

`--edit` opens `$EDITOR` on a temporary copy and commits the result through `revise_reference`,
so an editor session still leaves its log line. `new` with `--origin` is the promotion path.
Reading uses the existing `cdno open` and `cdno search --type reference`; no new read verb.

### 6.4 MCP

- `create_reference`, `revise_reference`, `verify_reference`. Their descriptions state the filing
  test and the provenance rule (§5.1), because tool descriptions are the only instruction surface
  an agent sees.
- **A generic `read_note(path|slug)`**, useful for every note type, but a precondition here: a
  reference note an agent can find but not read is worthless. For reference notes it returns
  `verified` and `days_since_verified`.
- `search_notes` needs no change beyond the new type name.

### 6.5 Desktop

No dedicated view in v1. Search with a type filter and the note viewer are enough, plus a small
age line in the viewer header. Whether editing a reference note in the desktop app goes through
`revise_reference` (asking for a reason) or keeps the free-edit raw save is open (§9, Q5).

---

## 7. Compatibility and risks

- **No migration.** Nothing moves automatically, and the folder fills organically.
- **Trial-to-built-in hand-over.** Custom type names that shadow a built-in are rejected at vault
  open, so a vault that declared `[note_types.reference]` during the trial (§8, stage 0) would stop
  opening once the built-in ships. Proposed: for one release, a `[note_types.reference]` whose
  folder is `reference` is *ignored with a warning* and a lint hint to delete it. Notes written
  during the trial already carry `type: reference` and the right folder, so they need no rewrite.
  The trial template should use the same field names as §5.2.
- **Risk: a junk drawer.** "Reference" can absorb anything that is not obviously something else.
  The filing test and promotion-on-second-use are the defences. The trial will show whether they
  hold.
- **Risk: log noise.** One line per revision is cheap. If revisions turn out to be frequent (a
  note edited ten times in a day), batching is a later concern.
- **Risk: the name.** "Reference" is overloaded in this project (§9, Q1).

---

## 8. Implementation plan — staged

**Stage 0: trial, zero code.** Ship `examples/note-types/reference/` (a config snippet plus a
template using §5.2's fields) and a docs-site recipe. Use it for four to six weeks. This answers:
does promotion happen? How many notes, and what shape? Is `tags` enough? What do the missing
behaviours (revise logging, age signal) cost in practice?

**Stage 1: generic gap.** Add an MCP `read_note` tool. This is independent of the rest of the
RFC, is worth doing anyway, and makes the trial usable from an agent.

**Stage 2: built-in type.** Only if stage 0 shows the habit takes: §6.1 to §6.4, the trial
hand-over (§7), the `CHANGELOG.md`, `STATUS.md` and design.md table updates, and a docs-site
concepts page.

**Stage 3: desktop and skills.** The viewer age line, the Q5 decision, and a promotion-suggesting
skill in `examples/skills/`.

---

## 9. Questions for reviewers

1. **Name.** `reference/` is clear but already heavily used here: the docs-site's "CLI reference",
   and design.md's "stable reference" lifecycle category for portfolios, stewardships and
   questions. Candidates: `handbook/` (the draft's *do or recall* split fits a handbook well),
   `shelf/`, `know-how/`, or a Spanish term (`apuntes/`, `recetario/`). The author leans towards
   **`handbook/`**. This RFC uses `reference` as a working name only.
2. **Revision trace.** Is a one-line `revised [[…]] — reason` enough, or should the log keep the
   old text of the *changed section* (a `was:` block limited to one section)? More history versus
   more log noise.
3. **Provenance rule strength.** Guidance in tool descriptions only, or a lint warning when an
   evidence note links to a reference note with no inline invocation? Detecting the latter
   reliably looks hard.
4. **Staleness threshold.** Is 365 days right, per vault or per note (a `review_after` field)?
   Or should age be shown always, with no threshold?
5. **Desktop editing.** Should the desktop app route reference edits through `revise_reference`
   and ask for a reason (keeping the invariant, adding friction), or keep the raw save (lower
   friction, silent history gap)?
6. **Attachments.** Can a reference note own files (diagrams, config templates)? The portfolio
   stub model (`<stem>.md` beside `<stem>/`) exists but is deliberately scoped to `portfolios/`.
   The author leans towards **no in v1**, since code blocks cover most cases; revisit after the
   trial.
7. **Routines.** Should stewardship routines stay separate (the author's lean: yes, they are
   bound to tracking), or become reference notes linked from the stewardship?
8. **Built-in versus generic capability.** Instead of a thirteenth built-in, custom types could
   gain generic opt-in behaviours (`revision_log = true`, `verified_field = "verified"`). That is
   more flexible, but it widens the custom-type contract and moves an RLM concept out of the
   method into config. The author leans towards **built-in**, because the filing test is part of
   the method, but this deserves a reviewer's view.

---

## 10. Verification (for stage 2)

- `cdno-domain` unit tests (memory store): revise logs exactly one `revised [[` line and bumps
  `verified`; a no-op revise writes nothing; promote fills `origin` and logs; verify writes no log
  line; reference notes are absent from the orientation, weekly and monthly contexts.
- `cdno-core`: `init` creates `reference/`, reconciliation indexes it, and a custom type may not
  claim the folder.
- `cdno-cli`: `assert_cmd` wiring for each verb, including `--no-interactive` missing-flag errors.
- `cdno-mcp`: handler tests for the three tools and `read_note`, and an `e2e_*` round-trip.
- Lint: duplicate-stem warning across `reference/` subfolders; no staleness finding.
- The trial hand-over: a vault declaring `[note_types.reference]` opens with a warning, not an
  error.
