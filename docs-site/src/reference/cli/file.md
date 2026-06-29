# `cdno file`

File a piece of evidence into a portfolio. Without `--attach` it writes a plain Markdown evidence
note; with `--attach` it copies a non-Markdown artefact (PDF, image, video) into the portfolio and
scaffolds a linked evidence stub beside it.

```text
cdno file [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--portfolio <PORTFOLIO>` | Portfolio slug. (Prompted/fuzzy-picked if omitted interactively.) |
| `--source <SOURCE>` | Citation, experiment id, conversation reference, … |
| `--origin <ORIGIN>` | Bare wikilink target to whatever produced this evidence (e.g. `projects/foo`); the CLI wraps it into `[[...]]`. |
| `--content <CONTENT>` | Inline body. For a plain note it's the content; with `--attach` it's the abstract. Optional; defaults to empty. |
| `--attach <ATTACH>` | Path to a non-Markdown artefact. Copied into `portfolios/<slug>/<evidence-slug>/` with a stub that links to it. |
| `--move` | With `--attach`, remove the source file after a successful copy (move instead of copy). |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result and runs non-interactively.

## Examples

```bash
# A plain prose evidence note:
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Chen et al. 2025, NeurIPS" \
          --origin projects/surrogate-model \
          --content "4x speedup at 95% accuracy on the OOD split."

# Attach a PDF (copied into the portfolio; --content is the abstract):
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Chen et al. 2025" --origin projects/surrogate-model \
          --attach ~/Downloads/chen2025.pdf --content "Key result: 4x speedup."

# Move the artefact in instead of copying:
cdno file --portfolio sparse-vs-dense-attention-ood --source "fig 3" \
          --origin projects/surrogate-model --attach ./fig3.png --move
```

Evidence notes are [append-only](../../concepts/business-rules.md).

## Related MCP tool

[`file_to_portfolio`](../mcp/writes.md).

## See also

- [Research and evidence](../../tutorials/research-and-evidence.md).
- [`portfolio`](portfolio.md) — create and inspect portfolios.
