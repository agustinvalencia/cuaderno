# Vault Linking Rules

When writing content into vault notes (daily-log lines, standup / intention / agenda sections, any narrative written to a note), **use wikilinks** for references to other notes. A wikilink keeps the vault's graph connected so backlinks, project mentions, and orientation queries can find the reference later.

## Link targets by note type

Cuaderno slugs are unique across the vault, so the **bare form** `[[<slug>]]` resolves in most cases. Use the **qualified form** (folder-prefixed) where the design expects it — notably `core_question`.

| Note type | On disk | Wikilink |
|-----------|---------|----------|
| Project map | `projects/<slug>.md` (or `projects/_parked/<slug>.md`) | `[[<slug>]]` or `[[projects/<slug>]]` |
| Action note | `actions/<slug>.md` | `[[<slug>]]` |
| Question | `questions/<domain>/<slug>.md` (`domain` = `research` \| `life`) | `[[questions/<domain>/<slug>]]` |
| Portfolio | `portfolios/<slug>/_index.md` | `[[portfolios/<slug>]]` or `[[<slug>]]` |
| Evidence | `portfolios/<portfolio>/<note>.md` | `[[<slug>]]` |
| Stewardship | `stewardships/<slug>.md` (or `stewardships/<slug>/_index.md`) | `[[<slug>]]` |
| Commitment | `commitments/<slug>.md` | `[[<slug>]]` |
| Daily note | `journal/<year>/daily/YYYY-MM-DD.md` | `[[YYYY-MM-DD]]` |
| Weekly note | `journal/<iso-year>/weekly/YYYY-WNN.md` | `[[YYYY-WNN]]` |

## Bare vs qualified form

Use the **bare form** when the slug is unambiguous and readable — this is the default:

```
Started [[project-alpha]] today.
Closed [[draft-the-spec]] — first pass done.
```

Use the **qualified form** when the design requires the folder context, or to disambiguate:

```
core_question: [[questions/research/key-open-question]]
Filed evidence into [[portfolios/reference-material]].
```

Never invent an alias-pipe (`[[slug|Display]]`) unless the slug is genuinely unreadable — cuaderno slugs are already human-readable, so the bare link reads cleanly on its own.

## Where this applies

- `append_to_log(text)` — daily-log lines (e.g. "Started the day — focus [[project-alpha]]: run the first pass").
- `upsert_daily_section(section, content)` — the Standup / Intention / Agenda you write into the daily note.
- Any narrative content a skill writes into a project map, portfolio, or other note.

## How to get the correct slug

Look the slug up from an MCP response before writing the link — don't hand-fabricate it:

- `get_orientation` → `projects[].slug` (active project slugs) and each project's `top_action`.
- `get_active_questions` → `slug` + `domain` for each question (build `[[questions/<domain>/<slug>]]`).
- `get_project_context` → the project slug plus `backlinks` grouped by note type (action / portfolio / question / evidence paths).
- `get_portfolio_contents` → evidence note paths within a portfolio.
- Write-op tools (`add_action`, `file_to_portfolio`, …) return the new note's `path` — derive the slug from its filename stem.

If you don't have the exact slug, a bare best-effort `[[<slug>]]` still beats plain text — it renders as an unresolved link rather than losing the reference entirely. Resolving broken links is lint's job, not the writing skill's.
