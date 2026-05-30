-- Migration 001: initial schema.
--
-- See crates/cdno-core/SCHEMA.md for full table rationale and query
-- patterns. The schema is delivered as a single migration because
-- nothing has shipped yet — pre-release is the cheapest time to keep
-- the schema-of-record uncluttered. The first post-release additive
-- change becomes migration 002.

CREATE TABLE notes (
    path          TEXT PRIMARY KEY,
    note_type     TEXT NOT NULL,
    title         TEXT,
    content_hash  TEXT NOT NULL,
    mtime_ns      INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    frontmatter   TEXT NOT NULL,
    indexed_at_ns INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX idx_notes_type ON notes(note_type);

CREATE TABLE deadlines (
    id         INTEGER PRIMARY KEY,
    note_path  TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    source     TEXT NOT NULL,
    title      TEXT NOT NULL,
    due_date   TEXT NOT NULL,
    is_hard    INTEGER NOT NULL,
    context    TEXT
);

CREATE INDEX idx_deadlines_due  ON deadlines(due_date);
CREATE INDEX idx_deadlines_note ON deadlines(note_path);

CREATE TABLE links (
    id            INTEGER PRIMARY KEY,
    source_path   TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    target_raw    TEXT NOT NULL,
    resolved_path TEXT,
    label         TEXT
);

CREATE INDEX idx_links_source   ON links(source_path);
CREATE INDEX idx_links_resolved ON links(resolved_path);

CREATE TABLE note_tags (
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    tag       TEXT NOT NULL,
    PRIMARY KEY (note_path, tag)
) WITHOUT ROWID;

CREATE INDEX idx_tags_tag ON note_tags(tag);

-- Project `## Milestones` lines extracted into a queryable timeline
-- (design §5.3). Superset of the hard-deadline rows in `deadlines`:
-- it carries soft targets and completed markers too.
CREATE TABLE milestones (
    id        INTEGER PRIMARY KEY,
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    date      TEXT,
    hard_soft TEXT NOT NULL,
    status    TEXT NOT NULL
);

CREATE INDEX idx_milestones_note ON milestones(note_path);
CREATE INDEX idx_milestones_date ON milestones(date);

-- Baseline for the append-only-after-completion lint (design §5.11).
-- Captured by `stage_action_archival` when an attached action moves to
-- `actions/_done/<year>/`; the lint re-hashes the file's first
-- `frozen_size` bytes and flags any mismatch.
CREATE TABLE archived_action_snapshots (
    note_path       TEXT PRIMARY KEY REFERENCES notes(path) ON DELETE CASCADE,
    frozen_size     INTEGER NOT NULL,
    frozen_hash     TEXT NOT NULL,
    archived_at_ns  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE schema_migrations (
    version        INTEGER PRIMARY KEY,
    applied_at_ns  INTEGER NOT NULL,
    description    TEXT
);

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    1,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'Initial schema: notes, deadlines, links, note_tags, milestones, archived_action_snapshots'
);
