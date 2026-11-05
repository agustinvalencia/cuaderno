-- Migration 004: the masked snapshot hash (RFC 0007 stage S4, #864).
--
-- A second hash over the frozen prefix with every wikilink address and every
-- reference-field value masked, so a rename that rewrites a reference inside
-- a frozen prefix leaves the snapshot valid. `mask_version` records the
-- masking rule it was computed under. Both are NULL until stamped or
-- backfilled: an existing row has no masked hash yet.

ALTER TABLE archived_action_snapshots ADD COLUMN masked_hash TEXT;
ALTER TABLE archived_action_snapshots ADD COLUMN mask_version INTEGER;

INSERT INTO schema_migrations (version, applied_at_ns, description)
VALUES (
    4,
    CAST((julianday('now') - 2440587.5) * 86400000000000 AS INTEGER),
    'archived_action_snapshots: masked_hash and mask_version'
);
