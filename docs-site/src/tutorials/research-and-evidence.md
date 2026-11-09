# Research and evidence

This is the knowledge-building half of the method: name the **questions** that matter, open a
**portfolio** for each, and **file evidence** into it as you go. Over months a portfolio becomes a
dossier you can actually reason from.

## Name a question

```bash
cdno question create --domain research --text "Does sparse attention beat dense out-of-distribution?"
# -> questions/research/does-sparse-attention-beat-dense-out-of-distribution.md
```

`--domain` is `research` or `life`. List the active ones any time:

```bash
cdno questions                  # grouped by domain
cdno questions --json | jq .
```

As a question's life changes, transition it (each transition is logged to the journal):

```bash
cdno question park   --slug does-sparse-attention-beat-dense-out-of-distribution
cdno question answer --slug does-sparse-attention-beat-dense-out-of-distribution
cdno question retire --slug does-sparse-attention-beat-dense-out-of-distribution
cdno question activate --slug does-sparse-attention-beat-dense-out-of-distribution
```

## Open a portfolio for it

A portfolio is a folder that accumulates evidence about one question:

```bash
cdno portfolio create --question "Sparse vs dense attention OOD"
# -> portfolios/sparse-vs-dense-attention-ood/_index.md

# Optionally tie it to a project when you create it:
cdno portfolio create --question "Sparse vs dense attention OOD" --project projects/surrogate-model
```

Already have a portfolio and want to link it after the fact? Use the retrofit verb (one of
`--question`/`--project`, not both):

```bash
cdno portfolio link --portfolio sparse-vs-dense-attention-ood --project projects/surrogate-model
```

## File evidence

Every useful artefact — a paper, an experiment result, a conversation — goes in as an `evidence`
note. `--source` is the citation/reference; `--origin` is a wikilink to whatever produced it:

```bash
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Chen et al. 2025, NeurIPS" \
          --origin projects/surrogate-model \
          --content "They report a 4x speedup at 95% accuracy on the OOD split."
```

### Attaching a real file

To file a non-Markdown artefact (PDF, image, video), point `--attach` at it. Cuaderno copies it into
the portfolio and scaffolds a linked evidence stub beside it (here `--content` is the abstract). Add
`--move` to move instead of copy:

```bash
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Chen et al. 2025" \
          --origin projects/surrogate-model \
          --attach ~/Downloads/chen2025.pdf \
          --content "Key result: 4x speedup at 95% accuracy."
```

### Filing a Typst note

If the evidence is already Typst, or you want it written as Typst (a derivation with equations, say),
file it as a Typst note. First the test every filing passes: **is this the thing you delivered?** A
deliverable belongs to its project and is not filed as evidence. Evidence is an observation or result
that bears on a question.

```bash
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Error budget derivation" \
          --origin projects/surrogate-model \
          --format typst --body-file derivation.typ
```

The body is complete Typst, preamble and imports included; cuaderno writes the metadata block itself,
so leave out any `<cdno>` block. When you have no preamble of your own, copy the one from the newest
Typst note in the portfolio. Over MCP the same filing is `file_to_portfolio` with `format: typst`;
an agent reads that sibling with `read_note`.

Three things to know:

- **Cuaderno checks syntax only.** A body that parses can still come out wrong when you compile it.
- **Escape inside `[[…]]`.** Put a backslash before any Typst markup character in a link
  (`[[surrogate-error-budget\#bias]]`). An unescaped character is refused, with the fix named.
- **Leave a space before `[[`** when it follows a `#…` expression or an `@ref`. Without one the link
  is lost silently, and nothing refuses it.

`--var` is refused with `--format typst`. Do not use `--attach` without `--format typst` for a `.typ`
that should be a note: attaching stores the bytes behind an abstract, while a Typst note is
searchable as text.

#### Adopting a file you already have

A deck or report already written, with its figures beside it, is adopted rather than retyped. Add
`--attach` to `--format typst` and the file itself becomes the note:

```bash
cdno file --portfolio surrogate-error-budget \
          --source "Review deck, 2026-11-08" \
          --origin "[[projects/surrogate-model]]" \
          --format typst --attach ~/talks/review.typ
# -> portfolios/surrogate-error-budget/2026-11-08-review-deck.typ
#    portfolios/surrogate-error-budget/2026-11-08-review-deck/figs/…   (copied, paths rewritten)
```

The folder copied is the directory the `.typ` sits in, so the deck needs a folder of its own, holding
only its figures and bibliography. Subfolders are kept; entries whose names start with a dot are
skipped; the limits are 200 files and 200 MiB. A symbolic link, or a folder that overlaps the vault,
is refused (code `adoption_refused`). Every literal relative path in the file (an `image`, a
`bibliography`, a `read`) is rewritten to point at the copy, and the result lists each as a `rewrites`
pair of `was` and `now`: a record, with nothing for you to do. A path that climbs out of the folder,
or a non-literal path, is refused with the fix named. Without `--format`, `--attach deck.typ` still
files an attachment with a stub.

## Review what's accumulated

```bash
cdno portfolio list                              # all portfolios + evidence counts + staleness
cdno portfolio show --portfolio sparse-vs-dense-attention-ood
cdno search "preconditioner" --portfolio sparse-vs-dense-attention-ood
```

`portfolio list` flags **staleness** — dossiers you haven't fed in a while — which is a useful prompt
during your monthly scan. Periodically synthesise the findings into the portfolio's `_index.md`.

Not everything worth keeping is evidence. What a result taught you about a technique — understanding
you will reuse, independent of any question — is a **concept**, and it lives in its own library;
see [The concept library](../concepts/concept-library.md) for the filing test.

Next: [Building a concept library](concept-library.md).
