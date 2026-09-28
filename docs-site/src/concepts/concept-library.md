# The concept library

Some of what you work out is neither evidence nor work. *What the Woodbury identity buys you when
you invert a low-rank update.* *How to do a rolling restart of a StatefulSet without losing
quorum.* It answers no open question, so it is not evidence. It has no deliverable and never
completes, so it is not a project or an action. It belongs to no perpetual responsibility, so it is
not a stewardship routine. Left in the daily log, it is findable but scattered: three partial
versions across three days, and the one you find first is not necessarily the best.

The cost of that is **re-derivation** — working the same thing out again because the last time is
buried, or because the half-versions disagree. A **concept note** is the fix: the single, current,
best account of one piece of understanding you expect to reuse. Concept notes together form a
**library of concepts** in a flat `concepts/` folder, organised by tags and links rather than by
folders.

A concept note is refined in place as your understanding improves, and each refinement leaves a
one-line trace in the daily log. It has no lifecycle, no staleness, no review obligation and no cap.

## A custom type, declared for you

`concept` is not a thirteenth built-in [note type](note-types.md). It is an ordinary
[custom note type](../reference/custom-note-types.md) that `cdno init` declares in a new vault's
`.cuaderno/config.toml`, with its template installed as `.cuaderno/templates/concept.md`:

```toml
# A declared custom type: the concept library (RFC 0002). Delete this block (and
# any notes under concepts/) if you do not want one; nothing else depends on it.
[note_types.concept]
folder = "concepts"
required = ["created"]
optional = ["tags", "origin"]
template = "concept.md"
```

If you do not want a library, delete the block. A vault created before the type existed can copy
the declaration and the template from
[`examples/note-types/concept/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/note-types/concept).

A concept note looks like this:

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
```

The title is the H1. `tags` is your subject vocabulary: open, indexed, and needing no
configuration. `origin`, written only when you supply it at creation, holds wikilinks to the
daily-note entries the concept came from. The body is free-form: *Statement / Why it matters / See
also* is the template's default, and a procedure or a definition simply uses its own headings.

**One concept per note.** If a note needs two headings that could each be cited on their own, it is
usually two notes, linked. A single derivation that does not split can stay one note, with its
sections cited by heading link (`[[concepts/woodbury-identity#Statement]]`).

## The filing test

Before you keep something, ask *what is it?*

1. **A dated observation, result or source that bears on an open question** → **evidence** in a
   [portfolio](../tutorials/research-and-evidence.md).
2. **The answer to a question you weighed evidence for** → the **question note**, with
   `status: answered`. Its `## Current Thinking` may be promoted into a concept that the question
   links to; the question keeps the record of *how* it was answered.
3. **A prescribed practice belonging to one stewardship** (a workout plan, a care routine) → a
   stewardship **routine**.
4. **Understanding you will reuse, independent of any deliverable** — a theorem, a definition, a
   technique, a procedure, an idea → a **concept note**.
5. **Not yet settled** → an entry in the day's **`## Notes`**, until you promote it.

And the things that are none of these stay where they already go: something to *do* is an
[action](../tutorials/actions.md) on a project map; finite work with a deliverable is a
[project](../tutorials/projects.md).

A benchmark result is evidence. What the benchmark taught you about the technique is a concept. The
command that ran it belongs in the evidence note — and may also become a concept, if you will run it
again.

### The provenance rule

**Evidence never depends on the current text of a concept note.** An evidence note records what was
done and observed inline — the exact invocation, the version, the parameters — and may link to a
concept only as *see also*. A concept is never the `origin:` of an evidence note. Concepts describe
your understanding **now**; evidence records what happened **then**. That separation is what lets
you refine a concept freely without rewriting the past.

## Substance in the day, pointers in the log

`## Logs` is the day's **sequence**: one line per event. Substance — a derivation, a worked
procedure, a page of reasoning — does not belong there, because a block of text turns a sequence
into a wall. It goes in the daily note's **`## Notes`** section instead, one `### <heading>` entry
per piece, while the log gets a one-line pointer to it:

```markdown
## Notes
### Woodbury identity
(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1.

Cheap when A^-1 is already known and the update is low-rank: today's use was the
k=3 refit on [[surrogate-model]]. #concept

## Logs
- **14:32**: noted [[journal/2026/daily/2026-09-02#Woodbury identity]] ([[surrogate-model]])
```

[`cdno log note`](../reference/cli/log.md#cdno-log-note) (or the `note_to_daily` MCP tool) writes
both halves in one go: the entry under `## Notes`, which it creates just above `## Logs` if the day
does not have one yet, and the `noted [[…#Heading]]` pointer line in `## Logs`, followed by the
entry's own wikilinks in parentheses. So the day's sequence shows *that* a derivation happened and
*what it touched*, while the derivation itself stays out of the way. Like `## Logs`, `## Notes` is
append-only.

Each entry's heading is its address, so the tool holds it to a few rules: unique within the day,
never a daily section name (`Standup`, `Intention`, `Agenda`, `Meeting`, `Notes`, `Logs`), no `[`,
`]`, `|`, `#` or inline markup, and not starting with `^`. Headings inside the entry must be level 3
or deeper. If you write an entry by hand, follow the same convention.

**Mark candidates.** End an entry you might reuse beyond today with the tag `#concept` on its last
line. That is how you — or an assistant — find promotion candidates later.

`## Notes` is not read by the project, weekly or monthly context: those read `## Logs`. An entry
about a project reaches that project through the pointer line's wikilinks and the entry's own links
(which become backlinks), so a review sees *that* you worked something out, not the working itself.

## Promotion

Write a concept note straight away when you already know you will reuse the thing. When you are not
sure, note it in the day and promote it **the second time it comes up**.

Promotion is simply **creating a concept note with `origin`** pointing at the entries it came from.
There is no separate promote operation, no counter, and no `promoted to` line:

```bash
cdno note create concept --title "Woodbury identity" --body-file woodbury.md \
  --origin "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]"
```

Creation is logged for you, to today's daily note, as `concept created [[concepts/<slug>]] — <title>`,
and `origin` records where the concept came from. Over MCP the same step is `create_custom_note`
with `body` and `origin`.

The `## Notes` entries stay as they were — the day's record is append-only — and their daily notes
now have a backlink from the concept.

**Search before you create.** A second note on a subject the library already covers is the
duplicate this whole method exists to avoid. `cdno search <word> --type concept` first; if the
concept exists, refine it rather than minting another. (Nothing refuses a second note with the same
title: it is created as `<slug>-2`.)

An assistant connected over MCP follows the same habit. Its tool descriptions tell it to search the
library before drafting an explanation, and to offer promotion when it finds `#concept` entries on
the same subject on two or more dates.

### Finding candidates, and the current limitation

Search matches **words, not tags**. Searching `concept` across daily notes finds the entries tagged
`#concept`, but also — by stemming — any day that merely created, revised or linked a concept note,
since those log lines contain `concepts/`. The snippet rarely shows the entry's subject either, so
open each hit and look for `## Notes` entries whose last line carries `#concept`:

```bash
cdno search concept --type daily
```

A search that filters by tag is planned but not shipped.

## Refinement

A concept note is **mutable in place**. A better explanation replaces the worse one; a correction
replaces the error. The note is always the current best account — correct it rather than annotate
it.

Refine through the tool, so the change leaves a trace:

```bash
cdno note revise woodbury-identity --section "Why it matters" --content-file why.md \
  --reason "added the cost comparison"
```

Each revision that changes the text writes **one line** to today's daily log:

```text
- **14:32**: revised [[concepts/woodbury-identity#Why it matters]] — added the cost comparison
```

- **The reason is required.** It is the part of the history worth keeping, so say briefly *why*.
- The `#Section` anchor is present when you revised one section, and absent for a whole-body
  rewrite.
- There is **no `was:` / `now:` block**, unlike a project's state change. The log records *when* and
  *why*; git records *what*.
- Text identical to what is already there writes and logs nothing.

You can replace the whole body (`--body-file`, or your editor when run interactively with no file)
or upsert one section (`--section` with `--content-file`). A section that exists is replaced
together with its sub-sections; one that does not is appended as `## <section>`. There is no append
operation for a concept — refinement means rewriting what is there.

**The hash guard.** `cdno note revise` reads the note first and remembers a hash of what it read.
If the file changes on disk before the revision is written — say you saved it in your editor while
the revise editor was open — the revision is refused rather than overwriting that change: read the
note again and redo it. Over MCP, `read_note` returns that `content_hash` and a whole-body
`revise_note` must pass it back as `expected_hash`.

Edits you make directly in an editor are legitimate — the Markdown is the source of truth — but they
leave no log line. That is a known limit of the history, not something `cdno lint` polices.

Only mutable custom types can be revised this way. Built-in types (a project's sections belong to
its own commands) and custom types declared `append_only = true` are refused.

## No staleness

Understanding does not go stale. A theorem is as true after two untouched years as on the day you
wrote it down, and an explanation you have not changed is one you were happy with. So concept notes
have **no verified date, no age signal, no staleness lint and no review chore**. They never appear
in `cdno orient`, the weekly or monthly context, or any review list.

The history lives in the daily log instead: every creation and revision is a dated line there, so
"how has my understanding of this evolved?" is a search, not a field.

Usefulness is a question you can ask, not a metric that is kept: the note's backlinks (which notes
link to it) are one `read_note` away over MCP, and whether it has been mentioned in the log is a
search. An unreferenced concept costs nothing and is left alone. A procedure that depends on a
version can say "last run on 2026-09 against v1.4" in its own body if that matters; there is no
field for it.

## Structure by links, not folders

`concepts/` is flat. A concept usually belongs to several subjects at once — a theorem is both
linear algebra and optimisation — so a folder is the wrong axis. **Tags** carry membership, and
**wikilinks** carry relationships.

**Hub notes** carry navigation. When a cluster is real, write a concept note that is mostly links —
`concepts/linear-algebra.md` listing the theorems, the notation sheet, the techniques. It is an
ordinary concept note, written by hand when you need it, never a folder decided in advance. By
convention its slug is the tag it maps. It plays the part a portfolio's `_index.md` plays for
evidence.

If the folder grows past a few hundred files and you browse it on disk, one coarse subfolder level
(`concepts/maths/`) is tolerated. The tool is blind to it: it never asks for a subfolder, and
`[[concepts/<slug>]]` still resolves after you move a file by hand.

## Summary

| You want to… | CLI | MCP |
|--------------|-----|-----|
| Write substance into the day | [`cdno log note`](../reference/cli/log.md#cdno-log-note) | [`note_to_daily`](../reference/mcp/writes.md#daily-weekly-and-monthly-sections) |
| Find an existing concept | `cdno search <word> --type concept` | `search_notes` with `note_type: concept` |
| Read a note whole | [`cdno open`](../reference/cli/open.md) `concept:<slug>` | [`read_note`](../reference/mcp/reads.md) |
| Create or promote a concept | [`cdno note create concept`](../reference/cli/note.md) `--origin …` | [`create_custom_note`](../reference/mcp/creation-and-lifecycle.md) with `origin` |
| Refine a concept | [`cdno note revise`](../reference/cli/note.md#cdno-note-revise-note) | [`revise_note`](../reference/mcp/writes.md#revising-a-note) |
| List the library | `cdno note list concept` | — |

Walk one promotion end to end in [Building a concept library](../tutorials/concept-library.md).

Next: [Vault structure](vault-structure.md).
