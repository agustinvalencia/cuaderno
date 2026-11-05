-- Migration 003: `links.via_alias` (RFC 0007 stage S3, #860).
--
-- A wikilink whose last segment names no note stem may resolve through a
-- slug-shaped `aliases:` entry on exactly one note. The row records that it
-- did, so a later change to the alias set knows which edges depend on it and
-- lint can tell a straggler (a link through an old name) from a direct one.
-- Every existing row resolved without aliases, so 0 is its true value.

ALTER TABLE links ADD COLUMN via_alias INTEGER NOT NULL DEFAULT 0;

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    3,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'links.via_alias: the row resolved through a note alias'
);
