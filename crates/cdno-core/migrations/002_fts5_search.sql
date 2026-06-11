-- Migration 002: FTS5 full-text content search.
--
-- Adds an inverted index over note title + body so the vault becomes
-- content-searchable ("where did we say X?") rather than only
-- structurally addressable by type/slug/date. See crates/cdno-core/SCHEMA.md
-- and issue #172 for the design rationale (storage model, lifecycle,
-- ranking, tokenizer).
--
-- Storage model: a regular (self-contained) FTS5 table — it stores both
-- the inverted index and a copy of the indexed text, so snippet()/bm25()
-- work directly off the table with no second read of the .md file. The
-- duplicated text is in-character for the index: it is a disposable cache,
-- rebuildable from markdown by reconciliation.
--
-- `path` is stored UNINDEXED purely so a row can be looked up and deleted
-- by note path. The `notes` table is WITHOUT ROWID, so there is no rowid
-- to align an external-content FTS table against; keying by path and
-- deleting by path is the pragmatic choice at personal-vault scale.
--
-- Tokenizer: `porter` (over the default `unicode61`) stems words to their
-- roots so `meeting`/`meetings`/`met` collapse together — forgiving recall
-- suits a personal "I don't remember the exact word" search.
--
-- This migration only creates the table. Population is the lifecycle's job:
-- writes maintain it incrementally (VaultTransaction commit seam) and the
-- reconciliation FTS-heal pass backfills any note missing from it — which
-- is what fills this table for an existing vault on first open after the
-- migration, since the per-file reconcile fast-path skips unchanged notes.

CREATE VIRTUAL TABLE notes_fts USING fts5(
    path UNINDEXED,
    title,
    body,
    tokenize = 'porter unicode61'
);

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    2,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'FTS5 full-text search over note title + body'
);
