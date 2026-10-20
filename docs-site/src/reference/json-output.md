# JSON output

Adding `--json` to a supported verb swaps the formatted table for machine-readable JSON. These shapes
match the [MCP server](mcp/overview.md) DTOs, with the exceptions noted below, so you get the same structures from the CLI and from an
AI client. (Which verbs support `--json` is covered in the [CLI overview](cli/overview.md#json-output).)

## Write verbs → a result object

Every write verb emits the same small object and runs non-interactively (`project complete` and
`project drop` add to it; see [below](#project-complete--drop---json--a-result-or-a-refusal)):

```json
{
  "path": "projects/surrogate-model.md",
  "message": "Created projects/surrogate-model.md"
}
```

`path` is the vault-relative file written or updated; `message` is the human-readable line. For the
two-file cases (e.g. `action add --note`), `path` is the file the verb considers primary.

## `project complete` / `drop --json` → a result or a refusal

A close that goes ahead emits the result object with three more arrays: what the drop let go, and
the linked standalone commitments, which no close ever touches.

```json
{
  "path": "projects/_done/2026/alpha-study.md",
  "message": "Dropped alpha-study. 0 of 5 slots in use. Let go: 1 action, 1 milestone.",
  "dropped_actions": ["Define first concrete step (light)"],
  "dropped_milestones": ["Submit"],
  "untouched_commitments": []
}
```

A close refused because actions or milestones are open prints the rejection object the MCP server
returns, on stdout, and exits `1`:

```json
{
  "code": "project_has_open_items",
  "message": "project 'alpha-study' has 2 open item(s) — tick the ones that are done, or let them go explicitly",
  "details": {
    "slug": "alpha-study",
    "actions": [{ "text": "Define first concrete step (light)", "note": null, "note_status": null }],
    "milestones": [{ "title": "Submit", "date": "2026-12-01", "hard": false }],
    "untouched_commitments": [],
    "open_items_hash": "76d0efb83a540bf9"
  }
}
```

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

## `open --json` → the resolved path

```bash
cdno open today --json
```
```json
{ "path": "/home/you/vault/journal/2026/daily/2026-08-21.md" }
```

Absolute, so the value composes from any directory. A reference that resolves to nothing — or to
more than one note — is an error, not an empty result.

## `open --list --json` → an array of candidates

Every note in the vault, most-recently-modified first. The plain-text form of the same listing is
tab-separated for piping into a fuzzy finder; `--json` is for programmatic consumers.

```json
[
  {
    "path": "projects/surrogate-model.md",
    "title": "Surrogate model",
    "type": "project"
  }
]
```

`title` is the note's body H1, and is `null` for a note that has none.

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

- **`project list --closed`** → `[{ "slug", "title", "context", "outcome", "closed_on" }]`, most
  recently closed first; `outcome` is the closure, `completed` or `dropped`, and never a parent. This row is the CLI's own: MCP
  `list_projects` with `include_closed` returns closed maps as `{ "slug", "frontmatter" }`.
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

## `orient --json` → the orientation object

```bash
cdno orient --json
```
```json
{
  "commitments": [],
  "projects": [
    {
      "slug": "surrogate-model",
      "status": "active",
      "context": "work",
      "core_question": "[[questions/research/surrogate-generalises]]",
      "state_snippet": "Mesh scaling works; assembly is the bottleneck",
      "top_action": { "text": "Profile the assembly step", "energy": "medium" },
      "held_by": null,
      "counts": true,
      "waiting_on_empty": true
    },
    {
      "slug": "mesh-study",
      "status": "active",
      "context": "work",
      "core_question": "[[surrogate-generalises]]",
      "state_snippet": "Coarse-mesh run validated end to end",
      "top_action": { "text": "Write up the convergence plot", "energy": "light" },
      "held_by": "the modelling group",
      "counts": false,
      "waiting_on_empty": false
    }
  ],
  "groups": [
    {
      "parent": { "kind": "question", "path": "questions/research/surrogate-generalises.md" },
      "projects": ["surrogate-model", "mesh-study"]
    }
  ],
  "slots": { "active": 2, "counted": 1, "cap": 5, "counted_slugs": ["surrogate-model"] },
  "lapsed_habits": []
}
```

`projects` holds one project-list element per active project; each carries `core_question`, the
map's frontmatter value exactly as written, or `null` when it sets none (`project list` and
`project show` elements carry it too). Each also carries three keys about the cap (`project list`
and `project show` elements too): `held_by` is the map's `held_by:` text as written, or `null` when
it has none or the value is not a plain string; `counts` is `true` when the map holds one of the
`max_active_projects` slots, that is, it is active and does not name someone else as its holder (a
`held_by` of `me`, `tbd` or another entry of
[`holder_sentinels`](configuration.md#keys) leaves it counted, so branch on `counts`, never on the
name); and `waiting_on_empty` is `true` when the map's `## Waiting On` is missing, empty, or only
`(nothing yet)`. `groups` arranges the same projects, by slug, under the
core question they share: a short and a full link to one question group together, since the key is
the question's vault path (or the link's raw target when it resolves to no note), and only the
field counts, never a passing mention of the question in a map's body. Every project falls in
exactly one group, groups follow the order of their first member, and a project with no question
is a group of its own with `"parent": null`. MCP `get_orientation` carries `groups` in the same
shape. `slots` is the active projects
against the cap: `active` counts the maps with `status: active`, `counted` those holding a slot
(the active maps whose `counts` is `true`), which is the number the cap is checked against, `cap` is the configured [`max_active_projects`](configuration.md#keys), and
`counted_slugs` names the counted maps. MCP `get_orientation` carries `slots` as
`{ "active", "cap" }`, the object `list_projects` returns.

## Casing

Enumerations serialise in their canonical lowercase/kebab form, matching the MCP DTOs:
`status` → `active`/`parked`/`completed`/`dropped`, `energy` → `deep`/`medium`/`light`, stewardship `variant` →
`flat`/`expanded`, `context` → `work`/`side-project`/`household`/… So a value is identical whether you
read it from `cdno --json` or over MCP.
