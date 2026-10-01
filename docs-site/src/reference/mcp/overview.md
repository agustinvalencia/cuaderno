# MCP server reference

`cdno-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server that exposes your
vault to AI clients (Claude Desktop, Claude Code, Kiro, Gemini CLI, …). It runs the same domain
engine as the CLI, so anything the assistant does goes through the same rules and lands in the same
Markdown files. To wire it up, see [Connect to Claude](../../getting-started/connect-to-claude.md).

## Transport and vault selection

Two binaries serve the same tool catalogue:

- **`cdno-mcp`** speaks JSON-RPC over **stdio** — the client launches it as a subprocess. It opens
  the vault named by `CUADERNO_VAULT_PATH`, or, if that's unset, discovers one from its working
  directory (the same rule as the CLI).
- **`cdno-mcp-server`** speaks MCP **Streamable HTTP** for remote clients — see
  [The HTTP server](http-server.md) for its flags and security model.

## The tool surface

The server advertises **60 tools**. This reference groups them by purpose:

| Group | Page | What's in it |
|-------|------|--------------|
| Context-gathering reads | [Context-gathering tools](reads.md) | Orientation, project/portfolio/weekly context, search, reads, lint, triage list |
| Writes | [Write tools](writes.md) | Log, capture, file evidence, project/action/milestone/waiting edits, note revision, commitments, tracking, daily/weekly sections |
| Creation & lifecycle | [Creation and lifecycle tools](creation-and-lifecycle.md) | Create projects/portfolios/questions/stewardships, link portfolios, park/activate/complete/drop, status transitions |

Every tool returns typed JSON; the shapes mirror the CLI's [`--json`](../json-output.md) output, so a
client gets the same structures whichever surface it uses.

## Conventions

- **Slugs, not paths.** Tools take slugs (`surrogate-model`), matching the CLI.
- **Substring matching** for completing actions/milestones and resolving waiting-on items, exactly as
  on the CLI.
- **The same rules apply.** The five-project cap, append-only notes, auto-logged project-state
  history, and commitments aggregation all hold — the MCP server is not a back door around them.

## When a call is rejected

Cuaderno splits failures along the line MCP itself draws, and the distinction is worth knowing
because the two arrive in different places in the response.

**A rejection you can act on comes back as a tool result** with `isError: true` — not as a JSON-RPC
error. The response carries no `error` member at all, and the payload is the single text content
item, JSON-encoded exactly as a successful result's payload is:

```json
{ "isError": true, "content": [{ "type": "text", "text": "…" }] }
```

Decoding that `text` gives the rejection itself. This is real output from `complete_action` with a
query matching two bullets:

```json
{
  "code": "ambiguous_action",
  "details": {
    "candidates": [
      "Draft the methods section (deep)",
      "Revise the methods appendix (light)"
    ],
    "query": "methods",
    "slug": "ablation-study"
  },
  "message": "ambiguous action match for 'methods' on project 'ablation-study': [\"Draft the methods section (deep)\", \"Revise the methods appendix (light)\"]"
}
```

Three fields, and each has a job:

| Field | For |
|---|---|
| `code` | A stable snake_case discriminant to branch on — `ambiguous_action`, `state_too_long`, `project_cap_reached`, `field_not_settable`, … |
| `message` | The human-readable sentence, for showing a person |
| `details` | The structured fields needed to recover — `candidates` to pick from, `chars`/`max` to condense against, `active_projects` to park one of |

`details` is the part that matters for automation. An ambiguous query hands back its **candidates as
an array**, so a client picks from a list rather than parsing them out of a sentence.

Two project codes carry details a client is likely to branch on:

| Code | `details` | When |
|---|---|---|
| `project_not_active` | `slug`, `status` (`parked`, `completed` or `dropped`), and `closed` when the map carries a date | Any verb that edits a project's body (`add_action`, `update_project_state`, `drop_milestone`, …) on a parked or closed project, and a close repeating the outcome a project already has. A `parked` status means `activate_project` first; a closed one means the project is finished, unless the user wants it back. |
| `ambiguous_project` | `slug`, `candidates: [{path, note_type}]` | The slug names maps in more than one of `projects/`, `projects/_parked/` and `projects/_done/<year>/`. Nothing can pick for you: keep one of the files by hand. |

The test is simply **whether a different argument would change the outcome**. A slug that matches no
note is a rejection (`not_found`), and its message carries the list of slugs that *do* exist. A note
missing the section a tool writes into is a rejection. So is a name that is already taken.

**A mechanical failure stays a JSON-RPC error** (`-32603`): a disk that would not write, permission
denied, a contended write lock, an index that would not answer, a transaction that rolled back.
Nothing the caller passed can fix those, so there is nothing to branch on. Malformed calls — an
unknown tool, unparseable arguments — are protocol errors too (`-32602`), per the spec.

The practical upshot for a client: **read `isError` on the result, not just the presence of `error`**.
A rejection is a normal, expected answer to a call that was itself well-formed.

## Building skills on top

Multi-step rituals (a morning orientation, a guided weekly review) are best wrapped as **Claude
skills** that call these tools in sequence. See [Using with Claude skills](with-claude-skills.md).
