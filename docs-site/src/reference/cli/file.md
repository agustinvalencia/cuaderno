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
| `--var <NAME=VALUE>` | Value for a custom evidence template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. Ignored with `--attach` (attachment stubs aren't templated). See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |
| `--format <markdown\|typst>` | The file format, default `markdown` (never prompted). `typst` writes `<portfolio>/<date>-<source>.typ` from `--body-file` (or `--content`): a metadata block holding `type`, `created`, `source`, `portfolio` and `origin`, a blank line, then the body as Typst markup. The body is required, and prompted for in an editor when interactive. `--var` is refused with it, and so is a body that carries a `<cdno>` label, does not parse, or has a `[[` link that is not one inline run; with `--json` the refusal is a `{code, message, details}` object whose `details.remedy` says what to do. Not yet combinable with `--attach`. |
| `--body-file <PATH>` | The body of a Typst note, read from a file. Only with `--format typst`; conflicts with `--content`. A blank body is refused. |

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

A Typst evidence note, written from a file:

```bash
cdno file --portfolio sparse-vs-dense-attention-ood \
          --source "Error budget derivation" --origin projects/surrogate-model \
          --format typst --body-file derivation.typ
# Filed at portfolios/sparse-vs-dense-attention-ood/2026-11-09-error-budget-derivation.typ
```

Evidence notes are [append-only](../../concepts/business-rules.md).

## Related MCP tool

[`file_to_portfolio`](../mcp/writes.md).

## See also

- [Research and evidence](../../tutorials/research-and-evidence.md).
- [`portfolio`](portfolio.md) — create and inspect portfolios.
