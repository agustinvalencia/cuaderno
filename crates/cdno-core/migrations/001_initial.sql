-- Migration 001: initial schema.
--
-- See crates/cdno-core/SCHEMA.md for table rationale and query patterns.
-- The index is a cache rebuildable from the filesystem; this migration
-- establishes the shape used at schema version 1.

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

CREATE TABLE schema_migrations (
    version        INTEGER PRIMARY KEY,
    applied_at_ns  INTEGER NOT NULL,
    description    TEXT
);

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    1,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'Initial schema: notes, deadlines, links, note_tags, schema_migrations'
);
