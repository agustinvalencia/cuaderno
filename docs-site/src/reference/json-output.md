# JSON output

Adding `--json` to a supported verb swaps the formatted table for machine-readable JSON. These shapes
match the [MCP server](mcp/overview.md) DTOs, so you get the same structures from the CLI and from an
AI client. (Which verbs support `--json` is covered in the [CLI overview](cli/overview.md#json-output).)

## Write verbs → a result object

Every write verb emits the same small object and runs non-interactively:

```json
{
  "path": "projects/surrogate-model.md",
  "message": "Created projects/surrogate-model.md"
}
```

`path` is the vault-relative file written or updated; `message` is the human-readable line. For the
two-file cases (e.g. `action add --note`), `path` is the file the verb considers primary.

## `search --json` → an array of hits

Ranked best-first; each hit:

```json
[
  {
    "path": "portfolios/sparse-vs-dense-attention-ood/2026-03-15-chen-2025.md",
    "note_type": "evidence",
    "title": "Chen et al. 2025",
    "snippet": "...4x speedup at 95% accuracy on the OOD split...",
    "score": 1.7
  }
]
```

A lower `score` ranks earlier (best match first). No matches → `[]`.

## `list` verbs → arrays of summaries

```bash
cdno project list --json
```
```json
[
  {
    "slug": "surrogate-model",
    "status": "active",
    "state_snippet": "Mesh scaling works; assembly is the bottleneck",
    "top_action": { "text": "Profile the assembly step", "energy": "medium" }
  }
]
```

- **`portfolio list`** → `[{ "slug", "question", "evidence_count", "last_updated", "staleness_days" }]`
- **`stewardship list`** → `[{ "slug", "name", "context", "variant", "tracking_count" }]`
- **`action list`** → `[{ "text", "energy", "attached": { "slug", "status" } | null }]`

## `show` verbs → a detail object

```bash
cdno project show surrogate-model --json   # same shape as a project-list element
cdno portfolio show --portfolio sparse-vs-dense-attention-ood --json
```
```json
{
  "slug": "sparse-vs-dense-attention-ood",
  "question": "Sparse vs dense attention OOD",
  "created": "2026-03-01",
  "project": "[[projects/surrogate-model]]",
  "evidence": [
    {
      "path": "portfolios/sparse-vs-dense-attention-ood/2026-03-15-chen-2025.md",
      "created": "2026-03-15",
      "source": "Chen et al. 2025",
      "origin": "[[projects/surrogate-model]]"
    }
  ]
}
```

- `project` is `null` when the portfolio isn't linked to one.
- An evidence entry gains a `"kind"` field (e.g. `"pdf"`) only for attachment stubs; it's omitted for
  plain prose notes.
- **`stewardship show`** → `{ "slug", "name", "context", "variant", "body_markdown" }`.

## Casing

Enumerations serialise in their canonical lowercase/kebab form, matching the MCP DTOs:
`status` → `active`/`parked`/`completed`, `energy` → `deep`/`medium`/`light`, stewardship `variant` →
`flat`/`expanded`, `context` → `work`/`side-project`/`household`/… So a value is identical whether you
read it from `cdno --json` or over MCP.
