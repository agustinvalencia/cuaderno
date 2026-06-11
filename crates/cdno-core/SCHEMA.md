---
schema_version: 2
applies_to_crate: cdno-core
last_updated: 2026-06-11
---

# Cuaderno Index Schema

The SQLite index is a **cache**, not the source of truth. The markdown
vault on disk is authoritative; the index exists only to answer read
queries quickly (list active projects, aggregate commitments, find
backlinks, …). If the index is lost, corrupted, or behind, startup
reconciliation rebuilds it from the filesystem — a stale index is
recoverable, a stale file is data loss.

This document describes **schema version 2**. Migrations:
`migrations/001_initial.sql` (the combined initial schema — the
milestones / archive-snapshots additions that landed during Phase 2 were
collapsed into it before any release pinned them in place) and
`migrations/002_fts5_search.sql` (the `notes_fts` full-text index, #172).

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

### `milestones`

The full project milestone timeline (design §5.3) — Gantt-style event
markers extracted from each project's `## Milestones` section at index
time. A **superset** of the `deadlines` `project_milestone` source: it
also carries soft targets and completed markers, so it backs both the
commitments funnel (hard + dated) and project-detail timeline views.
Milestones are deliberately *not* a note type — they fire once and don't
accumulate content.

```sql
CREATE TABLE milestones (
    id        INTEGER PRIMARY KEY,
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    date      TEXT,                       -- ISO YYYY-MM-DD, or NULL for a fuzzy marker
    hard_soft TEXT NOT NULL,              -- 'hard' | 'soft'
    status    TEXT NOT NULL               -- 'pending' | 'completed'
);

CREATE INDEX idx_milestones_note ON milestones(note_path);
CREATE INDEX idx_milestones_date ON milestones(date);
```

- **Extraction** parses each `## Milestones` checklist line:
  `- [ ] <name> — hard: YYYY-MM-DD` (pending hard), `- [ ] <name> —
  target: <date|marker>` (pending soft; `date` NULL when the marker
  isn't ISO, e.g. `target: April`), and `- [x] <name> — YYYY-MM-DD`
  (completed). The checkbox sets `status`; `hard:` sets `hard_soft`.
- **`date` is nullable** because an undated milestone is legitimate —
  it just can't appear in `milestones_between` (which filters
  `date IS NOT NULL`).
- `idx_milestones_date` backs the `milestones_between` range scan used
  by commitments aggregation; `idx_milestones_note` supports cascade
  and the clear-and-reinsert pattern on reconcile.
- **Relationship to `deadlines`**: the project-milestone source is
  currently materialised in *both* tables — `deadlines` keeps the hard
  subset for the existing commitments path. The commitments aggregation
  (#32) is expected to read project deadlines from `milestones_between`
  and the redundant `deadlines` project feed retired at that point.

### `archived_action_snapshots`

The baseline an action note's prefix is locked against once it moves to
`actions/_done/<year>/`. Captured once by `stage_action_archival` and
read by the append-only-after-completion lint (design §5.11): the lint
re-hashes the current file's first `frozen_size` bytes and flags any
mismatch, while bytes appended past that point are allowed (the "six
months later, follow-up" case from the decision note).

```sql
CREATE TABLE archived_action_snapshots (
    note_path       TEXT PRIMARY KEY REFERENCES notes(path) ON DELETE CASCADE,
    frozen_size     INTEGER NOT NULL,    -- file length at archival, bytes
    frozen_hash     TEXT NOT NULL,       -- xxh3_64 hex digest at archival
    archived_at_ns  INTEGER NOT NULL     -- wall-clock instant of archival
) WITHOUT ROWID;
```

- **Set once, never reconciled.** Reconciliation rebuilds `notes`,
  `deadlines`, `links`, `note_tags`, and `milestones` from the
  filesystem; snapshots persist on their own. Cascade on note delete
  drops them with the file.
- **Hash matches `content_hash`**: same xxh3_64 algorithm, so the same
  helper computes both. The frozen_hash is over the full file content at
  archival (frontmatter + body); any edit anywhere in the prefix flags.

### `notes_fts`

FTS5 inverted index over note title + body — the engine behind content
search ("where did we say X?", #172). A **regular (self-contained)** FTS5
table: it stores both the index and a copy of the indexed text, so
`snippet()` and `bm25()` work directly off it with no second read of the
`.md` file. The duplicated text is in-character for a cache — disposable
and rebuildable from markdown.

```sql
CREATE VIRTUAL TABLE notes_fts USING fts5(
    path UNINDEXED,                  -- stored for lookup/delete, not searched
    title,
    body,
    tokenize = 'porter unicode61'    -- stem so meeting/meetings/met collapse
);
```

- **Keyed by `path`, not rowid.** `notes` is `WITHOUT ROWID`, so there's
  no rowid to align an external-content FTS table against. `path` is an
  `UNINDEXED` column purely to look up and delete a row; deletion is a
  `WHERE path = ?` scan — negligible at personal-vault scale (a
  `path→rowid` side-table is the optimisation if a profile ever shows it).
- **Maintained two ways, mirroring the `notes` table.** Writes update it
  incrementally so same-session search is live; reconciliation heals it.
  Because `NoteEntry` carries no body, the write path derives the FTS body
  from the paired `write_file` content at `VaultTransaction::commit` — one
  seam covering every write call site rather than 20-odd manual hooks.
- **Porter stemming** biases for forgiving recall (search the plural, find
  the singular); `bm25()` weights a title hit 10× a body hit.

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
| `milestones_for_project(slug)` | `SELECT … FROM milestones WHERE note_path = ? OR note_path = ? ORDER BY id` — resolves slug to active + parked paths. |
| `milestones_between(from, to)` | `SELECT … FROM milestones WHERE date IS NOT NULL AND date BETWEEN ? AND ? ORDER BY date` — `idx_milestones_date` range scan. |
| `find_archival_snapshot(path)` | `SELECT frozen_size, frozen_hash, archived_at_ns FROM archived_action_snapshots WHERE note_path = ?` — PK lookup. |
| `search(query, limit)` | `SELECT f.path, n.note_type, f.title, snippet(...), bm25(notes_fts, 0, 10, 1) FROM notes_fts f JOIN notes n ON n.path = f.path WHERE notes_fts MATCH ? ORDER BY bm25(...) LIMIT ?` — FTS5 index lookup, ranked best-first. |

## Reconciliation semantics

Startup reconciliation (#16) treats the filesystem as truth:

1. `walk_dir` the vault for every `.md` file.
2. For each file: compare `mtime_ns + content_hash` with the `notes` row.
   - Missing row → insert, also insert deadlines, links, tags, milestones.
   - Matching row → skip.
   - Changed row → update `notes`, delete and re-insert dependent rows
     (deadlines, links, tags, milestones) via cascading writes.
3. For every `notes.path` not visited in the walk → delete. Cascades
   clean up dependents (and the explicit FTS delete in `remove_note`).
4. **FTS heal.** Diff `notes` paths against `notes_fts` paths: backfill
   any note missing from search (read file → parse body → `replace_fts`),
   drop any FTS row whose note is gone. This is decoupled from the step-2
   hash fast-path on purpose — an unchanged note is *skipped* there, so a
   freshly-migrated (or dropped) FTS table is filled here, not in step 2.

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

## Out of scope (for now)

- **Search filters** (note type, date range, portfolio) on top of
  `notes_fts` — the core `search(query, limit)` is filter-free; filtering
  lands with the domain `Vault::search` + `search_notes` MCP surface (#172
  PR 2).
- **Frontmatter fields in the FTS index** — only title + body are
  indexed; structured frontmatter is better queried via the typed columns.
- **Per-portfolio staleness materialisation** — derivable from
  `MAX(notes.mtime_ns) WHERE path LIKE 'portfolios/foo/%'`; only
  materialise if hot.
- **Last-accessed timestamp** — no current query needs it.
- **Tombstone table for deletions** — not needed while reconciliation
  handles deletes idempotently.
