# Typst notes

A note can be written in [Typst](https://typst.app) instead of Markdown. The `.typ` source is the
note: its text is indexed, its `[[wikilinks]]` are edges, lint reads it, and `read_note` returns it.
Nothing is compiled; a PDF made from the source is a derived file, like the index.

Two kinds of note may be Typst: **evidence** and **concept** (the concept library). Every other
type stays Markdown. This is a configuration switch ([`[typst] note_types`](configuration.md#typst)),
and `list_note_types` reports `admits_typst` for each type. The default is evidence, plus concept
when the vault declares it.

Markdown remains the default everywhere. Choose Typst when the material is already Typst, or when
it is mostly mathematics or needs a template's typesetting. Rendering to PDF is not part of cuaderno:
`cdno` does not call the Typst compiler.

## What makes a `.typ` file a note

A `.typ` file is a note when all three hold, checked in this order:

1. It lies in the folder of a type that admits Typst: under `portfolios/<slug>/` for evidence,
   under the type's folder (`concepts/`) for a concept.
2. It is not inside another note's assets folder. A Typst note `x.typ` owns the folder `x/` beside
   it (its figures and bibliography); a file inside an owned folder is an attachment, and is never
   read, label or no label.
3. Its source carries the metadata block below, labelled `<cdno>`.

Every other `.typ` in the vault, every one that exists before you use this feature, stays an
attachment with the meaning it had. A `.typ` is parsed only when it passes all three. Short of
that it is, at most, searched for the bytes `<cdno>`.

A `.typ` that fails only the third condition is reported by [`cdno lint`](cli/lint.md#typst-notes)
(a warning, "neither a note nor an attachment"), or an error when it looks like a note whose label
is missing or mistyped (`<cdn>`). See [Troubleshooting](troubleshooting.md#my-typ-file-is-not-in-the-index).

## The metadata block

The block stands where YAML frontmatter stands in a Markdown note. The engine writes it; you do not.

```typst
#metadata((
  type: "evidence",
  created: "2026-11-09",
  source: "Error budget derivation for the surrogate model",
  portfolio: "surrogate-error-budget",
  origin: "[[projects/surrogate-model]]",
)) <cdno>
```

- **One block, first in the file**, at the top level of the markup. Only whitespace and comments
  may precede it.
- The argument is a **literal dictionary**. Values are strings, arrays of strings, booleans or
  numbers (an integer or float literal, optionally negative). No identifiers, function calls,
  arithmetic, nested dictionaries or `datetime`: dates are strings in `YYYY-MM-DD` form. This keeps
  the block readable without evaluating Typst.
- `type` is required. A concept also carries `title`. Evidence carries no `title` and is known by
  its `source`, as a Markdown evidence note is.
- Strings take Typst's string escapes only (`"` and `\`). An anchor inside a string is a bare `#`,
  as in `origin: "[[journal/2026/daily/2026-11-02#Error budget sketch]]"`.
- Tags come from the block's `tags` array. `#tag` in the body is not read, and the block itself is
  excluded from search, as YAML frontmatter is.
- `set_frontmatter` rewrites a value that is already in the block, byte for byte, leaving comments
  and key order alone. A key that is absent from the block is an error, not an insertion.

## Links, and the one escape rule

`[[slug]]` and `[[slug|label]]` are written as in Markdown. Anything in the markup (including a
`#figure(…)` content block) is read; raw blocks, comments, code, labels, references and maths are
not. The string values of the `<cdno>` block are read too, so `origin` is an edge and a rename of
the project rewrites it.

Typst gives several characters a meaning inside markup, so a link whose address or anchor contains
one cannot be written bare. The rule, for anyone writing a body: **inside `[[…]]` in a `.typ` file,
put a backslash before any Typst markup character** (`#`, `_`, `*`, `$`, a backtick, `<`, `@`,
`~`). Over-escaping is harmless.

```typst
See [[concepts/bias-variance-split\#Statement]] and [[surrogate-model]].
```

Also put a space before `[[` when it follows a `#…` expression or a reference: `#emph[a][[x]]` and
`@ref[[x]]` are not links. The engine cannot see that mistake, and the link is silently lost.

An anchor addresses a heading by its source text, compared as a Markdown anchor is: `= The *bias*
term` is `[[concepts/x\#The *bias* term]]`, never its rendered text.

## Creating a Typst note

### Written for the occasion

Over the CLI, `--format typst` with `--body-file` (the CLI also takes `--content`):

```bash
cdno file --portfolio surrogate-error-budget \
          --source "Error budget derivation" --origin projects/surrogate-model \
          --format typst --body-file derivation.typ
# Filed at portfolios/surrogate-error-budget/2026-11-09-error-budget-derivation.typ

cdno note create concept --title "Bias variance split" --format typst --body-file bvs.typ
# Created concepts/bias-variance-split.typ
```

Over MCP, `file_to_portfolio` and `create_custom_note` take `format: "typst"`. The body is
**complete Typst**: preamble, `#import` and `#show` lines included. The engine adds the metadata
block above it and no title heading; there is no template, no `--var`, and no concept scaffold.
The `origin` is given bare in the CLI (`projects/surrogate-model`); the engine wraps it.

A body is refused, with a `remedy`, when it already holds a `<cdno>` label, does not parse, or has
a `[[…]]` that is not one inline run (an unescaped markup character). The refusal happens before
anything is written. A `.typ` and an `.md` cannot share a stem in one folder.

### Adopted

A deck or chapter that already exists, with its figures and bibliography, is adopted rather than
retyped. Add `--attach` to `--format typst` (MCP: `attach` with `format: "typst"`, `content` empty):

```bash
cdno file --portfolio surrogate-error-budget --source "Review deck" \
          --origin projects/surrogate-model --format typst --attach ~/talks/review.typ
# Filed at portfolios/surrogate-error-budget/2026-11-09-review-deck.typ
#   rewrote figs/err.png -> 2026-11-09-review-deck/figs/err.png
#   rewrote refs.bib -> 2026-11-09-review-deck/refs.bib
```

The file becomes the note, with the metadata block prepended. The **folder** that comes with it is
the directory the file sits in: every file there, subfolders included, is copied to the note's
assets folder `<stem>/`, and each literal relative path the file names (`image`, `read`,
`bibliography`, `include`, a path `import`, and the other file-reading calls) is rewritten to point
at the copy. The rewrites are listed in the result (`rewrites`, each `was` and `now`; with `--json`
an array of `{was, now}`).

Give the file a directory of its own. Adoption is refused, with the code `adoption_refused` and a
`remedy`, when:

- the directory holds more than 200 files or more than 200 MiB;
- it holds a symbolic link, or anything that is neither a regular file nor a folder;
- the directory overlaps the vault (it contains the vault, or is inside it).

Names starting with `.` are skipped, so a deck that names such a file is refused as not being in the
folder. A path that climbs out of the folder, a `/`-rooted path or a non-literal path argument is
refused (`asset_path_outside_folder`, `asset_path_not_literal`). A `@preview` package import is left
alone. A file that does not parse, or already carries `<cdno>`, is refused. `--content` and
`--body-file` are refused with adoption.

Without `--format typst`, `--attach deck.typ` still files an attachment with a stub, as it always
has. Use that for something you want stored, not searched and linked.

## Reading and revising

`read_note` returns `format` (`markdown` or `typst`), the source after the metadata block as `body`,
the dictionary as `frontmatter`, and the `=` headings as `headings`. `cdno open` opens the `.typ`
file itself.

- **Evidence is append-only in both formats.** `revise_note` refuses it. A later correction is a
  dated heading added at the end of the file (`= Revision 2026-12-01`) by hand, as for Markdown.
- **A concept is revised as a whole body**: read it, edit the body, send all of it with
  `expected_hash`. `cdno note revise --body-file` does the same. The blank lines after the metadata
  block are kept, so a body that begins with blank lines has them dropped; sending back exactly what
  `read_note` returned changes nothing. A section cannot be revised in a Typst note
  (`section_revision_unavailable_for_typst`); every equation is re-emitted with the whole body, so
  check that only the intended region changed.
- **Renames.** A Typst concept can be renamed: `cdno note rename` moves `<old>.typ` and `<old>/`
  together and rewrites the links to it. Evidence cannot be renamed, in either format.

## What you lose

- **Obsidian does not follow links to a Typst note.** It resolves `[[stem]]` only to `stem.md`, so
  every link to a `.typ` note is dead there, including the `evidence created` and `revised` lines
  cuaderno writes to the journal and every backlink. Clicking one makes an empty `stem.md` beside the
  `.typ` (lint reports the clash). Read Typst notes through `cdno open`, an editor, or the compiled
  PDF. This is why only evidence and concepts may be Typst.
- **Markdown-only tools do not apply**: `cdno lint --fix` repairs, section revision, inline tags,
  template variables and the concept scaffold.
- A **search hit** inside a Typst note shows Typst syntax in its snippet.
- **Adoption scans only the adopted file.** A sibling `.typ` copied with the folder is not scanned,
  so a bad path inside it fails when the chapter is compiled. A listed function used as a value
  (`("a.png",).map(image)`) is not seen, so its file is neither copied nor rewritten.
- **A rename rewrites only the note's own literal paths into its assets folder.** Another note's
  `#image("…/<old>/…")` into the moved folder, a non-literal or `/`-rooted path, and a path in a
  `.typ` file inside the assets folder are left as written and break on the move, without a warning.
- A Typst evidence body is **frozen as written**. A wrong package name or version has no remedy
  until rendering exists.

## A project's deliverable is not evidence

A Typst report, deck or paper that you delivered belongs where the project's work lives, the forge
or the repository, linked from the map's `## Links`. It is not filed to a portfolio, in either
format. Evidence is what the work produced and observed along the way. Filing asks one question
first: *is this the thing you delivered?*

## See also

- [`cdno file`](cli/file.md), [`cdno note`](cli/note.md), [`cdno lint`](cli/lint.md#typst-notes).
- [Research and evidence](../tutorials/research-and-evidence.md#filing-a-typst-note) and
  [Building a concept library](../tutorials/concept-library.md).
- [`[typst]` configuration](configuration.md#typst).
