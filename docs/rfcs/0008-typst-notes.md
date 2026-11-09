# RFC 0008 — Typst notes: a `.typ` source as a first-class evidence or concept note

| | |
|---|---|
| **Status** | Accepted — 2026-11-08, after a three-seat review (three rounds, on PR #925; no seat held a blocking point). The maintainer's decisions on D13, D16 and O1–O4 are recorded in §9. Implementation plan: [0008-implementation-plan.md](0008-implementation-plan.md). |
| **Tracked by** | PR #925 |
| **Affects** | `cdno-core` (a new `typst.rs`, `paths.rs`, `extractors.rs`, `frontmatter.rs`, `frontmatter_edit.rs`, `artefacts.rs`, `reconcile.rs`, `transaction.rs`, `config.rs`), `cdno-domain` (`type_registry.rs`, `vault/portfolios.rs`, `vault/custom_notes.rs`, `vault/revise.rs`, `vault/set_frontmatter.rs`, `vault/lint.rs`, `vault/rename.rs`, `vault/note_ref.rs`, `vault/notes.rs`, `vault/search.rs`, `vault/log.rs`), `cdno-cli` (`--format` on `cdno file` and `cdno note create`, the watcher), `cdno-mcp` (one field on two tools, `read_note`, rejection codes, server instructions), `docs/design.md`, `CLAUDE.md`, `docs-site`, the `file-to-portfolio` and `concept` skills |
| **Related** | #154 and #183 (attachments), RFC 0002 (concept notes), RFC 0007 (renames rewrite references; slugs at creation), #225 (globally unique stems), #215 (last-segment resolution), #211 (`cdno lint --fix`) |

> **Amended after review.** The first draft was reviewed on the PR by three seats (invariants and
> integrity, adversarial; method and user; implementation and layering), three rounds, seats
> replying to each other until they agreed. The review removed the masked frozen-prefix work the
> draft built on, because evidence has no frozen-prefix snapshot (§3.3); made the link scanner
> read the metadata dictionary's strings (§5.5); replaced the one path predicate with three
> helpers and one classifier (§5.1); added the adoption of an existing `.typ` file with its assets
> (§5.7, S4b); fixed the escape rule structurally (§5.5); made creation refuse a body that does not
> parse (§5.7); corrected the config default for a vault without a concept type (§5.9); moved the
> parsing and editing surfaces into per-format modules in core (§6.1); deferred the `imports`
> column; and restaged the work (§8). The text below is the amended version; the record is on the
> PR.

---

## 1. Summary

Cuaderno knows one note syntax, Markdown. Everything written in Typst — a slide deck, a chapter
with its own bibliography, a derivation with real mathematics, a report typeset for reading —
reaches the vault as an *attachment*: the bytes are imported into a portfolio, a Markdown stub
stands in for them, and the stub's abstract is all that search, lint, backlinks and an agent over
MCP ever see. The source is in the vault but not of it.

This RFC lets a `.typ` file **be** the note, for two types only: **evidence** and **concept**.

- The `.typ` **source is the note**. Its text is indexed, its `[[wikilinks]]` are edges, lint reads
  it, `read_note` returns it. Nothing is compiled; a compiled PDF is a derived file, like the index.
- Its metadata is a **literal dictionary** labelled `<cdno>`, `#metadata((…)) <cdno>`, read with
  the Typst parser alone and standing where YAML frontmatter stands in a Markdown note.
- A `.typ` file is a note **only if it carries that label**. Every other `.typ` in the vault — every
  one that exists today — stays an attachment, with the meaning it has now.
- A Typst body can be **authored** through the create verbs, or an existing `.typ` file **adopted**
  as the note together with its figures and bibliography.
- The reader is general and lives in `cdno-core`; which types may be written in Typst is a domain
  rule with a config switch, so widening it later is not an engine change.

Rendering to PDF is out of scope for this RFC. It is a later phase (§9 D12) that depends on nothing
here except the file being a note.

### 1.1 How you will use it

Filing a Typst note into a portfolio, through the `file_to_portfolio` tool or the CLI:

```sh
cdno file --portfolio surrogate-error-budget \
  --source "Error budget derivation for the surrogate model, 2026-11-08" \
  --origin projects/surrogate-model \
  --format typst --body-file derivation.typ
# -> portfolios/surrogate-error-budget/2026-11-08-error-budget-derivation-for-the.typ
```

`derivation.typ` is whatever the writer's template needs — an `#import`, a `#show` rule, headings,
equations, figures under a sibling folder. The engine writes the metadata block at the top:

```typst
#metadata((
  type: "evidence",
  created: "2026-11-08",
  source: "Error budget derivation for the surrogate model, 2026-11-08",
  portfolio: "surrogate-error-budget",
  origin: "[[projects/surrogate-model]]",
)) <cdno>

#import "@preview/some-template:0.3.0": report
#show: report.with(title: "Error budget derivation")

= Setting

The surrogate's error splits as in [[concepts/bias-variance-split]] …

$ E[(hat(f) - f)^2] = "bias"^2 + "variance" + sigma^2 $
```

From then on the note behaves as evidence does: it is append-only by the same verb rule, it
appears in `get_portfolio_contents` and search, `[[concepts/bias-variance-split]]` is a backlink
on the concept, a rename of the concept or of the project named in `origin` rewrites that address
inside the note, and `cdno lint` reports a dangling link exactly as for a Markdown evidence note.
The title in the `show` rule is appearance, the template's business; the index knows the note by
its `source`, as it knows a Markdown evidence note today.

A deck already written, with its figures beside it, is adopted rather than retyped:

```sh
cdno file --portfolio surrogate-error-budget --source "Review deck, 2026-11-08" \
  --origin projects/surrogate-model --format typst --attach ~/talks/review.typ
# -> portfolios/surrogate-error-budget/2026-11-08-review-deck.typ
#    portfolios/surrogate-error-budget/2026-11-08-review-deck/figs/…   (copied, paths rewritten)
```

A concept is created the same way, with `--format typst` on `cdno note create` or
`format: "typst"` on `create_custom_note`, and revised whole with `revise_note`.

What does not change: `cdno file --attach deck.typ` without `--format` still files an attachment
with a stub, and every stub and attachment already in the vault is untouched for ever.

## 2. Motivation

### 2.1 The need, counted

The question put to the vault was not "would Typst be nice" but "how often does the gap cost
something". Measurements taken on 2026-11-08 over one working vault:

| Measurement | Count |
|---|---|
| Typst-built documents filed as attachments, by month (Jul, Sep, Oct 2026) | 1, 5, 13 |
| of which evidence (a dated record bearing on a question) rather than a project's deliverable | not counted (§9 O4) |
| Attachment stubs whose `kind` records the derived PDF rather than the Typst source | 13 of 14 |
| `.typ` files in the vault, all under `portfolios/` | 58 |
| Evidence notes whose content is a reusable concept, and that need Typst's mathematics | 2 of the 8 strongest candidates |

The first row is the one that matters: filing a Typst document is now a weekly act, and growing.
The second row is the scope check: this RFC serves evidence, and a deck that *is* a project's
deliverable is not evidence and is not served here (§9 D1). The third row shows what the
attachment route does: the thing recorded is the PDF, the source is incidental, and the stub's
abstract has grown into a slide-by-slide summary because it is the only searchable surface. The
fourth row says the vault already holds the sources; nothing about them is indexed, and most were
written with figures and a bibliography beside them, which is why adoption (§5.7) is in scope. The
last row is the concept need: small, but it is exactly the concepts Markdown serves worst, the ones
that are mostly equations.

### 2.2 Why not a better attachment

The attachment route was examined first and rejected on the PR's precursor discussion. A stub plus
a source plus a build is three files describing one thing, and the abstract is a hand-written copy
of the content that drifts from it. Indexing the attached `.typ` text while keeping the stub would
index the content and attribute it to a file that is not the note, so a backlink would land on a
stub. The source has to be the note for the graph to be right.

### 2.3 What cuaderno actually depends on

Cuaderno does not depend on Markdown. It depends on four things a `.typ` file provides as well:

1. **Plain text on disk is the source of truth**, and the index is a cache rebuilt from it.
2. **Metadata readable without executing anything.** YAML is parsed, never evaluated. A Typst
   literal dictionary read by the parser is the same: no packages, no fonts, no evaluation, and a
   note that does not compile is still in the index.
3. **Links as plain text**, `[[slug]]`, which Typst markup leaves alone (with one escape rule, §5.5).
4. **Targeted edits** — replace a whole body, rewrite a reference at its span — which are byte
   operations on a text file.

What *is* Markdown-specific is the section machinery the machine-written types rely on: a daily
note's `## Logs`, a project map's `## Current State`, a tracking table. Those types are not in
scope (§9 D1), so none of that machinery needs a second syntax.

## 3. Background

### 3.1 How a file becomes a note today

Reconciliation lists the vault and indexes every file with an `.md` extension that is not under an
`ignore` glob and not inside an attachment's folder (`reconcile.rs`). The review counted twenty
sites that spell `.md` out, of three kinds:

- **(a) classification** — is this path a note: `reconcile.rs`, two closures in `transaction.rs`,
  the CLI watcher, the rename's referrer scan (`rename.rs`), `note_ref.rs`;
- **(b) address to path** — turning `[[concepts/x]]` into a file: `resolve_one` in
  `extractors.rs` (rules 0a, 1 and 1b append `.md`), `note_ref.rs`, `rename.rs`, `artefacts.rs`;
- **(c) path to address** — stripping `.md` to write a log line or a link: `revise.rs`, `log.rs`,
  `set_frontmatter.rs`, `portfolios.rs`, `notes_section.rs`, `monthly.rs`, `tracking.rs`.

A `.typ` note breaks (b) and (c) as much as (a): an exact-path link to it would never match, and
the log would record `revised [[concepts/x.typ]]`. A few sites are Markdown on purpose
(`migrate.rs`, `questions.rs`, `stewardships.rs`, a lint walk) and stay so. Section 5.1 replaces
the rest.

### 3.2 Attachments and stubs (#154)

`cdno file --attach` imports a file to `portfolios/<slug>/<stem>/<file>` and writes an evidence
stub `portfolios/<slug>/<stem>.md` carrying `kind: <pdf|image|typst|…>` and the abstract as its
body. `artefacts.rs` makes the folder belong to the stub by *location*: anything inside it is not
a note, however it is named, and ownership is established positively by reading the stub's
frontmatter, never inferred from the path shape alone. A `.typ` source attached today is such a
file: present, owned, unindexed. The import is one transaction op, `FileOp::Import`, that copies an
external file and rolls back by deleting what it created.

### 3.3 Append-only is a verb rule for evidence

Evidence is append-only. For evidence, as RFC 0007 §3.4 records, that is a **convention enforced
by the write verbs**: `revise_note` refuses an append-only type, and nothing else edits a body. No
byte check exists. The frozen-prefix snapshot and the masked hash of RFC 0007 §5.3–5.4 apply to
**archived action notes only**: the snapshot is recorded at archive and lint verifies it only for
`type == "action"` under `actions/_done/`. The first draft of this RFC assumed the check covered
evidence and specified a Typst locator for the masked hash; the review found the check does not
run on any type in scope, and the locator is gone. If evidence is ever to be snapshotted, that is
an RFC of its own, for both formats, with the frozen point and the backfill stated.

### 3.4 How links are parsed

`wikilink_spans` scans a body for `[[…]]`, skipping fenced code, inline code and inline HTML so a
link quoted in a code sample is not an edge. For Markdown the scan runs over the whole file, YAML
included, so `origin: "[[projects/x]]"` is found, counted as an edge and rewritten by a rename
without the scanner knowing any YAML. Typst has its own quoting constructs — raw blocks, line and
block comments, code mode, strings, maths — and its own hazards: `#` begins an embedded expression
in markup, `_` and `*` open emphasis that must close, and the markup lexer splits a run of text at
many characters (`[`, `]`, `/`, `-`, `.`, `:`, `<`, `>`, `'` among them), turning `--` into a
shorthand leaf and `'` into a smart quote (§5.5).

### 3.5 Titles, headings and sections

A Markdown note's index title comes from a `title` frontmatter field when one exists, and the
full-text title from its first `# ` heading (`first_h1`); evidence has neither and is known by its
`source`. Sections are `## ` headings, and `revise_note` replaces one section by heading. Typst
headings are `= `, `== `, …, but a Typst document's title is usually an argument to its template's
`show` rule, not a heading in the body.

### 3.6 Concepts (RFC 0002)

A concept is a custom type the vault declares in config (`[note_types.concept]`), mutable,
refined in place with `revise_note` by section or whole, and created with `create_custom_note` or
`cdno note create`. Its template appends a scaffold after the body. It is not built in: a vault
may not declare it.

## 4. Terminology

- **Typst note** — a `.typ` file that carries a `<cdno>` metadata block and lies in a folder of a
  type that admits Typst. It is a note in every sense the index has.
- **Metadata block** — the `#metadata((…)) <cdno>` expression: a function call to `metadata` with
  one literal dictionary argument, immediately labelled `cdno`.
- **Preamble** — the run of `#import`, `#set` and `#show` expressions (with comments and
  whitespace) between the metadata block and the first other node. `#let` is never preamble.
- **Attachment** — any other non-Markdown file, `.typ` included: imported bytes owned by a stub or
  by a Typst note.
- **Adoption** — filing an existing `.typ` file as the note itself, with its assets, rather than
  as an attachment.
- **Format** — `markdown` or `typst`, a property of a note derived from its extension, never
  stored.

## 5. Proposal

### 5.1 Three helpers and one classifier, before anything Typst

The twenty sites of §3.1 are replaced first, as a stage of its own with no Typst behaviour (§8 S1):

- `note_format(path) -> Option<NoteFormat>` — the format a path *would* have as a note, from its
  extension alone; a pure function in `cdno-core/src/paths.rs`. It does not say whether the file
  is a note: for Typst that depends on content (§5.2).
- `note_address(path) -> String` — the path without its note extension, for log lines and links
  (the (c) sites).
- `note_candidates(address) -> Vec<VaultPath>` — the paths an address may name, one per format
  the folder admits, for `resolve_one` and the other (b) sites. A link to a Typst note then matches
  under rule 1 as a Markdown note does.
- **One classifier** — the walk that reconcile performs (list, skip `ignore` globs, skip
  attachment folders, read a `.typ` for its label) becomes one core function, used by reconcile
  and by the rename's referrer snapshot, which duplicates the walk today. It is the only code
  that ever reads a `.typ` to decide what it is, and it returns a **report** of what it skipped
  and why, which lint surfaces (§5.10): lint is index-driven and would otherwise never see a file
  the classifier left out.
- The transaction derives the notes it added and removed from its own upsert and remove ops, not
  from extensions. This also fixes a small existing bug: an imported `.md` attachment counts as an
  added note today.

"Is a note" means **membership in the index after reconcile**, nothing else. S1 is behaviour-neutral
except for one stated unification: the watcher's extension test is case-insensitive and
reconcile's is not; both follow `note_format`. The sites that are Markdown on purpose each keep
their literal with a comment stating why.

### 5.2 What makes a `.typ` file a note

A `.typ` file is a Typst note when **all three** hold, checked **in this order**:

1. It lies in the folder of a type that admits Typst (§5.9): under `portfolios/<slug>/` for
   evidence, under the declared folder for concept.
2. It is not inside an attachment's folder or a Typst note's assets folder (§3.2, §5.7). This is
   settled from the owner, before the file itself is opened: a file inside an owned folder is an
   attachment and is **never read**, label or no label.
3. Its source contains a metadata block (§4) at the top level of the document.

**The promise is "never parsed".** A `.typ` is parsed only when it is a candidate note: in an
admitted folder, not owned, and carrying the label. Short of that, the one thing that can happen to
it is a **byte search for `<cdno>`**, which runs on every non-owned `.typ` under `portfolios/` and
under every custom type's folder, **whether or not the type admits Typst**, so that a labelled file
in a folder the switch excludes can be reported (§5.9). A `.typ` anywhere else is never touched.
That matters because a Markdown note pays its parse once and then rides the index row's fast path,
while a `.typ` that is not a note has no row and is re-examined on every `Vault::new`, which is
every CLI invocation. The residual cost is one read per non-owned unlabelled `.typ` per command.
S0 measured it (warm page cache, release build): `typst_syntax::parse` takes 24.9 µs on 2 KB,
226 µs on 20 KB and 2.2 ms on 200 KB; the byte search 0.04 µs, 0.58 µs and 12.9 µs; a read about
11 µs (33 µs at 200 KB). Reading and searching 58 files of 20 KB raised `open_vault` from about
5 ms to about 6 ms, the upper bound for 58 loose files; owned attachments cost nothing.

A `.typ` that satisfies 1 and 2 but not 3, a loose `.typ` under a permitted folder with neither
label nor owner, is reported by the classifier as a **Warning** ("Typst file is neither a note nor
an attachment") and not indexed. A `.typ` that satisfies 1 and 2, has no `<cdno>`, and whose bytes
hold `#metadata(`, optional whitespace including newlines, `(`, and later a `type` or `"type"` key
is reported as an **Error** on the file (§9 D6): it was meant to be a note, with a missing or
mistyped (`<cdn>`) label, and the one visible trace of a note that fell out of the index must be
loud. `#metadata ((` is not a call in Typst and does not match. The check is a byte search, so it
also fires on such a call inside a comment, which errs on the loud side. A label that is present
but is not the call's sibling, or a block not at top level, is found by the parse and reported on
the note (§5.3).

A labelled `.typ` under an owned folder stays an invisible attachment, with no row, Warning or
Error. This is accepted: the folder belongs to its owner, and opening every owned file to look for
a label would cost the read that ownership-first exists to avoid.

The detection is **by content, not by convention**, for the same reason `artefacts.rs` reads the
stub rather than trusting the path: a wrong answer in the "is a note" direction indexes a file as
a note with no metadata and reports a parse error, which is visible; a wrong answer in the other
direction drops a note silently.

### 5.3 The metadata block

```typst
#metadata((
  type: "concept",
  title: "Bias–variance split",
  created: "2026-11-08",
  origin: "[[journal/2026/daily/2026-11-02#Error budget sketch]]",
  tags: ("surrogate", "error-budget"),
)) <cdno>
```

Rules, each enforced at creation and checked by lint:

- **Exactly one** block per note, at the **top level** of the markup (not inside a content block,
  a function body or a `show` rule), and **first** in the file: only whitespace and comments may
  precede it. The engine writes it there; lint reports an Error when it is elsewhere.
- The argument is a **literal dictionary** whose values are **strings**, **arrays of strings**,
  **booleans**, or **numbers**. A number is an integer or float literal with an optional leading
  `-`, read as Typst's parser takes it: an integer too wide for 64 bits is a float, and a float
  that is not finite is refused. No identifiers, no function calls, no arithmetic, no units, no
  nested dictionaries, no string concatenation. A key is an identifier or a string; the engine
  writes a string key for anything that is not an identifier, and for a Typst keyword (`let`,
  `in`, `none`, `true`, …), which the parser does not take as a bare key. Reading it is then a tree walk with no evaluation, and the result maps
  onto the same `serde_json::Value` shape YAML frontmatter produces today, so everything
  downstream of the parse — the type registry, `extra_required`, schema validation,
  `set_frontmatter` — sees no difference. A block that breaks the rule is a **parse error**: the
  file is indexed as a note with a frontmatter error, the way a Markdown note with malformed YAML
  is, and lint reports it as an Error. It is not demoted to an attachment.
- `type` is **required**. `title` is required for a type whose Markdown form carries a title
  heading (concept) and is the note's index title and full-text title; evidence carries no `title`
  and is known by `source`, as in Markdown. The engine never guesses a title from a `= ` heading
  or a template argument.
- Dates are strings in the vault's `YYYY-MM-DD` form, as in YAML. Typst's `datetime` is a
  function call and is refused by the literal rule.
- **Strings take Typst string escapes only** (`"` and `\`), written by the engine; the markup
  escape rule of §5.5 never applies inside the block, so an anchor in a string is a bare `#`
  (the parser keeps `\#` in a string as a literal backslash and hash). A one-element array is written `("x",)`,
  since `("x")` is a parenthesised string; the reader accepts both.
- **The block is the frontmatter.** `set_frontmatter` rewrites a value inside it byte-exactly, the
  way `frontmatter_edit.rs` rewrites a YAML scalar, and errors on a key the block does not hold; it never
  re-serialises the dictionary, so a comment or the writer's key order survives. The per-format
  edit surface is listed in §6.1.
- The block is **excluded from the full-text index** (§5.4), so a tag in it is as invisible to
  `search_notes` as a tag in YAML frontmatter is today, and the two formats search alike.

A Typst document may hold other `#metadata` calls with other labels; only `<cdno>` is read.

### 5.4 What is indexed

- The note row: path, type, title (from the block when present), created, format-neutral
  frontmatter JSON, content hash — as today, from the block.
- **Full text**: the source after the metadata block, as it is. Typst syntax leaks into search
  snippets (`#`, `=`, `$`), which is accepted in v1; a text-only projection for search is a later
  refinement and needs no schema change.
- **Headings**: every `= `, `== `, … heading, with its level, in the `headings` the index already
  keeps for Markdown notes, so `read_note` reports them and an anchored link to one can resolve.
- **Wikilinks** from the markup and from the block's strings (§5.5). **Inline tags** are not read
  from Typst text; tags come from the metadata `tags` array only.
- **Not indexed**: the `#import` lines. A column for them was proposed and deferred to the
  rendering RFC (§9 D15); the parse is cheap enough to run on demand.

### 5.5 Links in Typst markup

`[[slug]]` and `[[slug|label]]` are plain text in Typst markup, so the syntax is unchanged. The
scanner is a new one over the parsed tree rather than a character scan with Markdown exclusions,
and it is defined structurally so that no character list has to be maintained:

- **Where links are read.** In markup, at any nesting (a content block inside a `#figure(…)` call
  is markup), over **contiguous runs of inline leaves** read from the source bytes — Text, Escape,
  Shorthand, SmartQuote and Space (the `[` and `]` of a link are Text leaves of their own) — because the lexer splits a plain run like
  `[[2026-11-08-error-budget]]` into several leaves whenever it holds a `--` or a `'`, with shorthands and smart quotes among them. Not in raw blocks
  or inline raw, line or block comments, code-mode expressions, labels, references (`@x`) or
  maths. **And in the string values of the `<cdno>` dictionary**, with the span inside the quotes,
  so that `origin: "[[projects/surrogate-model]]"` is an edge and a rename of the project rewrites
  it, as the Markdown scan over YAML does today. No other string literal is read.
- **Escapes.** `#` starts code in markup, `_` and `*` open emphasis, and several other characters
  change the lexing of a run, so a link whose address or anchor contains one cannot be written
  bare: `[[slug#Heading]]` would parse `#Heading` as a variable and fail to compile. The rule for
  **writers**, one line in LINKING-RULES: *inside `[[…]]` in a `.typ` body, put a backslash before
  any Typst markup character; over-escaping is harmless.* The characters met in practice are `#`,
  `_`, `*`, `$`, a backtick, `<`, `@`, `~`, `//` (so also the `://` of a URL, which otherwise
  lexes as a Link) and `/*`, and `:` in the head of a term-list item. And *put a space before `[[` when it follows a `#…`
  expression or a reference*: `#emph[a][[x]]` is a trailing content-block argument and `@ref[[x]]` a reference's supplement, so neither holds a link at all. The rule for the **machine**: the bytes
  between `[[` and `]]` must lex as one inline run of the leaf kinds above; any other leaf kind
  inside the brackets is a lint **Error**, as is a `[[` with no `]]` in its run (a stray `[[` in
  prose, unlike Markdown) and a link that spans a line (a Space leaf can hold a newline; the
  Markdown scanner refuses the same). The scanner decodes
  every **Escape** leaf by its lexed value (`\#` → `#`, `\u{2013}` → the character); Shorthand and
  SmartQuote leaves keep their **source bytes**, so `--` in a slug and a date's hyphens match as
  written. Lint *finds* `[[` as two adjacent Text leaves `[` in a markup node, so a run the lexer
  broke cannot hide a link from lint, and the content-block bracket of `#foo[[x]]` is not an opening.
- **Anchors.** `[[concepts/x\#Error budget]]` resolves to the `= ` heading whose **source text** —
  the bytes after the `=` run and one space, a trailing `<label>` stripped, escapes decoded, trimmed,
  whitespace collapsed — equals the anchor text, compared exactly as a Markdown anchor is compared
  with the text after `## ` today. So an anchor written in a Markdown daily, `[[concepts/x#Error
  budget]]`, reaches the Typst heading, `= The *bias* term` is addressed as `The *bias* term`, and
  a heading is never matched by its rendered text (`--` matches `--`, not an en dash).
- **Renames.** `rewrite_wikilink_addresses` rewrites addresses in a Typst note over the spans the
  Typst scanner reports, writing the new address escaped by the same rule, and `differs_only_by`
  verifies the result as for Markdown. The address span holds the raw bytes of the address, any
  escape inside it included (`a\_b`), and ends before the backslash of an escaped anchor; S0
  confirmed the round trip.

How a `[[slug]]` *looks* when compiled is a rendering question for the template (a `show regex`
rule in a template can turn it into a link or a margin note) and is not the engine's concern.

### 5.6 Append-only and renames

Append-only for a Typst evidence note is **the same verb rule** as for Markdown (§3.3):
`revise_note` refuses it, `set_frontmatter` edits only the block, and a rename rewrites only
references. No byte check exists for evidence in either format, so nothing in this RFC masks,
snapshots or verifies a prefix, and `MASK_VERSION` is untouched. A dated revision appended at the
end (a `= Revision 2026-12-01` heading) is the convention for Typst as it is for Markdown; no
engine verb writes it in either format.

A Typst concept or other custom-type note may itself be **renamed** (RFC 0007); evidence stays
unrenamable in both formats (RFC 0007 §5.2). The file moves with its assets folder, its
referrers are rewritten through the Typst scanner, and the alias is appended to the block's
`aliases` array with the per-format edit surface (§6.1). This is a stage of its own (§8 S3b).

### 5.7 Creation: authored or adopted

One verb per type, as today, with a format switch, and two ways to supply the Typst:

- `file_to_portfolio` gains `format: "markdown" | "typst"` (default `markdown`); `cdno file` gains
  `--format typst`. `create_custom_note` and `cdno note create` gain the same field, for the types
  that admit it.
- **Authored.** With `format: typst` and no `attach`, `content` is the **Typst body**: everything
  after the metadata block, preamble included, complete.
- **Adopted** (stage S4b). With `format: typst` **and** `attach: "<file>.typ"`, the file *is* the
  note. The interface layer reads it and enumerates the directory it sits in (its figures, bibliography and any other files; at most 200 files or 200 MiB; entries named `.…` are skipped, so a deck that names one is refused as not in the folder; a symbolic link or a folder overlapping the vault is refused) into
  `(source, destination)` pairs (the domain does no file I/O); the engine refuses a file whose
  parse holds an Error node or that already carries `<cdno>`; it finds every **literal relative
  path** under `image` (and its `icc:`), `read`, `json`, `csv`, `yaml`, `toml`, `xml`, `cbor`,
  `plugin`, `pdf.attach`, `bibliography`, `include` and a path `import`, and the string values
  of `bibliography(style:)` and `cite(style:)` when they have an extension (any other string there is
  a built-in style name), `raw(theme:)` and `raw(syntaxes:)`, array forms included, whether the
  callee is bare, `std.`-qualified, parenthesised or reached through `.with(...)`, and `set`
  rules, resolves each against the source file's directory, **refuses**
  unless every one lands inside the folder being copied (a non-literal path argument is refused
  with a `remedy`; a `/`-rooted path is refused, since it depends on the compile root the
  rendering RFC owns; a `@preview` package import or include is left alone; Typst has no URL loading, so a string
  such as `x://y` is an ordinary local path and is checked like any other), and **rewrites** each at its
  span to `<stem>/…` with the literal writer, listing the rewrites in the result. The assets are
  copied with the existing `FileOp::Import`, one op per file, before the note's write, each
  destination checked for existence as `file_attachment` does today. A compiled PDF among the
  assets is an owned file lint does not warn about: while rendering is deferred it is the view the
  person actually reads, and the folder copy brings it along. `read_note` does not point at it in
  v1.
- Without `format`, `attach` files an attachment with a stub, exactly as today.
- The engine **writes the metadata block** from the fields it owns — `type`, `created`,
  `portfolio`, `origin`, `source`; `title` and the declared fields for a custom type — and
  prepends it to the body. The skill never writes `<cdno>` itself, so one authority stamps the
  fields in both formats, and a body that already carries a block is refused. **Field transport**:
  `fields` values are strings, and an array only where the caller supplies one; a declared `int`,
  `float` or `bool` field of a custom type is coerced to its type and written typed, so the note
  passes `check_value` on re-read (`FieldSpec.list` is reserved, so no declared
  field is a list); `vars`
  with `format: typst` is refused with a code, since template variables belong to Markdown
  templates; no title heading is written.
- **Creation refuses a body that does not parse** or that would trip any lint row of §5.10: one
  pure check, shared with lint, run before the write, vault byte-identical on refusal, every
  refusal carrying a `remedy`. An evidence note is frozen as written, so the mistakes an agent can
  make — an unescaped `#`, an unclosed `_`, a raw block left open — must be refused at the door
  rather than reported for ever. The one mistake that passes is a package name or version that
  does not exist; nothing in v1 compiles, so it costs nothing until the rendering RFC, which owns
  the remedy (§9 D16).
- **Minimum Typst version 0.12.** Before 0.12 a document `set` rule had to precede all content, and
  a `#metadata` ahead of a template whose `show` rule sets `document(title: …)` failed to compile.
  S0 confirmed it by compiling a prepended body and an adopted file through a template that sets
  page and document: 0.11.1 fails with "document set rules must appear before any content"; 0.12.0
  matches the un-prepended source in text, raster, title and page count; 0.14.2 is byte-identical.
- **No engine template for Typst bodies in v1.** `.cuaderno/templates/*.md` and `[variables]` are
  Markdown's; a Typst body arrives complete from the skill, which chooses the Typst template — the
  precursor's decision that templates are plain Typst and the skill's business. The concept
  scaffold (*Statement / Why it matters / See also*) is therefore not appended to a Typst concept.
- The slug is derived as today (`slugify`, `--slug`/`slug` from RFC 0007); the extension is `.typ`.
- **Stem exclusivity.** Creation refuses a `.typ` whose stem matches an existing `.md` in the same
  folder, and vice versa, with the same rejection the rename uses for a taken slug. Two notes
  sharing a stem would make `[[stem]]` ambiguous under the stem rule and, worse, a stub `x.md`
  beside a note `x.typ` would look like the stub owns the note. Reconciliation indexes both if it
  finds both (nothing is dropped silently) and lint reports an Error whose message names the
  likely cause: Obsidian creates `x.md` when a link to a `.typ` note is clicked (§5.11).
- **Sibling assets.** A Typst note at `portfolios/<slug>/<stem>.typ` owns `portfolios/<slug>/<stem>/`
  for its figures and `.bib`, the layout attachments already use; `artefacts.rs` extends ownership
  from "an evidence stub with `kind`" to "a stub, or a Typst note", still established positively by
  reading the file. The same holds under `concepts/`.

### 5.8 Reading and revising

- `read_note` returns the source; `cdno open` resolves a Typst note to its `.typ` file as it does a Markdown one. The DTO gains `format`; `body` is
  the source **after** the metadata block (the bytes after the label, so a leading comment is
  kept), `frontmatter` is the dictionary as JSON, `headings` are the `= ` headings. `content_hash`
  is over the whole file as today.
- `revise_note` **whole-body** (`body` + `expected_hash`) works on a Typst concept unchanged: the
  new body replaces everything after the metadata block; a new body carrying `<cdno>` is refused.
  The skill reads `format` from `read_note`, re-emits the preamble with the body, and confirms that
  only the intended region changed, since every equation is re-emitted with it.
- `revise_note` **by section** is **not supported for Typst in v1** and is refused with a rejection
  whose `remedy` names the whole-body form. Typst headings are in the tree, so a section's byte
  span is computable, but the Markdown section machinery (`replace_section`, `ensure_section`,
  trailing-heading rules) is what RFC 0002 built on and it is Markdown's. Section revision for
  Typst is a candidate second stage (§9 D7).
- `set_frontmatter` edits a value in the block (§5.3).

### 5.9 Which types admit Typst, and where it is configured

The engine reads Typst notes wherever the classifier is told to; the domain decides where that is.
A minimal table, parsed in core's `VaultConfig` like the rest of `config.toml`:

```toml
[typst]
# Types whose notes may be written in Typst. Absent means the default below.
note_types = ["evidence", "concept"]
```

- **The default is `evidence`, plus `concept` when the vault declares it.** The implicit default is
  never validated against, so a vault that never declared `concept` opens as before. `[]` turns the
  feature off: no `.typ` is parsed, a labelled one gets the Warning below, and the rest stay
  attachments.
- An **explicit** list is validated: a name must exist (built-in or declared), and the registry
  refuses `daily`, `weekly`, `monthly`, `project`, `action`, `commitment`, `inbox`, `tracking`,
  `stewardship`, `portfolio` and `question`: Typst is admitted for evidence and custom types only
  (§9 D1). The refusal list and the
  `admits_typst` check live on the **type registry** in `cdno-domain` — a custom type such as
  `concept` is not a `NoteType` variant — and the engine stays ignorant of it.
- A labelled `.typ` in a folder the switch **excludes** is reported by the classifier as a
  **Warning**, so turning the feature off after Typst notes exist does not make them vanish
  silently.
- Nothing machine-specific belongs here: no compiler path, no package root, no fonts. Those are
  Typst's own environment and `$PATH`, per machine, and `config.toml` syncs between machines.
- Rendering keys (`output`, `targets`) are **not** added by this RFC; the table exists so the later
  rendering RFC has somewhere to put them.

### 5.10 Lint

New rows, format-aware. The two marked *classifier* concern files not in the index and are raised
from the classifier's report (§5.1), since the lint walk is index-driven:

| Row | Level | When |
|---|---|---|
| metadata block missing a required key | Error | `type` absent, or `title` absent on a type that requires it |
| metadata block not literal | Error | any value outside strings, string arrays, booleans, numbers |
| metadata block not first or not top-level | Error | anything but whitespace or comments before it, or nested |
| metadata present, label missing or mistyped | Error, *classifier* | §5.2 |
| `<cdno>` present but not the call's sibling | Error | §5.2; found by the parse |
| link interior not one inline run | Error | an unescaped markup character inside `[[…]]` (§5.5) |
| Typst note does not parse | Warning | an Error node anywhere in the tree; creation already refuses it, so this catches a later hand edit; a Warning by the lint convention, since nothing downstream trips on it in v1 |
| stem shared between `.md` and `.typ` | Error | §5.7; the message names the Obsidian cause |
| Typst file neither note nor attachment | Warning, *classifier* | §5.2 |
| labelled `.typ` in a folder the switch excludes | Warning, *classifier* | §5.9 |
| dangling link, unknown field, and the other existing rows | as today | now over Typst notes too |

"Does not compile" and "package not installed" are **not** lint rows in this RFC: both need the
compiler, which is the rendering phase.

### 5.11 What is lost, stated

- **Obsidian** resolves `[[stem]]` only to `stem.md`. Every link to a Typst note is dead there,
  including the engine's own `evidence created` and `revised` log lines and every backlink, and
  clicking one **creates an empty `stem.md`** beside the `.typ`, which is the stem clash of §5.7.
  Evidence and concepts written in Typst are read through `cdno open`, an editor, or the compiled
  PDF. This is accepted for the two types in scope and is the reason the scope is two types; the
  docs-site page says it in those words.
- **Markdown-only tooling** — `cdno lint --fix`'s Markdown repairs, the section verbs, inline tags,
  template variables, the concept scaffold — does not apply to Typst notes in v1. Each is listed
  above with its rejection.
- A **search hit** inside a Typst note shows Typst syntax in its snippet.
- **Adoption scans only the adopted file.** A sibling `.typ` copied with the folder is not scanned,
  so a `/`-rooted or otherwise unresolvable path inside it passes adoption and fails when the
  chapter is compiled. The folder is the adopted file's own directory, everything in it comes
  along, and a symbolic link in it is refused.
- **A listed function used as a value is not seen.** `("a.png",).map(image)` or `let load = read`
  followed by `load("x.txt")` reach for a file the adoption scan cannot find, so the file is not
  copied and the path is not rewritten. The scan reads calls, not data flow.
- **A rename rewrites only the note's own literal relative paths into its assets folder**: a
  non-literal or `/`-rooted path into that folder, a `../<old>/…` path in a `.typ` file inside it,
  and another note's `#image("…/<old>/…")` into the moved folder are left as written and break on
  the move, with no warning.
- A Typst **evidence** body is frozen as written; a wrong package version has no remedy until the
  rendering RFC (§9 D16).

## 6. Detailed design

### 6.1 `cdno-core`

- `paths.rs`: `NoteFormat { Markdown, Typst }`, `note_format`, `note_address`, `note_candidates`
  (§5.1). The permitted folders for Typst are a value threaded to the classifier the way
  `IgnoreSet` is threaded to reconcile's callers today (the vault's composition root, `notes.rs`,
  the domain's reconcile, the HTTP server, the CLI watcher) — not injected into the store, whose
  job is not classification.
- `typst.rs`, the only module that names `typst_syntax`: `metadata_block(source) ->
  Result<(Range<usize>, serde_json::Value), TypstError>`; `headings(source)`;
  `wikilink_spans(source)` over inline runs and block strings (§5.5); `relative_path_literals(source)`
  for adoption (§5.7); `preamble_span(source)` (for the rendering RFC's verb, §9 D16; built when
  needed); the well-formedness check (no Error node); and a **metadata-syntax module** with the
  operations the YAML side has today — rewrite a scalar at its span, append to and remove from a
  list value, insert a missing key, "changed only at these spans" — plus the **literal writer**
  (escaping, `("x",)`, key insertion). Compile-time cost and added crates are recorded by S0.
- `frontmatter.rs`: `Frontmatter::from_typst` producing the same type as `Frontmatter::parse`;
  and **`parse_note(path, raw)`**, dispatching on `note_format`, as the one entry every reader
  uses. The six core call sites and the roughly sixty-five domain call sites of
  `Frontmatter::parse` that can see an evidence or concept path move to it (§6.2); a domain
  reader that still calls `Frontmatter::parse` on such a path would fail on the first Typst note
  and, in `get_portfolio_contents`, fail the whole call.
- `extractors.rs`: `wikilink_spans` dispatches on format; `rewrite_wikilink_addresses` and
  `differs_only_by` are span-based already; `resolve_one` uses `note_candidates`.
- `artefacts.rs`: `owning_artefact_stub` → `owning_note`, recognising a Typst note as an owner.
- `reconcile.rs`, `transaction.rs`: through the classifier and the op-keyed add/remove sets; the
  FTS title for a Typst note is the block's `title`.
- `config.rs`: the `[typst]` table with its default (§5.9); validation of an explicit list is the
  domain's.
- **`typst-syntax`** pinned to an **exact, recent** version (`=0.x.y`): the language is backwards
  compatible but the crate's `SyntaxKind` and node API are not semver-stable across 0.x, and a
  parser reads only the syntax it postdates — an old parser turns new syntax into Error nodes and
  may lose the links and headings after one. The edition already requires Rust 1.85 or later, so
  the MSRV is unlikely to bind. No parse cache: the byte search of §5.2 is enough.

### 6.2 `cdno-domain`

- `type_registry.rs`: `admits_typst(type_name, config)`, the refusal list, validation of an
  explicit `[typst] note_types`.
- Every reader that can see an evidence or concept path — `notes.rs` (`read_note`), `search.rs`,
  `portfolios.rs` (`get_portfolio_contents`), `index_entry.rs` (used by `revise_note`),
  `rename.rs` and the rest — through `parse_note` (S3).
- `vault/portfolios.rs` (`file_evidence`) and `vault/custom_notes.rs`: the `format` argument, the
  block writer, field transport, the `<cdno>`-already-present refusal, the creation-time
  well-formedness check, stem exclusivity (S4, S5); adoption with the asset imports (S4b).
- `vault/revise.rs`: whole-body for Typst, body slice after the label; section →
  `DomainError::UnsupportedForFormat` with a `remedy`. `ensure_revisable` is unchanged: evidence
  stays refused in both formats.
- `vault/set_frontmatter.rs`: dispatch to the Typst metadata-syntax module.
- `vault/rename.rs`: a Typst note as the renamed note (move with its assets folder, alias append
  through the Typst module) and as a referrer (S3b); the referrer snapshot through the shared
  classifier.
- `vault/lint.rs`: the rows of §5.10, including surfacing the classifier's report.
- `vault/log.rs`, `notes_section.rs`, `monthly.rs`, `tracking.rs`: through `note_address`.

### 6.3 `cdno-cli`

- `cdno file --format typst` with `--body-file <path>` or `--content`, and `--attach <file>.typ`
  for adoption (the CLI reads the file and enumerates the assets folder); `cdno note create
  --format typst`. Flags-and-prompts: `--format` is an `Option`, defaulting to `markdown`, never
  prompted (a Typst body is not typed at a prompt).
- `cdno open`: resolves to the `.typ` file, as before; `cdno lint`: the new rows; the watcher through
  `note_format`.

### 6.4 `cdno-mcp`

- `FileToPortfolioInput.format`, `CreateCustomNoteInput.format` (`Option<String>`, documented
  values); `NoteDto.format`. For adoption the server reads the file and enumerates the assets
  folder, under the same "the file must be on the server" limit `attach` has today.
- Rejection codes, each with a `remedy` as the focus errors carry: `format_unsupported_for_type`,
  `body_carries_metadata_block`, `body_does_not_parse` (naming the first Error node's line),
  `link_needs_escape`, `vars_unsupported_for_typst`, `stem_taken_by_other_format`,
  `section_revision_unavailable_for_typst`, `asset_path_outside_folder`, `asset_path_not_literal`.
- Server instructions, one paragraph: Markdown is the default and `typst` is chosen only when the
  person asks or the material is already Typst; evidence and concepts may be Typst, deliverables
  are not filed to a portfolio in either format; the engine writes the metadata and no title
  heading; the body is complete Typst, preamble included, copied from the newest Typst sibling
  (`read_note`) when the person has not supplied one, since nothing in the vault holds "my
  preamble"; inside `[[…]]` in a `.typ` body backslash-escape any markup character; tags go in
  metadata; a Typst evidence body is frozen as written.

### 6.5 Documentation and skills

- `docs/design.md`: note formats, the metadata block, the two-type scope, adoption, the losses of
  §5.11, and the rule that a project's deliverable lives where the project's work lives — the
  forge or the repository, linked from the map's `## Links` — and is not filed to a portfolio in
  either format.
- `CLAUDE.md`: the invariant "Markdown is the source of truth" becomes "plain text on disk is the
  source of truth"; the classifier named as the one place that decides what is a note.
- `docs-site`: a *Typst notes* page under the reference; `cli/file.md`, `cli/note.md`,
  `configuration.md`, the method page's changelog of practices (RLM 1.3, §9 D13).
- `examples/skills/references/LINKING-RULES.md`: the one-line escape rule and the no-inline-tags
  rule.
- `cuaderno-flow`: `file-to-portfolio` (when to choose `format: typst`; "is this the thing you
  delivered?" before filing a Typst document; adoption over retyping; never `attach` a `.typ` that
  should be a note) and `concept` (Typst for maths-heavy concepts; whole-body revision with the
  region check; promotion from `## Notes` translates Markdown or LaTeX maths into Typst).

## 7. Compatibility

- **No existing file changes meaning.** Every `.typ` in a vault today lies inside an attachment
  folder or beside no stub; none carries `<cdno>`; all remain attachments, and the loose ones gain
  a lint Warning they did not have.
- **No index schema change** in this RFC.
- **Vaults without the table** behave as if `note_types = ["evidence"]` plus `concept` when
  declared, which is a change only if such a vault already holds a labelled `.typ` under
  `portfolios/` or `concepts/`, which none does.
- **Second clone** (RFC 0007 §5.8): a Typst note is a file like any other; `merge=union` on the
  journal is unaffected.
- **MCP clients** that ignore `format` get Markdown, as today.
- **Typst ≥ 0.12** for the prepended block to compile with templates that set `document`
  (confirmed by S0, §5.7).

## 8. Implementation plan — staged

Each stage is a PR on its own, green and reviewed before the next.

- **S0 — the spike (not merged).** One authored evidence note, one adopted deck with a figures
  folder, and one concept, on a scratch vault; a throwaway branch that indexes them, searches,
  lints, renames the concept and the project named in `origin`, and renames the Typst note itself.
  It measures: the `typst-syntax` parse cost per file and the byte-search fast path; compile time
  and the crates added; whether the label-after-call shape of §4 is stable in the tree; that the
  `\#` round trip through rename keeps the backslash out of the address span; and the compile
  diff of §5.7 through a template that sets page and document, for a prepended body and for an
  adopted file. Findings amend this RFC.
- **S1 — helpers and classifier.** §5.1, no Typst behaviour; the twenty sites replaced or
  annotated; the transaction keyed on its ops; the shared classifier used by reconcile and the
  rename snapshot.
- **S2 — the Typst reader.** `typst.rs` with its tests: the metadata block (accept, and each
  refusal), headings, links over inline runs and block strings including escapes and anchors, the
  well-formedness check, the metadata-syntax operations and the literal writer, relative-path
  literals. No caller yet.
- **S3 — Typst notes in the engine.** The classifier reads `.typ` under the permitted folders;
  `parse_note` and every domain reader through it (read, search, portfolio contents, the FTS
  title); `artefacts.rs` ownership; `[typst]` config and the registry check; the lint rows,
  including the classifier's report. Evidence notes placed by hand are indexed; nothing creates
  them yet.
- **S3b — renaming Typst notes.** A Typst note as the renamed note and as a referrer; alias append
  through the Typst module; tested on the memory store.
- **S4 — authored evidence.** `format` on `file_to_portfolio` and `cdno file`; the block writer
  and field transport; the creation-time refusals; stem exclusivity; docs for filing; the
  `file-to-portfolio` skill.
- **S4b — adoption.** `attach` with `format: typst`; the interface layers read the file and
  enumerate the assets folder; the relative-path check and rewrite; the imports before the write;
  the skill's "adopt over retype" rule.
- **S5 — concepts.** `format` on `create_custom_note` and `cdno note create`; whole-body
  `revise_note` with the body slice; the section refusal; `set_frontmatter` on the block; the
  `concept` skill.
- **S6 — documentation and method.** `design.md`, `CLAUDE.md`, the docs-site page, the method page
  (RLM 1.3, §9 D13), LINKING-RULES, the server instructions.

The task breakdown — thirty-two tasks with deliverables, probes, dependencies and the tracks
that run in parallel — is the [implementation plan](0008-implementation-plan.md).
- **Later, separate RFC — rendering.** `cdno render`, the `typst` binary on `$PATH` with a minimum
  version, output folder, targets, "does not compile" and "package missing" as lint Warnings, the
  `imports` column, the preamble verb of D16, `read_note` pointing at a compiled PDF, serving PDFs
  over MCP (a vault-anywhere concern).

## 9. Decisions and open questions

Settled on the precursor discussion, restated for the record; the review raised no objection:

- **D1 — scope: evidence and concept only.** The machine-written types keep Markdown. The draft's
  sentence that a report needed as a daily or project artefact "is filed as evidence instead" was
  deleted by the review as bending the filing test: **a project's deliverable lives where the
  project's work lives, the forge or the repository, linked from the map's `## Links`, and is not
  filed to a portfolio in either format.** The `file-to-portfolio` skill asks "is this the thing
  you delivered?" before choosing `format: typst`. Agreed by the review.
- **D2 — the source is the note; metadata is a literal `#metadata((…)) <cdno>`; a `.typ` is a
  note only with the label.** Rejected: `typst query` (compiles), embedding the compiler,
  attachment-only, a cdno DSL over Typst, vendoring packages, freezing templates, committing PDFs
  as the norm.
- **D3 — appearance is not content.** Template drift is accepted; a note that no longer compiles
  is a warning (later phase), never frozen against.

Proposed by the draft and settled by the review:

- **D4 — the literal rule (§5.3).** Agreed, resting on "readable without evaluation" alone: the
  draft's second justification, exact spans for a masked hash, fell with §3.3; exact spans are
  still needed for in-place edits.
- **D5 — the engine writes the block; the skill supplies the body (§5.7).** Agreed, with the
  conditions the review added: minimum Typst 0.12, the S0 compile diff, field transport
  specified, and a title in a `show` rule acknowledged as appearance beside the block's.
- **D6 — a malformed block is a parse error, not a demotion (§5.2, §5.3).** Agreed and extended:
  a present block with a missing, mistyped or nested label is an Error from the classifier.
- **D7 — concepts start whole-body only (§5.8).** Agreed on three conditions: the rejection
  carries a `remedy`; the skill branches on `format` and confirms only the intended region
  changed; promotion translates the maths.
- **D8 — escapes (§5.5).** The draft's `\#` alone was too narrow. Agreed: the writer's rule is
  "escape any markup character, over-escaping harmless"; the machine's rule is structural (one
  inline run); the scanner decodes Escape leaves by their lexed value and keeps Shorthand and
  SmartQuote source bytes; anchors match heading source text.
- **D9 — inline tags are not read from Typst text; tags live in the metadata array.** Agreed; the
  block is excluded from full text so the two formats search alike.
- **D10 — stem exclusivity across formats (§5.7).** Agreed; the lint message names the Obsidian
  cause.
- **D11 — `[typst] note_types` (§5.9).** Agreed as one switch in one place, with two corrections:
  the default is `evidence` plus `concept` when declared and is never validated against; the
  check lives on the type registry, not `NoteType`; a labelled `.typ` the switch excludes gets a
  Warning.
- **D12 — rendering is a separate, later RFC.** Agreed; a compiled PDF may sit in the note's
  assets folder meanwhile.
- **D13 — method version.** The method page says the notes are Markdown; "plain text" with Typst
  admitted for two types is a change to the method's text. Proposed: a minor method version with
  one sentence on the method page, as RFC 0007 did; the sentence also keeps deliverables out of
  portfolios. Two seats agreed, one was indifferent. **Decided 2026-11-08: the minor version.**
  RLM 1.3, one sentence in the method page's changelog of practices, stamped last among the code
  changes (plan T31). The deliverable sentence belongs on the method page whatever the format
  question, and that alone changes the method's text.
- **D14 — full text indexed raw in v1 (§5.4), block excluded.** Agreed.
- **D15 — the `imports` column.** The draft proposed it now; the review deferred it to the
  rendering RFC: nothing reads it in v1, and the parse is cheap enough to run on demand.

Raised by the review:

- **D16 — correcting a Typst evidence note's preamble.** Not in v1: nothing compiles, so a wrong
  package version costs nothing yet. The rendering RFC decides the verb, and the review recorded
  two positions for it. One seat: the verb may rewrite the whole preamble — `#import`, `#set` and
  `#show` between the block and the first other node — because those shape rendering and record
  nothing observed, so they are appearance under D3. Another: the version string of a top-level
  `#import` only, because a `show` rule can change the text a reader sees. One rule is shared and
  written now: **`#let` is content, and no verb on evidence ever touches it.** "File it again" is
  rejected as a remedy: it costs a slug and splits the backlinks. **The maintainer's lean,
  recorded 2026-11-08 for the rendering RFC: the narrow scope first** — the version string of a
  top-level `#import` — because widening a verb's scope later is reversible and narrowing it
  after notes have been rewritten is not. The rendering RFC may widen it with the first
  position's argument; `preamble_span` is exposed either way, so neither choice costs a second
  parse.

Raised as open by the review; the maintainer's positions, 2026-11-08:

- **O1** — whether `read_note` should return a text-only projection beside the source for agents
  that do not read Typst well. **Deferred until S4 has shipped** and an agent is seen to stumble;
  decided on evidence, not in advance. Needs no schema change when it comes.
- **O2** — whether a Typst evidence note's appended revision should use a heading of a fixed form
  (`= Revision <date>`) so the index can list revisions, or stay free-form as Markdown's is.
  **Free-form in both formats in v1.** Parity between the formats matters more than the listing;
  a cross-format revision heading is a small RFC of its own if the index ever needs it.
- **O3** — whether `cdno lint --fix` should learn any Typst repair in v1. **No.** The candidates
  (adding a missing label, escaping a character inside a link) each guess at intent on a note
  that may be frozen.
- **O4** — whether the next count of Typst filings should record evidence and deliverables
  separately, so §2.1's second row has a number. **Yes, if a count is ever taken again — and
  it is not a task.** The measurements in §2.1 were aggregate counts taken outside the
  repository; a later count follows the same rule, records two integers and nothing else, and no
  stage of the implementation waits on it. Development is agnostic to any particular vault.

## 10. Verification

- A hand-written Typst evidence note under a portfolio with a sibling assets folder: after reindex
  it is in `get_portfolio_contents`, its text is searchable, its `[[concepts/…]]` is a backlink on
  the concept, its headings are listed by `read_note`, and the assets folder's files are not notes.
- The same file with the block nested, non-literal, second in the file, or missing `type`: indexed
  as a note with a parse error and one lint Error naming the rule; never silently an attachment.
  With the label mistyped or missing: one classifier Error on the file.
- A `.typ` under `portfolios/` without the label and with an owning stub: an attachment, as before;
  without a stub: a lint Warning, not indexed, and never parsed (the byte search decides). Inside
  an owned folder a `.typ` is never read, labelled or not.
- A rename of the concept the evidence note links to, and a rename of the project named in its
  `origin`: afterwards both addresses are rewritten, a byte comparison shows changes only inside
  the two references, and `cdno lint` is clean on this clone and on a clone that only pulled.
- A rename of the Typst note itself: the file and its assets folder move, every referrer is
  rewritten, the alias is in the block, and the index rebuilt from scratch agrees.
- `[[slug\#Heading]]` resolves to the heading; `[[slug#Heading]]` is refused at creation and, if
  hand-written, one lint Error and no edge; `[[x]]` inside a raw block, a comment, a non-metadata
  string literal and an equation: no edge; `[[x]]` inside the block's `origin` string: one edge.
- An anchor written in a Markdown daily resolves to a `= ` heading by source text; `--` in a slug
  matches as written.
- Adopting a `.typ` with a figures folder: the figures are copied, every relative path literal is
  rewritten to `<stem>/…` and listed, the original file is untouched; a path outside the folder,
  a non-literal path, a `/`-rooted path, an Error node or a `<cdno>` block in the file: refused,
  vault byte-identical.
- Filing a `.typ` whose stem matches an existing `.md`, whose body carries `<cdno>`, whose body
  does not parse, or with `vars`: refused with the named code and `remedy`, vault byte-identical.
- `revise_note` whole-body on a Typst concept: logged `revised [[concepts/<slug>]] — <reason>`,
  the block untouched; by section: refused with the named rejection; on a Typst evidence note:
  refused as for Markdown.
- `[typst] note_types = []`: no `.typ` is parsed, every existing attachment behaves as before, and
  each labelled `.typ` not in an owned folder gets one Warning. A vault with no `concept` type and no table opens clean.
- After S1, `grep` for `OsStr::new("md")` and `".md"` in non-test source finds only the three
  helpers, the classifier, and the annotated Markdown-on-purpose sites.
