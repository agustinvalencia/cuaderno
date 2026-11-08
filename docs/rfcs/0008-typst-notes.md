# RFC 0008 — Typst notes: a `.typ` source as a first-class evidence or concept note

| | |
|---|---|
| **Status** | Draft — for review on the PR. Implementation plan to follow once the review agrees, as RFC 0004, 0005 and 0007 had. |
| **Tracked by** | this PR |
| **Affects** | `cdno-core` (a new `typst.rs`, `extractors.rs`, `hash.rs`, `reconcile.rs`, `transaction.rs`, `artefacts.rs`, the watcher, one migration), `cdno-domain` (`note_type.rs`, `vault/portfolios.rs`, `vault/custom_notes.rs`, `vault/revise.rs`, `vault/lint.rs`, `vault/rename.rs`, `vault/note_ref.rs`, `config.rs`), `cdno-cli` (`--format` on `cdno file` and `cdno note create`), `cdno-mcp` (one field on two tools, `read_note`, server instructions), `docs/design.md`, `CLAUDE.md`, `docs-site`, the `file-to-portfolio` and `concept` skills |
| **Related** | #154 and #183 (attachments), RFC 0002 (concept notes), RFC 0007 (renames rewrite references; the masked frozen-prefix hash), #225 (globally unique stems), #215 (last-segment resolution), #211 (`cdno lint --fix`) |

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
- The reader is general and lives in `cdno-core`; which types may be written in Typst is a domain
  rule with a config switch, so widening it later is not an engine change.

Rendering to PDF is out of scope for this RFC. It is a later phase (§9 D12) that depends on nothing
here except the file being a note.

### 1.1 How you will use it

Filing a Typst note into a portfolio, through the `file_to_portfolio` tool or the CLI:

```sh
cdno file --portfolio surrogate-error-budget \
  --source "Error budget derivation for the surrogate model, 2026-11-08" \
  --origin "[[projects/surrogate-model]]" \
  --format typst --body-file derivation.typ
# -> portfolios/surrogate-error-budget/2026-11-08-error-budget-derivation-for-the.typ
```

`derivation.typ` is whatever the writer's template needs — an `#import`, a `#show` rule, headings,
equations, figures under a sibling folder. The engine writes the metadata block at the top:

```typst
#metadata((
  type: "evidence",
  title: "Error budget derivation for the surrogate model",
  created: "2026-11-08",
  source: "Error budget derivation for the surrogate model, 2026-11-08",
  portfolio: "surrogate-error-budget",
  origin: "[[projects/surrogate-model]]",
)) <cdno>

#import "@preview/some-template:0.3.0": report
#show: report.with(title: "Error budget derivation for the surrogate model")

= Setting

The surrogate's error splits as in [[concepts/bias-variance-split]] …

$ E[(hat(f) - f)^2] = "bias"^2 + "variance" + sigma^2 $
```

From then on the note behaves as evidence does: it is append-only past its frozen prefix, it
appears in `get_portfolio_contents` and search, `[[concepts/bias-variance-split]]` is a backlink
on the concept, a rename of the concept rewrites that address inside the frozen prefix without
tripping the append-only check, and `cdno lint` reports a dangling link or an edited prefix exactly
as for a Markdown evidence note. A concept is created the same way, with `--format typst` on
`cdno note create` or `format: "typst"` on `create_custom_note`, and revised whole with
`revise_note`.

What does not change: `cdno file --attach deck.typ` still files an attachment with a stub, and
every stub and attachment already in the vault is untouched for ever.

## 2. Motivation

### 2.1 The need, counted

The question put to the vault was not "would Typst be nice" but "how often does the gap cost
something". Three measurements, taken on 2026-11-08 over one working vault:

| Measurement | Count |
|---|---|
| Typst-built documents filed as attachments, by month (Jul, Sep, Oct 2026) | 1, 5, 13 |
| Attachment stubs whose `kind` records the derived PDF rather than the Typst source | 13 of 14 |
| `.typ` files in the vault, all under `portfolios/` | 58 |
| Evidence notes whose content is a reusable concept, and that need Typst's mathematics | 2 of the 8 strongest candidates |

The first row is the one that matters: filing a Typst document is now a weekly act, and growing.
The second row shows what the attachment route does to it: the thing recorded is the PDF, the
source is incidental, and the stub's abstract has grown into a slide-by-slide summary because it
is the only searchable surface. The third row says the vault already holds the sources; nothing
about them is indexed. The last row is the concept need: small, but it is exactly the concepts
Markdown serves worst, the ones that are mostly equations.

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
3. **Links as plain text**, `[[slug]]`, which Typst markup leaves alone.
4. **Targeted edits** — append past a frozen prefix, replace a whole body — which are byte
   operations on a text file.

What *is* Markdown-specific is the section machinery the machine-written types rely on: a daily
note's `## Logs`, a project map's `## Current State`, a tracking table. Those types are not in
scope (§9 D1), so none of that machinery needs a second syntax.

## 3. Background

### 3.1 How a file becomes a note today

Reconciliation lists the vault and indexes every file with an `.md` extension that is not under an
`ignore` glob and not inside an attachment's folder (`reconcile.rs`). The same `.md` test appears
wherever a path must be classified as a note: in the transaction's re-resolution of links
(`transaction.rs`), the rename's referrer scan (`rename.rs`), lint's file walk (`lint.rs`), the
watcher, the note-reference resolver (`note_ref.rs`) and the method migrations (`migrate.rs`) —
about a dozen sites, each spelling the test out. Section 5.1 makes that one function.

### 3.2 Attachments and stubs (#154)

`cdno file --attach` imports a file to `portfolios/<slug>/<stem>/<file>` and writes an evidence
stub `portfolios/<slug>/<stem>.md` carrying `kind: <pdf|image|typst|…>` and the abstract as its
body. `artefacts.rs` makes the folder belong to the stub by *location*: anything inside it is not
a note, however it is named, and ownership is established positively by reading the stub's
frontmatter, never inferred from the path shape alone. A `.typ` source attached today is such a
file: present, owned, unindexed.

### 3.3 Append-only and the masked hash (RFC 0007)

Evidence is append-only. The index keeps a snapshot of each append-only note's frozen prefix —
its size and hash — and lint reports an Error when the prefix no longer verifies. RFC 0007 let a
rename rewrite references inside a frozen prefix by hashing the prefix with its references
*masked*: every wikilink address and every scalar under the type's `reference_keys` (`portfolio`,
`origin`, …) becomes one fixed token before hashing (`hash.rs`, `mask_references`). The reference
scalars are found with `split_frontmatter` and `frontmatter_scalar_spans`, which know YAML. A
Typst note's `portfolio:` lives in a Typst dictionary, so today's masking would not find it, a
rename of the portfolio would change the frozen prefix's hash, and lint would call the note
edited. The precursor discussion assumed the frozen-prefix check "works on `.typ` unchanged";
that was true before RFC 0007 and is not now (§5.6).

### 3.4 How links are parsed

`wikilink_spans` scans a body for `[[…]]`, skipping fenced code, inline code and inline HTML so a
link quoted in a code sample is not an edge. Typst has its own quoting constructs — raw blocks,
line and block comments, code mode, strings, maths — and its own hazard: `#` begins an embedded
expression in markup, so `[[slug#Heading]]` is not plain text in a Typst note (§5.5).

### 3.5 Titles, headings and sections

A Markdown note's title is its first `# ` heading (`first_h1`); its sections are `## ` headings,
and `revise_note` replaces one section by heading. Typst headings are `= `, `== `, …, but a Typst
document's title is usually an argument to its template's `show` rule, not a heading in the body.

### 3.6 Concepts (RFC 0002)

A concept is a custom type the vault declares in config (`[note_types.concept]`), mutable,
refined in place with `revise_note` by section or whole, and created with `create_custom_note` or
`cdno note create`. Its template appends a scaffold after the body.

## 4. Terminology

- **Typst note** — a `.typ` file that carries a `<cdno>` metadata block and lies in a folder of a
  type that admits Typst. It is a note in every sense the index has.
- **Metadata block** — the `#metadata((…)) <cdno>` expression: a function call to `metadata` with
  one literal dictionary argument, immediately labelled `cdno`.
- **Attachment** — any other non-Markdown file, `.typ` included: imported bytes owned by a stub.
- **Format** — `markdown` or `typst`, a property of a note derived from its extension, never
  stored.
- **Frozen prefix, masked hash** — as RFC 0007 §5.3–5.4.

## 5. Proposal

### 5.1 One predicate for "is this path a note"

Before anything Typst-specific, the dozen `.md` tests of §3.1 become one function in `cdno-core`,
`is_note_path(path) -> Option<NoteFormat>`, returning the format or `None`. On its own this is a
refactor with no behaviour change — it answers `Some(Markdown)` for `.md` and `None` for the rest —
and it ships first and alone (§8 S1). Every later stage changes what that function answers, and
nothing else has to learn about extensions. A site that spells the extension out after S1 is a
review finding.

### 5.2 What makes a `.typ` file a note

A `.typ` file is a Typst note when **all three** hold:

1. It lies in the folder of a type that admits Typst (§5.9): under `portfolios/<slug>/` for
   evidence, under the declared folder for concept. A `.typ` anywhere else is never read.
2. It is not inside an attachment's folder (§3.2 applies unchanged).
3. Its source contains a metadata block (§4) at the top level of the document.

Condition 3 requires reading the file, and reconciliation does: for each `.typ` under a permitted
folder it parses the source with the Typst parser and looks for the block. That is the cost YAML
frontmatter already pays per Markdown note. A `.typ` that satisfies 1 and 2 but not 3 is an
attachment as today — but only if a stub owns its folder; a loose `.typ` under `portfolios/` with
neither block nor owning stub is reported by lint as a Warning ("Typst file is neither a note nor
an attachment"), the way an orphan file is today, and not indexed.

The detection is **by content, not by convention**, for the same reason `artefacts.rs` reads the
stub rather than trusting the path: a wrong answer in the "is a note" direction indexes a file as
a note with no metadata and reports a parse error, which is visible; a wrong answer in the other
direction drops a note silently.

### 5.3 The metadata block

```typst
#metadata((
  type: "evidence",
  title: "…",
  created: "2026-11-08",
  portfolio: "surrogate-error-budget",
  origin: "[[projects/surrogate-model]]",
  source: "…",
  tags: ("surrogate", "error-budget"),
)) <cdno>
```

Rules, each enforced at creation and checked by lint:

- **Exactly one** block per note, at the **top level** of the markup (not inside a content block,
  a function body or a `show` rule), and **first** in the file: only whitespace and comments may
  precede it. The engine writes it there; lint reports an Error when it is elsewhere, so that the
  frozen prefix (§5.6) always contains it.
- The argument is a **literal dictionary** whose values are **strings**, **arrays of strings**, or
  **booleans**. No identifiers, no function calls, no arithmetic, no nested dictionaries, no
  string concatenation. Reading it is then a tree walk with no evaluation, and the result maps
  onto the same `serde_json::Value` shape YAML frontmatter produces today, so everything
  downstream of `Frontmatter::parse` — the type registry, `extra_required`, schema validation,
  `set_frontmatter` — sees no difference. A block that breaks the rule is a **parse error**: the
  file is indexed as a note with a frontmatter error, the way a Markdown note with malformed YAML
  is, and lint reports it as an Error. It is not demoted to an attachment (§5.2, last paragraph).
- `type` and `title` are **required** in every Typst note. `title` is the note's title; a Typst
  note has no `first_h1` (§3.5), and the engine does not guess it from a `= ` heading or a
  template argument.
- Dates are strings in the vault's `YYYY-MM-DD` form, as in YAML. Typst's `datetime` is a
  function call and is refused by the literal rule.
- **The block is the frontmatter.** `set_frontmatter` rewrites a value inside it, byte-exactly, the
  way `frontmatter_edit.rs` rewrites a YAML scalar; it never re-serialises the dictionary, so a
  comment or the writer's key order survives. The locator that finds a key's value span is the
  Typst analogue of `frontmatter_scalar_spans` and the piece that §5.6 reuses.

A Typst document may hold other `#metadata` calls with other labels; only `<cdno>` is read.

### 5.4 What is indexed

- The note row: path, type, title, created, format-neutral frontmatter JSON, content hash — as
  today, from the block.
- **Full text**: the source as it is, minus nothing. Typst syntax leaks into search results (`#`,
  `=`, `$`), which is accepted in v1; a text-only projection for search is a later refinement and
  needs no schema change.
- **Headings**: every `= `, `== `, … heading, with its level, in the `headings` the index already
  keeps for Markdown notes, so `read_note` reports them and an anchored link to one can resolve.
- **Wikilinks** and **inline tags** (`#tag` is also Typst code — see §5.5; tags in a Typst note
  come from the metadata `tags` array only).
- **Imports**: every `#import "@<namespace>/<name>:<version>"` at the top level, as a list on the
  note row, read without compiling. It costs one column (append-only migration) and lets a later
  phase report "package not installed here" without a second list in the metadata.

### 5.5 Links in Typst markup

`[[slug]]` and `[[slug|label]]` are plain text in Typst markup, so the syntax is unchanged and the
precursor's intent holds. The scanner, however, is a new one over the parsed tree rather than a
character scan with Markdown exclusions:

- A wikilink is recognised in **text nodes of markup**, at any nesting (a content block inside a
  `#figure(…)` call is markup), and **not** in raw blocks or inline raw, line or block comments,
  code-mode expressions, string literals, labels, references (`@x`) or maths.
- **Anchors.** `#` starts code in markup, so `[[slug#Heading]]` cannot be written bare: Typst would
  parse `#Heading` as a variable and fail to compile. In a Typst note an anchored link is written
  with the escape Typst itself uses, `[[slug\#Heading]]`, and the scanner unescapes `\#` inside
  `[[…]]`. The same applies to `|` only if a template redefines it; it does not by default. Lint
  reports an unescaped `#` inside `[[…]]` as an Error, since the file will not compile either.
- **Inline tags.** `#concept` in markup is a call to a (probably undefined) function. Typst notes
  therefore carry tags only in the metadata array; the inline-tag extractor is not run over Typst
  text.
- The rename rewriter (`rewrite_wikilink_addresses`) rewrites addresses in a Typst note the same
  way, over the spans the Typst scanner reports; `differs_only_by` verifies the result as it does
  for Markdown.

How a `[[slug]]` *looks* when compiled is a rendering question for the template (a `show regex`
rule in a template can turn it into a link or a margin note) and is not the engine's concern.

### 5.6 Append-only, frozen prefix and renames

The plain frozen-prefix check is a byte comparison and needs no change: an evidence note in Typst
is frozen past its prefix like any other, and a dated revision appended at the end (a `= Revision
2026-12-01` heading) is an append.

The masked hash (§3.3) does need the Typst locator: `mask_references` takes the metadata-block
locator from the format rather than calling `split_frontmatter` and `frontmatter_scalar_spans`
directly, and for Typst the reference scalars are the string values of the `reference_keys` in the
`<cdno>` dictionary. Wikilink spans come from the Typst scanner (§5.5). Nothing about what is
masked changes for Markdown, so `MASK_VERSION` does not bump; the snapshot gains nothing, since
the format is derived from the path.

The prefix-locality argument of `mask_references` (a blank line between prefix and appendix keeps
the masked prefix stable) has a Typst equivalent: an unclosed raw block or block comment at the
end of the prefix could be closed by the appendix and change what is excluded from scanning. The
appender already separates with a blank line; for Typst notes lint additionally reports an Error
when the frozen prefix ends inside a raw block or a comment, which also fails to compile.

### 5.7 Creation

One verb per type, as today, with a format switch:

- `file_to_portfolio` gains `format: "markdown" | "typst"` (default `markdown`); `cdno file` gains
  `--format typst`. With `typst`, `content` is the **Typst body**: everything after the metadata
  block, template import and `show` rule included. `attach` and `format: typst` are mutually
  exclusive (`attach` files bytes; `format` writes a note).
- `create_custom_note` and `cdno note create` gain the same field, for the types that admit it.
- The engine **writes the metadata block** from the fields it owns — `type`, `title`, `created`,
  `portfolio`, `origin`, `source`, declared fields for a custom type — and prepends it to the body.
  The skill never writes `<cdno>` itself, so one authority stamps the fields in both formats, and
  a body that already contains a `<cdno>` block is refused ("body already carries a metadata
  block").
- **No engine template for Typst bodies in v1.** `.cuaderno/templates/*.md` and `[variables]` are
  Markdown's; a Typst body arrives complete from the skill, which chooses the Typst template
  (tufte, a report class, a diagram package) — the precursor's decision that templates are plain
  Typst and the skill's business. The concept scaffold (*Statement / Why it matters / See also*) is
  therefore not appended to a Typst concept. A later RFC may add `[typst] templates`; nothing here
  prevents it.
- The slug is derived as today (`slugify`, `--slug`/`slug` from RFC 0007); the extension is `.typ`.
- **Stem exclusivity.** Creation refuses a `.typ` whose stem matches an existing `.md` in the same
  folder, and vice versa, with the same rejection the rename uses for a taken slug. Two notes
  sharing a stem would make `[[stem]]` ambiguous under the stem rule (§3.4 of RFC 0007) and, worse,
  a stub `x.md` beside a note `x.typ` would look like the stub owns the note. Reconciliation
  indexes both if it finds both (nothing is dropped silently) and lint reports an Error.
- **Sibling assets.** A Typst note at `portfolios/<slug>/<stem>.typ` owns `portfolios/<slug>/<stem>/`
  for its figures and `.bib`, the layout attachments already use; `artefacts.rs` extends ownership
  from "an evidence stub with `kind`" to "a stub, or a Typst note", still established positively by
  reading the file. The same holds under `concepts/`.

### 5.8 Reading and revising

- `read_note`, `cdno open` and `cdno note show` return the source. The DTO gains `format`; `body` is
  the source **after** the metadata block, `frontmatter` is the dictionary as JSON, `headings` are
  the `= ` headings. `content_hash` is over the whole file as today.
- `revise_note` **whole-body** (`body` + `expected_hash`) works on a Typst concept unchanged: the
  new body replaces everything after the metadata block.
- `revise_note` **by section** is **not supported for Typst in v1** and is refused with a clear
  rejection ("section revision is not available for a Typst note; revise the whole body"). Typst
  headings are in the tree, so a section's byte span is computable, but the Markdown section
  machinery (`replace_section`, `ensure_section`, trailing-heading rules) is what RFC 0002 built
  on and it is Markdown's. Section revision for Typst is a candidate second stage (§9 D7).
- `set_frontmatter` edits a value in the block (§5.3).
- The append path for evidence (a revision appended past the frozen prefix) appends to the file as
  for Markdown, after a blank line.

### 5.9 Which types admit Typst, and where it is configured

The engine reads Typst notes wherever `is_note_path` says so; the domain decides where that is.
A minimal table:

```toml
[typst]
# Types whose notes may be written in Typst. Absent means exactly this default.
note_types = ["evidence", "concept"]
```

- The default is the RFC's scope. `[]` turns the feature off: no `.typ` is read, every one is an
  attachment, as today.
- A type listed here must exist (built-in or declared); config validation refuses an unknown name,
  and refuses `daily`, `weekly`, `monthly`, `project`, `action`, `commitment`, `inbox`, `tracking`
  and `stewardship`, whose section machinery is Markdown's (§9 D1). The refusal is a list in
  `cdno-domain`, so the engine stays ignorant of it.
- Nothing machine-specific belongs here: no compiler path, no package root, no fonts. Those are
  Typst's own environment and `$PATH`, per machine, and `config.toml` syncs between machines.
- Rendering keys (`output`, `targets`) are **not** added by this RFC; the table exists so the later
  rendering RFC has somewhere to put them.

### 5.10 Lint

New rows, all format-aware through `is_note_path`:

| Row | Level | When |
|---|---|---|
| metadata block missing a required key | Error | `type` or `title` absent |
| metadata block not literal | Error | any value outside strings, string arrays, booleans |
| metadata block not first or not top-level | Error | anything but whitespace or comments before it, or nested |
| unescaped `#` inside `[[…]]` | Error | §5.5 |
| frozen prefix ends inside raw or comment | Error | §5.6 |
| stem shared between `.md` and `.typ` | Error | §5.7 |
| Typst file neither note nor attachment | Warning | §5.2 |
| dangling link, edited frozen prefix, unknown field | as today | the existing rows, now over Typst notes too |

"Does not compile" and "package not installed" are **not** lint rows in this RFC: both need the
compiler, which is the rendering phase.

### 5.11 What is lost, stated

- **Obsidian** will not preview or follow links in a `.typ` note. Evidence and concepts written in
  Typst are read through `cdno open`, an editor, or the compiled PDF. This is accepted for the two
  types in scope and is the reason the scope is two types.
- **Markdown-only tooling** — `cdno lint --fix`'s Markdown repairs, the section verbs, inline tags,
  template variables — does not apply to Typst notes in v1. Each is listed above with its
  rejection.
- A **search hit** inside a Typst note shows Typst syntax in its snippet.

## 6. Detailed design

### 6.1 `cdno-core`

- `NoteFormat { Markdown, Typst }` and `is_note_path(&VaultPath) -> Option<NoteFormat>` in
  `paths.rs`, replacing every `.md` test (§3.1). The permitted-folder rule for Typst (§5.2 item 1)
  is injected as a `TypstFolders` value (the folders, not the type names) when the store is opened,
  so core needs no type knowledge.
- `typst.rs`: `parse_metadata_block(source) -> Result<(Range<usize>, serde_json::Value), TypstError>`
  (block span, dictionary); `metadata_scalar_spans(source, key)`; `headings(source)`;
  `wikilink_spans(source)` and `imports(source)`, all over `typst_syntax::parse`. One dependency,
  `typst-syntax`, pinned to the version of the oldest Typst the vault's notes are written for; the
  parser is backwards compatible across the 0.x line for the constructs used here.
- `Frontmatter::parse` gains a sibling, `Frontmatter::from_typst`, producing the same type; the
  six call sites (`transaction.rs`, `artefacts.rs`, `reconcile.rs` ×2, `markdown.rs`,
  `frontmatter_edit.rs`) dispatch on `NoteFormat`.
- `hash.rs`: `mask_references` takes a `MetadataLocator` (block span + scalar spans + wikilink
  spans) built per format; the Markdown locator is today's code.
- `extractors.rs`: `wikilink_spans` dispatches on format; `rewrite_wikilink_addresses` and
  `differs_only_by` are span-based already and need no change beyond the source of spans.
- `artefacts.rs`: `owning_artefact_stub` → `owning_note` recognises a Typst note as an owner (§5.7).
- `reconcile.rs`, `transaction.rs`, the watcher: use `is_note_path`; the watcher's extension
  filter follows it.
- `index.rs`: one append-only migration adding `imports` (JSON array) to `notes`. The `format`
  is not stored; it is derived from `path`.

### 6.2 `cdno-domain`

- `NoteType::admits_typst(&self, config)`; the refusal list of §5.9; `[typst]` in `config.rs` with
  validation.
- `vault/portfolios.rs` (`file_evidence`) and `vault/custom_notes.rs` (`create_custom_note`): the
  `format` argument, the block writer (`typst::write_metadata_block(fields)`, in core, since it
  is syntax), the `<cdno>`-already-present refusal, stem exclusivity.
- `vault/revise.rs`: whole-body for Typst; section → `DomainError::UnsupportedForFormat`.
- `vault/set_frontmatter.rs`: dispatch to the Typst scalar editor.
- `vault/lint.rs`: the rows of §5.10; `frozen_prefix_verifies` builds the locator per format.
- `vault/rename.rs`, `vault/note_ref.rs`, `vault/links.rs`: through `is_note_path`; no other change,
  which is the point of §5.1.
- `vault/reconcile.rs`, `migrate.rs`, `questions.rs`, `stewardships.rs`, `track.rs`: through
  `is_note_path`.

### 6.3 `cdno-cli`

- `cdno file --format typst --body-file <path>` (or `--content` as today); `cdno note create
  --format typst`. Flags-and-prompts: `--format` is an `Option`, defaulting to `markdown`, never
  prompted (a Typst body is not typed at a prompt).
- `cdno open`, `cdno note show`: print the source; `cdno lint`: the new rows.

### 6.4 `cdno-mcp`

- `FileToPortfolioInput.format`, `CreateCustomNoteInput.format` (`Option<String>`, documented
  values); `NoteDto.format`.
- Rejection codes: `format_unsupported_for_type`, `body_carries_metadata_block`,
  `stem_taken_by_other_format`, `section_revision_unavailable_for_typst`.
- Server instructions: one paragraph — evidence and concepts may be Typst; the engine writes the
  metadata; the body is complete Typst; anchored links escape `#`; tags go in metadata.

### 6.5 Documentation and skills

- `docs/design.md`: note formats, the metadata block, the two-type scope, the losses of §5.11.
- `CLAUDE.md`: the invariant "Markdown is the source of truth" becomes "plain text on disk is the
  source of truth"; `is_note_path` named as the one classification point.
- `docs-site`: a *Typst notes* page under the reference; `cli/file.md`, `cli/note.md`,
  `configuration.md`, the method page (§9 D13).
- `examples/skills/references/LINKING-RULES.md`: the `\#` escape and the no-inline-tags rule.
- `cuaderno-flow`: `file-to-portfolio` (when to choose `format: typst`; never `attach` a `.typ`
  that should be a note) and `concept` (Typst for maths-heavy concepts; whole-body revision).

## 7. Compatibility

- **No existing file changes meaning.** Every `.typ` in a vault today lies inside an attachment
  folder or beside no stub; none carries `<cdno>`; all remain attachments, and the loose ones gain
  a lint Warning they did not have.
- **The index schema** gains one column by append-only migration; `cdno reindex` once after the
  release, as any migration asks.
- **Vaults without the table** behave as if `note_types = ["evidence", "concept"]`, which is a
  change only if such a vault already holds a labelled `.typ` under `portfolios/` or `concepts/`,
  which none does.
- **Second clone** (RFC 0007 §5.8): a Typst note is a file like any other; `merge=union` on the
  journal is unaffected; the frozen-prefix check behaves identically on both clones.
- **MCP clients** that ignore `format` get Markdown, as today.

## 8. Implementation plan — staged

Each stage is a PR on its own, green and reviewed before the next.

- **S0 — the spike (not merged).** One evidence note and one concept note written by hand in
  Typst on a scratch vault; a throwaway branch that indexes them, searches, lints, and renames
  the concept while the evidence note links to it. The spike exists to measure three things
  before S1 is final: the `typst-syntax` parse cost per file, whether the label-after-call shape
  of §4 is stable in the tree, and the rename through the masked hash. Findings amend this RFC.
- **S1 — `is_note_path`.** The refactor of §5.1, no behaviour change, every `.md` test replaced.
- **S2 — the Typst reader.** `typst.rs` with its tests: metadata (accept, and each refusal),
  headings, links including `\#`, imports, scalar spans, the masked-hash locator. No caller yet.
- **S3 — Typst notes in the engine.** `is_note_path` answers `Typst` under the injected folders;
  reconcile, transaction, watcher, `artefacts.rs`, the migration; `[typst]` config; lint rows.
  Evidence notes placed by hand are indexed; nothing creates them yet.
- **S4 — evidence creation.** `format` on `file_to_portfolio` and `cdno file`; stem exclusivity;
  the block writer; docs for filing; the `file-to-portfolio` skill.
- **S5 — concepts.** `format` on `create_custom_note` and `cdno note create`; whole-body
  `revise_note`; the section refusal; `set_frontmatter` on the block; the `concept` skill.
- **S6 — documentation and method.** `design.md`, `CLAUDE.md`, the docs-site page, the method page
  if §9 D13 says so, LINKING-RULES.
- **Later, separate RFC — rendering.** `cdno render`, the `typst` binary on `$PATH` with a minimum
  version, output folder, targets, "does not compile" and "package missing" as lint Warnings,
  serving PDFs over MCP (a vault-anywhere concern).

## 9. Decisions and open questions

Settled on the precursor discussion, restated for the record and open to objection on the PR:

- **D1 — scope: evidence and concept only.** The machine-written types keep Markdown; a report
  needed as a daily or project artefact is filed as evidence instead.
- **D2 — the source is the note; metadata is a literal `#metadata((…)) <cdno>`; a `.typ` is a
  note only with the label.** Rejected: `typst query` (compiles), embedding the compiler,
  attachment-only, a cdno DSL over Typst, vendoring packages, freezing templates, committing PDFs
  as the norm.
- **D3 — appearance is not content.** Template drift is accepted; a note that no longer compiles
  is a warning (later phase), never frozen against.

Proposed here, for the review to settle:

- **D4 — the literal rule (§5.3): strings, string arrays, booleans; nothing computed.**
  Alternative: allow any expression and read what the parser can, ignoring the rest. Rejected
  because "readable without compiling" has to be a guarantee, not a best effort, and because the
  masked hash needs an exact byte span for every reference value.
- **D5 — the engine writes the block; the skill supplies the body (§5.7).** Alternative: the skill
  writes a complete `.typ` with `<cdno>` and the engine validates. Rejected because two writers of
  the same fields drift, and because `created`, `portfolio` and the slug are the engine's to stamp
  in Markdown today.
- **D6 — a malformed block is a parse error, not a demotion to attachment (§5.2, §5.3).**
  Fail visible.
- **D7 — concepts start whole-body only (§5.8).** Alternative: build Typst section revision now,
  since the heading spans are available. Deferred because the Markdown section machinery is not
  shared and the concept need counted is two notes.
- **D8 — anchored links escape `#` as `\#` (§5.5).** Alternatives: a different anchor separator
  in Typst notes (two syntaxes for one thing); a helper function `#note("slug", "Heading")` (the
  DSL D2 rejected). The escape is Typst's own and reads back exactly.
- **D9 — inline tags are not read from Typst text; tags live in the metadata array (§5.5).**
- **D10 — stem exclusivity across formats, refused at creation and an Error from lint (§5.7).**
- **D11 — `[typst] note_types` with the default equal to the scope, `[]` to opt out (§5.9).**
  Alternative: per-type `formats = [...]` under `[note_types.<name>]` and a new `[schemas.evidence]`
  key for the built-in. Rejected as two places for one switch.
- **D12 — rendering is a separate, later RFC.** Nothing here depends on a compiler.
- **D13 — method version.** The method page says the notes are Markdown; "plain text" with Typst
  admitted for two types is a change to the method's text. Proposed: a minor method version with
  one sentence on the method page, as RFC 0007 did. **For the maintainer.**
- **D14 — full text indexed raw in v1 (§5.4).** A text-only projection later, with no schema
  change.
- **D15 — imports indexed now, in one column, for the rendering phase to use (§5.4).**
  Alternative: defer the column to the rendering RFC. Proposed now because the parse is already
  being done and a migration is cheaper when the reader is written once.

Open, not proposed:

- **O1** — whether `read_note` should return a text-only projection beside the source for agents
  that do not read Typst well.
- **O2** — whether a Typst evidence note's appended revision should use a heading of a fixed form
  the appender writes (`= Revision <date>`), so the index can list revisions, or stay free-form as
  Markdown's is.
- **O3** — whether `cdno lint --fix` should learn any Typst repair in v1 (none is proposed).

## 10. Verification

- A hand-written Typst evidence note under a portfolio with a sibling assets folder: after reindex
  it is in `get_portfolio_contents`, its text is searchable, its `[[concepts/…]]` is a backlink on
  the concept, its headings are listed by `read_note`, and the assets folder's files are not notes.
- The same file with the block nested, non-literal, second in the file, or missing `title`: indexed
  as a note with a parse error and one lint Error naming the rule; never silently an attachment.
- A `.typ` under `portfolios/` without the label and with an owning stub: an attachment, as before;
  without a stub: a lint Warning, not indexed.
- A rename of the concept the evidence note links to, and a rename of the portfolio named in its
  `portfolio:`: afterwards the evidence note's masked hash verifies on this clone and on a clone
  that only pulled, a byte comparison shows changes only inside the two rewritten references,
  and `cdno lint` is clean.
- An edit inside the frozen prefix of a Typst evidence note that is not a reference: one lint Error,
  as for Markdown.
- `[[slug\#Heading]]` resolves to the heading; `[[slug#Heading]]` is one lint Error and no edge.
- `[[x]]` inside a raw block, a comment, a string literal and an equation: no edge.
- Filing a `.typ` whose stem matches an existing `.md`, or whose body carries `<cdno>`: refused,
  vault byte-identical.
- `revise_note` whole-body on a Typst concept: logged `revised [[concepts/<slug>]] — <reason>`,
  the block untouched; by section: refused with the named rejection.
- `[typst] note_types = []`: no `.typ` is read; every existing attachment behaves as before.
- Every `.md` test of §3.1 is gone: `grep` for `OsStr::new("md")` in non-test source finds only
  `is_note_path`.
