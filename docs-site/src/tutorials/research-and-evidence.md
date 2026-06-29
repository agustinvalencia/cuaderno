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

## Review what's accumulated

```bash
cdno portfolio list                              # all portfolios + evidence counts + staleness
cdno portfolio show --portfolio sparse-vs-dense-attention-ood
cdno search "preconditioner" --portfolio sparse-vs-dense-attention-ood
```

`portfolio list` flags **staleness** — dossiers you haven't fed in a while — which is a useful prompt
during your monthly scan. Periodically synthesise the findings into the portfolio's `_index.md`.

Next: [Actions](actions.md).
