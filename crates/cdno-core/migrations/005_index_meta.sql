-- Migration 005: facts about the index itself (#998).
--
-- A key/value table. Its one key today is `vault_root`: an index opened at an
-- overridden location (`CUADERNO_INDEX_PATH`, `--index-path`) records the
-- canonical root of the vault it belongs to, and an open for another vault is
-- refused. An index at the default `.cuaderno/index.db` records nothing.

CREATE TABLE index_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    5,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'index_meta: key/value facts about the index itself (the bound vault root)'
);
