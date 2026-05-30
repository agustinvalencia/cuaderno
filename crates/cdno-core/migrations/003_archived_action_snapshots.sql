-- Migration 003: archived_action_snapshots table.
--
-- Captures a content-hash snapshot of an action note at the moment it
-- moves to `actions/_done/<year>/`. The append-only lint (design §5.11)
-- verifies that the file's frozen prefix still hashes to the stored
-- value — any mismatch flags an edit to a line that pre-existed at
-- archival, while bytes appended past `frozen_size` are allowed
-- (the "six months later, follow-up" case).
--
-- The snapshot is written once, by `stage_action_archival`. The
-- foreign key cascades on note delete so the row vanishes if the
-- archived file is removed.

CREATE TABLE archived_action_snapshots (
    note_path       TEXT PRIMARY KEY REFERENCES notes(path) ON DELETE CASCADE,
    -- File length at archival time, in bytes.
    frozen_size     INTEGER NOT NULL,
    -- xxh3_64 hex digest of the file's content at archival time.
    -- Lint takes the first `frozen_size` bytes of the current file,
    -- hashes them with the same algorithm, and compares.
    frozen_hash     TEXT NOT NULL,
    -- Wall-clock instant of the archival, nanoseconds since epoch.
    archived_at_ns  INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    3,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'Archived action snapshots for append-only lint'
);
