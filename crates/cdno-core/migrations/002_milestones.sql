-- Migration 002: milestones index table.
--
-- Project `## Milestones` lines are extracted into a first-class,
-- queryable table during reconciliation (design §5.3). Milestones are
-- Gantt-style event markers, not a note type — this table gives O(1)
-- timeline queries without inflating the note surface. It is a superset
-- of the hard deadlines fed into `deadlines`: it also carries soft
-- targets and completed markers.

CREATE TABLE milestones (
    id        INTEGER PRIMARY KEY,
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    -- ISO `YYYY-MM-DD`, or NULL for a non-date marker like `target: April`.
    date      TEXT,
    -- `hard` | `soft`. `hard:` lines are hard; soft targets and
    -- keyword-less completed markers are soft.
    hard_soft TEXT NOT NULL,
    -- `pending` | `completed`, derived from the `- [ ]` / `- [x]` checkbox.
    status    TEXT NOT NULL
);

CREATE INDEX idx_milestones_note ON milestones(note_path);
CREATE INDEX idx_milestones_date ON milestones(date);

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    2,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'Milestones index table'
);
