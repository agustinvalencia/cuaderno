---
schema_version: 1
applies_to_crate: cdno-core
last_updated: 2026-04-19
---

# Cuaderno Index Schema

The SQLite index is a **cache**, not the source of truth. The markdown
vault on disk is authoritative; the index exists only to answer read
queries quickly (list active projects, aggregate commitments, find
backlinks, …). If the index is lost, corrupted, or behind, startup
reconciliation rebuilds it from the filesystem — a stale index is
recoverable, a stale file is data loss.

This document describes **schema version 1**. The corresponding
executable migration is `migrations/001_initial.sql`.

## Conventions

- **Paths** are vault-relative, forward-slash separated. `VaultPath` guarantees
  no absolute paths, no `..` components, consistent across macOS and Linux.
- **Timestamps** use nanoseconds since the UNIX epoch (`SystemTime::duration_since(UNIX_EPOCH).as_nanos()`), stored as `INTEGER`.
- **Dates** (due dates, not timestamps) use ISO-8601 `YYYY-MM-DD` text.
- **Booleans** are `INTEGER` with values `0` or `1` (SQLite has no native bool).
- **`ON DELETE CASCADE`** is used on every foreign key so that removing a note
  automatically cleans up its facets. Requires `PRAGMA foreign_keys = ON` at
  connection time (see the connection setup task, #11).
- **WAL mode** is enabled at connection init (`PRAGMA journal_mode = WAL`)
  to allow readers and writers to proceed concurrently — important for
  the MCP + Tauri access pattern.

## Tables

### `notes`

One row per markdown file in the vault. The canonical place to answer
"does this note exist?", "what type is it?", "when did it last change?".

```sql
CREATE TABLE notes (
    path          TEXT PRIMARY KEY,        -- vault-relative path
    note_type     TEXT NOT NULL,           -- kebab-case NoteType variant
    title         TEXT,                    -- frontmatter.title, or first heading
    content_hash  TEXT NOT NULL,           -- xxhash3 of the raw file
    mtime_ns      INTEGER NOT NULL,        -- file mtime (nanoseconds since epoch)
    size          INTEGER NOT NULL,        -- file size in bytes
    frontmatter   TEXT NOT NULL,           -- full frontmatter as JSON
    indexed_at_ns INTEGER NOT NULL         -- when this row was last written
) WITHOUT ROWID;

CREATE INDEX idx_notes_type ON notes(note_type);
```

- `WITHOUT ROWID` — we key by path and never want a surrogate rowid.
  Saves space and gives clustered storage by path, which is how we
  most often access notes.
- `frontmatter` is a JSON blob rather than typed columns. SQLite's
  `json_extract()` handles ad-hoc queries on hot fields (`status`,
  `context`), and we avoid migrations every time a note type adds a
  field. If a specific query becomes measurably slow, we can add a
  generated column without disturbing callers.

### `deadlines`

Materialised flat view of every deadline in the vault, aggregated at
index time from the three sources the commitments query needs to merge.
This is what keeps `cdno orient` fast — a single `SELECT` instead of
re-parsing every project and stewardship on every call.

```sql
CREATE TABLE deadlines (
    id         INTEGER PRIMARY KEY,
    note_path  TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    source     TEXT NOT NULL,              -- 'project_milestone' | 'stewardship_periodic' | 'commitment_note'
    title      TEXT NOT NULL,
    due_date   TEXT NOT NULL,              -- ISO-8601 YYYY-MM-DD
    is_hard    INTEGER NOT NULL,           -- 0 or 1
    context    TEXT                        -- 'work' | 'personal' | 'home-family' | NULL
);

CREATE INDEX idx_deadlines_due  ON deadlines(due_date);
CREATE INDEX idx_deadlines_note ON deadlines(note_path);
```

- The **three sources** mirror the design doc's commitments aggregation:
  project milestones with `hard:` deadlines, stewardship periodic
  commitments (e.g. "tax return every 12 months"), and standalone
  `commitment` notes.
- `idx_deadlines_due` is the workhorse: every orient / commitments-view
  query filters by date range.
- `idx_deadlines_note` supports `ON DELETE CASCADE` and the "clear old
  deadlines, re-insert fresh ones" pattern used when a project's
  milestones change.

### `links`

The wikilink graph. Supports both outgoing (from a note) and incoming
(backlinks) queries through two indexes on the same table.

```sql
CREATE TABLE links (
    id            INTEGER PRIMARY KEY,
    source_path   TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    target_raw    TEXT NOT NULL,           -- as authored, e.g. 'icml-paper' or '2026-04-19'
    resolved_path TEXT,                    -- resolved at index time, NULL if broken
    label         TEXT                     -- display text after '|', if present
);

CREATE INDEX idx_links_source   ON links(source_path);
CREATE INDEX idx_links_resolved ON links(resolved_path);
```

- **Link-time resolution**: `resolved_path` is populated when the link
  is indexed, not computed per query. When a note moves, we `UPDATE`
  both `notes.path` and any `links.resolved_path` pointing at the old
  location. Broken links surface immediately in lint rather than at
  query time.
- Two separate indexes give us O(log n) lookup in either direction:
  `find_outgoing_links(path)` uses `source_path`, `find_backlinks(path)`
  uses `resolved_path`.

### `note_tags`

Tag-to-note join table for filtering and Zettelkasten-style co-occurrence
queries ("notes that share ≥2 tags with X", "latest interactions with
#collaborator-name").

```sql
CREATE TABLE note_tags (
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    tag       TEXT NOT NULL,
    PRIMARY KEY (note_path, tag)
) WITHOUT ROWID;

CREATE INDEX idx_tags_tag ON note_tags(tag);
```

- **Extraction policy** (implemented in the indexer, #13):
  1. **Frontmatter `tags:` list** — always indexed.
  2. **Inline `#tag-name` in body** — indexed, but only when emitted by
     `pulldown-cmark` inside a `Text` event. That automatically excludes
     code blocks, code spans, HTML, and headings (a `# Introduction`
     line produces `Start(Heading)` + `Text("Introduction")`, never a
     raw `#` text token).
- **Tag pattern**: `#[a-zA-Z0-9][a-zA-Z0-9_-]*`. Starts with an
  alphanumeric; allows hyphens and underscores. Matches
  `#agustin-valencia`, `#deep-work`, `#quarterly_review`; rejects bare
  `#-` and `#_`.

### `schema_migrations`

Tracks which migration versions have been applied to the current DB.
On every connection open, the engine compares `MAX(version)` against
the migrations directory and applies any that are missing.

```sql
CREATE TABLE schema_migrations (
    version        INTEGER PRIMARY KEY,
    applied_at_ns  INTEGER NOT NULL,
    description    TEXT
);
```

## Query patterns

| Query | Plan |
|---|---|
| `find_by_path(p)` | `SELECT * FROM notes WHERE path = ?` — PK lookup. |
| `list_by_type(t)` | `SELECT * FROM notes WHERE note_type = ?` — uses `idx_notes_type`. |
| `active_projects()` | `SELECT * FROM notes WHERE note_type = 'project' AND json_extract(frontmatter, '$.status') = 'active'` — type index + JSON extract. |
| `commitments(lookahead)` | `SELECT * FROM deadlines WHERE due_date BETWEEN ? AND ? ORDER BY due_date` — `idx_deadlines_due` range scan. |
| `find_backlinks(p)` | `SELECT source_path FROM links WHERE resolved_path = ?` — `idx_links_resolved`. |
| `find_outgoing_links(p)` | `SELECT resolved_path, target_raw, label FROM links WHERE source_path = ?` — `idx_links_source`. |
| Notes tagged `#X` by recency | `SELECT n.path FROM note_tags t JOIN notes n ON n.path = t.note_path WHERE t.tag = ? ORDER BY n.mtime_ns DESC` — `idx_tags_tag` + PK. |

## Reconciliation semantics

Startup reconciliation (#16) treats the filesystem as truth:

1. `walk_dir` the vault for every `.md` file.
2. For each file: compare `mtime_ns + content_hash` with the `notes` row.
   - Missing row → insert, also insert deadlines, links, tags.
   - Matching row → skip.
   - Changed row → update `notes`, delete and re-insert dependent rows
     (deadlines, links, tags) via cascading writes.
3. For every `notes.path` not visited in the walk → delete. Cascades
   clean up dependents.

Because the index is rebuildable, *any* inconsistency is recoverable
— at worst we drop and rerun migrations against an empty DB.

## Migration strategy

- **Up-only migrations.** Every schema change is a new file:
  `002_something.sql`, `003_…`, etc. Never edit `001_initial.sql`
  after it ships — that would leave existing DBs in an undefined state.
- **Each migration ends with** `INSERT INTO schema_migrations` recording
  the new version. That row is what lets the engine skip already-applied
  migrations.
- **Escape hatch.** Because the index is always rebuildable from the
  filesystem, if a migration is too painful we can bump the version,
  drop the DB, and trigger full reconciliation. Expensive (O(vault
  size)) but always correct.
- **SCHEMA.md is updated** to reflect each new version; the
  `schema_version` frontmatter is the authoritative "current state"
  indicator.

## Out of scope for version 1

- **FTS5 full-text search** — add when a "search notes" feature lands.
- **Per-portfolio staleness materialisation** — derivable from
  `MAX(notes.mtime_ns) WHERE path LIKE 'portfolios/foo/%'`; only
  materialise if hot.
- **Last-accessed timestamp** — no current query needs it.
- **Tombstone table for deletions** — not needed while reconciliation
  handles deletes idempotently.
