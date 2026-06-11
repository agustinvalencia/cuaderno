//! SQLite-backed vault index.
//!
//! The index is a cache — filesystem state is authoritative, reconciliation
//! rebuilds the index on any inconsistency. See `SCHEMA.md` for the table
//! shapes and rationale.
//!
//! This module owns connection setup, migrations, the [`VaultIndex`] trait,
//! and a concrete [`SqliteIndex`] implementation.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value as JsonValue;

use crate::error::IndexError;
use crate::path::VaultPath;

/// Ordered list of embedded migrations: (version, description, sql).
///
/// Every schema change is a new entry appended here. Migrations are
/// applied strictly in order; never edit a migration that has shipped.
const MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "Initial schema: notes, deadlines, links, note_tags, milestones, archived_action_snapshots",
        include_str!("../migrations/001_initial.sql"),
    ),
    (
        2,
        "FTS5 full-text search over note title + body",
        include_str!("../migrations/002_fts5_search.sql"),
    ),
];

/// SQLite-backed implementation of the vault index.
///
/// Single [`Mutex<Connection>`] for now. This serialises reads and
/// writes through one connection, which is fine for CLI invocations
/// and simple MCP/Tauri access patterns. Upgrade path: swap for an
/// `r2d2`/`r2d2_sqlite` pool when read concurrency becomes a
/// measurable bottleneck — e.g. an agentic flow where multiple
/// workers read vault state while one writes. SQLite itself always
/// serialises writers, so a pool helps readers only; don't add the
/// dep until there's evidence.
pub struct SqliteIndex {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SqliteIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection doesn't implement Debug usefully, so surface
        // just the type name rather than the full lock state.
        f.debug_struct("SqliteIndex").finish_non_exhaustive()
    }
}

impl SqliteIndex {
    /// Open or create the index at `path`, creating missing parent
    /// directories and applying any pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IndexError> {
        let path = path.as_ref();

        // Ensure the containing directory exists so rusqlite can create
        // the file on first open. Silently a no-op if already present.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| IndexError::Open {
                path: path.display().to_string(),
                source: e,
            })?;
        }

        let conn = Connection::open(path)?;
        configure_connection(&conn)?;
        apply_pending_migrations(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run `PRAGMA quick_check` — the cheap subset of `integrity_check`
    /// that still catches the common corruption modes.
    ///
    /// Not invoked automatically on open: for a CLI-style invocation
    /// pattern, every command is a fresh startup and this would add
    /// tens of milliseconds to each call. Callers that care about
    /// integrity (e.g. a future `cdno doctor`) invoke this explicitly.
    /// If any routine query elsewhere returns a corruption-shaped
    /// error, the recovery path is to drop the index and reconcile.
    pub fn check_integrity(&self) -> Result<bool, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        Ok(result == "ok")
    }

    /// Borrow the underlying connection under the mutex for ad-hoc
    /// operations that haven't been promoted to typed methods yet.
    ///
    /// Exists primarily for tests and for the CRUD work in issue #12.
    /// Prefer typed methods once they land.
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, IndexError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T>,
    {
        let conn = self.conn.lock().expect("poisoned mutex");
        f(&conn).map_err(IndexError::from)
    }
}

/// Apply the PRAGMAs required by the schema contract on every new
/// connection. Most of these are per-connection (not persisted in the
/// file) and must be re-set on each open.
fn configure_connection(conn: &Connection) -> Result<(), IndexError> {
    // Cascade deletes on foreign keys. Without this the ON DELETE
    // CASCADE clauses in the schema are silently ignored.
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // WAL mode lets readers proceed concurrently with a writer. This
    // is persisted in the database header, but re-setting is cheap
    // and documents intent.
    conn.pragma_update(None, "journal_mode", "WAL")?;

    // With WAL, NORMAL sync is durable under crash and much faster
    // than FULL. See https://www.sqlite.org/pragma.html#pragma_synchronous.
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    // Keep temp tables in memory; cheap speedup with no durability cost.
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    // Avoid spurious SQLITE_BUSY on brief writer contention. 5s is
    // generous for a personal-scale vault but trips quickly enough
    // that a genuine deadlock still surfaces.
    conn.busy_timeout(Duration::from_millis(5000))?;

    Ok(())
}

/// Read the current schema version (0 if the `schema_migrations`
/// table doesn't exist yet) and apply every migration whose version
/// exceeds it. Each migration runs in a transaction so a partial
/// failure leaves the database at the previous version.
fn apply_pending_migrations(conn: &Connection) -> Result<(), IndexError> {
    let current = current_schema_version(conn)?;

    for (version, description, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        apply_one_migration(conn, *version, description, sql)?;
    }

    Ok(())
}

/// Return the maximum applied migration version, or 0 if no migration
/// has ever run on this database.
fn current_schema_version(conn: &Connection) -> Result<u32, IndexError> {
    // Probe for the migrations table first — on a fresh database it
    // doesn't exist, and querying MAX(version) would error.
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
            [],
            |r| {
                let b: i64 = r.get(0)?;
                Ok(b != 0)
            },
        )?;

    if !has_table {
        return Ok(0);
    }

    // Fresh tables (after a migration that created the row) will have
    // one or more entries; pre-migration state has none — treat that
    // as version 0 so the first migration runs.
    let version: Option<u32> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })?;
    Ok(version.unwrap_or(0))
}

/// Apply a single migration inside a transaction. The migration SQL
/// is expected to be self-contained — it creates its own schema
/// objects and inserts its own `schema_migrations` row.
fn apply_one_migration(
    conn: &Connection,
    version: u32,
    description: &str,
    sql: &str,
) -> Result<(), IndexError> {
    conn.execute_batch(&format!("BEGIN; {sql} COMMIT;"))
        .map_err(|e| IndexError::Migration {
            version,
            reason: format!("{description}: {e}"),
        })
}

// ---------------------------------------------------------------------
// VaultIndex trait + entry types + SqliteIndex implementation
// ---------------------------------------------------------------------

/// Plain-data view of a row in the `notes` table.
///
/// Kept as a value type with no behaviour: this is the data the index
/// hands back, and what callers pass in to `upsert_note`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEntry {
    pub path: VaultPath,
    pub note_type: String,
    pub title: Option<String>,
    pub content_hash: String,
    pub mtime_ns: u64,
    pub size: u64,
    /// Full frontmatter as JSON. Stored as `TEXT` in SQLite; deserialised
    /// on the way out so callers get a structured value.
    pub frontmatter: JsonValue,
    pub indexed_at_ns: u64,
}

/// One row of the `deadlines` table, scoped to a single note.
#[derive(Debug, Clone, PartialEq)]
pub struct DeadlineEntry {
    /// Which of the three aggregation sources this deadline came from:
    /// `"project_milestone" | "stewardship_periodic" | "commitment_note"`.
    pub source: String,
    pub title: String,
    /// ISO-8601 `YYYY-MM-DD`.
    pub due_date: String,
    pub is_hard: bool,
    pub context: Option<String>,
}

/// One row of the `links` table, scoped to a single source note.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkEntry {
    pub target_raw: String,
    pub resolved_path: Option<VaultPath>,
    pub label: Option<String>,
}

/// One row of the `milestones` table, scoped to a single project note.
///
/// A superset of [`DeadlineEntry`]'s project-milestone source: it
/// captures soft targets and completed markers as well as hard
/// deadlines. `date` is `None` for non-date markers (`target: April`),
/// which can't participate in date-window queries.
#[derive(Debug, Clone, PartialEq)]
pub struct MilestoneEntry {
    pub name: String,
    /// ISO `YYYY-MM-DD`, or `None` for a non-date marker.
    pub date: Option<String>,
    /// `true` for `hard:` deadlines; `false` for soft targets and
    /// keyword-less completed markers.
    pub is_hard: bool,
    /// `true` when the source checkbox is `- [x]`.
    pub completed: bool,
}

/// Snapshot of an action note at the moment it was archived to
/// `actions/_done/<year>/`. The append-only lint (#111, design §5.11)
/// re-hashes the file's first `frozen_size` bytes on each lint run
/// and flags any divergence; bytes appended past `frozen_size` are
/// allowed (late retrospectives). Set once by `stage_action_archival`
/// and never overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivalSnapshot {
    /// File length captured at archival, in bytes.
    pub frozen_size: u64,
    /// xxh3_64 hex digest of the file content at archival.
    pub frozen_hash: String,
    /// Wall-clock instant of the archival, ns since the UNIX epoch.
    pub archived_at_ns: u64,
}

/// One ranked full-text search result from [`VaultIndex::search`].
///
/// `snippet` is an excerpt of the matched body with the query terms
/// wrapped in `[`…`]`, straight from FTS5's `snippet()`. `score` is the
/// raw `bm25()` relevance value — **lower is a better match** (it is the
/// sort key, surfaced so callers can threshold or display it). `note_type`
/// is joined in from the `notes` row so callers can filter/group without a
/// second lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub path: VaultPath,
    pub note_type: String,
    pub title: Option<String>,
    pub snippet: String,
    pub score: f64,
}

/// The two vault locations a project with `slug` can occupy: active
/// `projects/<slug>.md` and parked `projects/_parked/<slug>.md`. Used
/// by `milestones_for_project` to resolve a slug to its note path
/// without a separate index lookup. Projects are the only note type
/// that contributes milestones, and they live only in these two
/// directories (design §4 vault structure).
fn project_path_candidates(slug: &str) -> [String; 2] {
    [
        format!("{}/{slug}.md", crate::paths::PROJECTS),
        format!("{}/{slug}.md", crate::paths::PROJECTS_PARKED),
    ]
}

/// Cache-oriented query API for the vault index.
///
/// All methods take `&self`; implementations are responsible for
/// interior mutability and transaction boundaries. Each `replace_*`
/// call is expected to be atomic — a partial failure must leave the
/// prior facet set intact.
///
/// The trait is `Send + Sync` so one implementation can be shared
/// through an `Arc` across MCP handlers, Tauri commands, and
/// background reconciliation.
pub trait VaultIndex: Send + Sync {
    // notes -----------------------------------------------------------
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError>;
    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError>;
    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError>;
    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError>;
    /// Return every path currently in the index. Used by reconciliation
    /// to find orphans (index rows with no corresponding file on disk).
    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError>;

    // deadlines -------------------------------------------------------
    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError>;
    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError>;

    // links -----------------------------------------------------------
    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError>;
    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError>;
    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError>;

    // tags ------------------------------------------------------------
    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError>;
    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError>;

    // milestones ------------------------------------------------------
    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError>;
    /// Every milestone of the project named `slug`, in source order
    /// (by row id). Resolves the slug against both the active and
    /// parked project locations.
    fn milestones_for_project(&self, slug: &str) -> Result<Vec<MilestoneEntry>, IndexError>;
    /// Dated milestones across all projects whose `date` falls in the
    /// inclusive `[from, to]` window, sorted by date. Non-date markers
    /// (`date IS NULL`) are excluded — they can't be placed on a
    /// timeline. This is the source the commitments aggregation (#32)
    /// reads project deadlines from.
    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError>;

    // full-text search ------------------------------------------------
    /// Replace the FTS row for `path` with the given title + body
    /// (delete-then-insert; FTS5 has no clean per-row UPSERT). Mirrors
    /// the `replace_*` facet idiom: the note's searchable text is just
    /// another projection of the note, refreshed on every write.
    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError>;
    /// Full-text search over title + body, ranked best-first. `query` is
    /// an FTS5 MATCH expression (bare terms, `"phrases"`, `AND`/`OR`,
    /// `prefix*`). Title matches are weighted above body matches. At most
    /// `limit` hits are returned.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError>;
    /// Every path currently present in the FTS index. Used by the
    /// reconciliation FTS-heal pass to diff against `notes` and backfill
    /// any note missing from search (independent of the per-file hash
    /// fast-path, so it fills a just-migrated/dropped FTS table).
    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError>;

    // archival snapshots -------------------------------------------------
    /// Record (or replace) the archival snapshot for `path`. Written
    /// once at archival by `stage_action_archival`; subsequent writes
    /// for the same path simply overwrite, which is harmless because
    /// the snapshot only matters as a stable per-path baseline.
    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError>;
    /// Look up the archival snapshot for `path`, or `None` if there
    /// isn't one (the note was never archived through the lifecycle).
    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError>;
}

impl VaultIndex for SqliteIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        // INSERT OR REPLACE is shorthand for "delete-if-exists then
        // insert". For the notes table that's exactly the semantics we
        // want: upsert by primary key `path`. The dependent tables
        // (deadlines, links, note_tags) are refreshed through their own
        // `replace_*` calls — cascades from INSERT OR REPLACE would
        // silently drop those rows, which would be surprising.
        let frontmatter_json = serde_json::to_string(&entry.frontmatter)
            .map_err(|e| IndexError::Update(format!("failed to serialise frontmatter: {e}")))?;
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.execute(
            "INSERT INTO notes (path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(path) DO UPDATE SET \
                 note_type = excluded.note_type, \
                 title = excluded.title, \
                 content_hash = excluded.content_hash, \
                 mtime_ns = excluded.mtime_ns, \
                 size = excluded.size, \
                 frontmatter = excluded.frontmatter, \
                 indexed_at_ns = excluded.indexed_at_ns",
            params![
                entry.path.to_string(),
                entry.note_type,
                entry.title,
                entry.content_hash,
                entry.mtime_ns as i64,
                entry.size as i64,
                frontmatter_json,
                entry.indexed_at_ns as i64,
            ],
        )?;
        Ok(())
    }

    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        // Cascade FKs drop dependent rows in deadlines, links, note_tags.
        // `notes_fts` is a virtual table, so FK cascade doesn't reach it —
        // delete its row explicitly under the same lock to keep search in
        // step with the notes table.
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.execute(
            "DELETE FROM notes WHERE path = ?1",
            params![path.to_string()],
        )?;
        conn.execute(
            "DELETE FROM notes_fts WHERE path = ?1",
            params![path.to_string()],
        )?;
        Ok(())
    }

    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.query_row(
            "SELECT path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns \
             FROM notes WHERE path = ?1",
            params![path.to_string()],
            row_to_note_entry,
        )
        .optional()
        .map_err(IndexError::from)?
        .transpose()
    }

    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns \
             FROM notes WHERE note_type = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map(params![note_type], row_to_note_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare("SELECT path FROM notes ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored path in list_all_paths: {e}"))
            })?);
        }
        Ok(out)
    }

    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        // Delete-then-insert under one transaction: FTS5 regular tables
        // have no clean per-row UPSERT, and an UNINDEXED key column can't
        // anchor an ON CONFLICT. The delete is keyed on the UNINDEXED
        // `path` column (an O(n) scan — negligible at vault scale; a
        // path->rowid side-table is the optimisation if a profile ever
        // shows it).
        let mut conn = self.conn.lock().expect("poisoned mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM notes_fts WHERE path = ?1",
            params![path.to_string()],
        )?;
        tx.execute(
            "INSERT INTO notes_fts (path, title, body) VALUES (?1, ?2, ?3)",
            params![path.to_string(), title, body],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        // Column order in `notes_fts` is (path, title, body) => indices
        // (0, 1, 2). bm25 weights mirror that: path contributes nothing
        // (UNINDEXED anyway), a title hit is weighted 10x a body hit so a
        // note *about* the query outranks one that mentions it in passing.
        // bm25 returns lower-is-better, so ORDER BY ascending is best-first.
        // `snippet`'s column arg is -1 (auto-select): it excerpts whichever
        // column actually matched, so a title-only hit brackets the title
        // term instead of returning an unrelated body prefix.
        // The JOIN to `notes` is inner by design — an FTS row with no
        // surviving note (transient reconcile state) is dropped, not shown.
        let mut stmt = conn.prepare(
            "SELECT f.path, n.note_type, f.title, \
                    snippet(notes_fts, -1, '[', ']', '…', 10), \
                    bm25(notes_fts, 0.0, 10.0, 1.0) \
             FROM notes_fts f \
             JOIN notes n ON n.path = f.path \
             WHERE notes_fts MATCH ?1 \
             ORDER BY bm25(notes_fts, 0.0, 10.0, 1.0) \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            let path_str: String = row.get(0)?;
            Ok((
                path_str,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, note_type, title, snippet, score) = row?;
            let path = VaultPath::new(&path_str)
                .map_err(|e| IndexError::Query(format!("invalid stored path in search: {e}")))?;
            out.push(SearchHit {
                path,
                note_type,
                title,
                snippet,
                score,
            });
        }
        Ok(out)
    }

    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare("SELECT path FROM notes_fts ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored path in fts_indexed_paths: {e}"))
            })?);
        }
        Ok(out)
    }

    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().expect("poisoned mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM deadlines WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO deadlines (note_path, source, title, due_date, is_hard, context) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for d in deadlines {
                stmt.execute(params![
                    path.to_string(),
                    d.source,
                    d.title,
                    d.due_date,
                    d.is_hard as i64,
                    d.context,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT note_path, source, title, due_date, is_hard, context \
             FROM deadlines WHERE due_date BETWEEN ?1 AND ?2 ORDER BY due_date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            let path_str: String = r.get(0)?;
            let entry = DeadlineEntry {
                source: r.get(1)?,
                title: r.get(2)?,
                due_date: r.get(3)?,
                is_hard: r.get::<_, i64>(4)? != 0,
                context: r.get(5)?,
            };
            Ok((path_str, entry))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, entry) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored VaultPath in deadlines: {e}"))
            })?;
            out.push((path, entry));
        }
        Ok(out)
    }

    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().expect("poisoned mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM links WHERE source_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO links (source_path, target_raw, resolved_path, label) \
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for link in links {
                stmt.execute(params![
                    path.to_string(),
                    link.target_raw,
                    link.resolved_path.as_ref().map(|p| p.to_string()),
                    link.label,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT DISTINCT source_path FROM links WHERE resolved_path = ?1 ORDER BY source_path",
        )?;
        let rows = stmt.query_map(params![path.to_string()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(
                VaultPath::new(path_str)
                    .map_err(|e| IndexError::Query(format!("invalid stored backlink path: {e}")))?,
            );
        }
        Ok(out)
    }

    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT target_raw, resolved_path, label FROM links WHERE source_path = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![path.to_string()], |r| {
            let resolved: Option<String> = r.get(1)?;
            Ok((
                r.get::<_, String>(0)?,
                resolved,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (target_raw, resolved_str, label) = row?;
            let resolved_path = match resolved_str {
                Some(s) => Some(VaultPath::new(s).map_err(|e| {
                    IndexError::Query(format!("invalid stored resolved_path: {e}"))
                })?),
                None => None,
            };
            out.push(LinkEntry {
                target_raw,
                resolved_path,
                label,
            });
        }
        Ok(out)
    }

    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().expect("poisoned mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM note_tags WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO note_tags (note_path, tag) VALUES (?1, ?2)")?;
            for tag in tags {
                stmt.execute(params![path.to_string(), tag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt =
            conn.prepare("SELECT note_path FROM note_tags WHERE tag = ?1 ORDER BY note_path")?;
        let rows = stmt.query_map(params![tag], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(
                VaultPath::new(path_str)
                    .map_err(|e| IndexError::Query(format!("invalid tag path: {e}")))?,
            );
        }
        Ok(out)
    }

    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().expect("poisoned mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM milestones WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO milestones (note_path, name, date, hard_soft, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for m in milestones {
                stmt.execute(params![
                    path.to_string(),
                    m.name,
                    m.date,
                    hard_soft_str(m.is_hard),
                    status_str(m.completed),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn milestones_for_project(&self, slug: &str) -> Result<Vec<MilestoneEntry>, IndexError> {
        let [active, parked] = project_path_candidates(slug);
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT name, date, hard_soft, status FROM milestones \
             WHERE note_path = ?1 OR note_path = ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![active, parked], row_to_milestone_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        let mut stmt = conn.prepare(
            "SELECT note_path, name, date, hard_soft, status FROM milestones \
             WHERE date IS NOT NULL AND date BETWEEN ?1 AND ?2 ORDER BY date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            let path_str: String = r.get(0)?;
            Ok((path_str, row_to_milestone_entry_from(r, 1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, entry) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored VaultPath in milestones: {e}"))
            })?;
            out.push((path, entry));
        }
        Ok(out)
    }

    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.execute(
            "INSERT INTO archived_action_snapshots (note_path, frozen_size, frozen_hash, archived_at_ns) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(note_path) DO UPDATE SET \
                 frozen_size = excluded.frozen_size, \
                 frozen_hash = excluded.frozen_hash, \
                 archived_at_ns = excluded.archived_at_ns",
            params![
                path.to_string(),
                snapshot.frozen_size as i64,
                snapshot.frozen_hash,
                snapshot.archived_at_ns as i64,
            ],
        )?;
        Ok(())
    }

    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError> {
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.query_row(
            "SELECT frozen_size, frozen_hash, archived_at_ns FROM archived_action_snapshots \
             WHERE note_path = ?1",
            params![path.to_string()],
            |r| {
                Ok(ArchivalSnapshot {
                    frozen_size: r.get::<_, i64>(0)? as u64,
                    frozen_hash: r.get(1)?,
                    archived_at_ns: r.get::<_, i64>(2)? as u64,
                })
            },
        )
        .optional()
        .map_err(IndexError::from)
    }
}

/// Encode the `is_hard` flag as the `hard_soft` column's text value.
fn hard_soft_str(is_hard: bool) -> &'static str {
    if is_hard { "hard" } else { "soft" }
}

/// Encode the `completed` flag as the `status` column's text value.
fn status_str(completed: bool) -> &'static str {
    if completed { "completed" } else { "pending" }
}

/// Build a [`MilestoneEntry`] from a row whose columns start at offset
/// `base`: `name, date, hard_soft, status`.
fn row_to_milestone_entry_from(
    row: &rusqlite::Row<'_>,
    base: usize,
) -> rusqlite::Result<MilestoneEntry> {
    let hard_soft: String = row.get(base + 2)?;
    let status: String = row.get(base + 3)?;
    Ok(MilestoneEntry {
        name: row.get(base)?,
        date: row.get(base + 1)?,
        is_hard: hard_soft == "hard",
        completed: status == "completed",
    })
}

/// Row extractor for a `name, date, hard_soft, status` column tuple
/// starting at column 0.
fn row_to_milestone_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MilestoneEntry> {
    row_to_milestone_entry_from(row, 0)
}

/// Shared row extractor for the full `notes` column set. Used by
/// `find_by_path` and `list_by_type`.
fn row_to_note_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<NoteEntry, IndexError>> {
    let path_str: String = row.get(0)?;
    let frontmatter_str: String = row.get(6)?;

    let path = match VaultPath::new(&path_str) {
        Ok(p) => p,
        Err(e) => return Ok(Err(IndexError::Query(format!("invalid stored path: {e}")))),
    };
    let frontmatter: JsonValue = match serde_json::from_str(&frontmatter_str) {
        Ok(v) => v,
        Err(e) => {
            return Ok(Err(IndexError::Query(format!(
                "invalid stored frontmatter JSON: {e}"
            ))));
        }
    };

    Ok(Ok(NoteEntry {
        path,
        note_type: row.get(1)?,
        title: row.get(2)?,
        content_hash: row.get(3)?,
        mtime_ns: row.get::<_, i64>(4)? as u64,
        size: row.get::<_, i64>(5)? as u64,
        frontmatter,
        indexed_at_ns: row.get::<_, i64>(7)? as u64,
    }))
}

/// Build a short excerpt of `body` around the first occurrence of
/// `needle` (already lowercased), bracketing the match — the
/// [`MemoryIndex`] stand-in for FTS5's `snippet()`. Falls back to a
/// prefix of the body when the match is title-only (not in the body).
fn memory_snippet(body: &str, needle: &str) -> String {
    const WINDOW: usize = 40;
    match body.to_lowercase().find(needle) {
        Some(pos) => {
            let start = body[..pos]
                .char_indices()
                .rev()
                .nth(WINDOW)
                .map_or(0, |(i, _)| i);
            let match_end = pos + needle.len();
            let end = body[match_end..]
                .char_indices()
                .nth(WINDOW)
                .map_or(body.len(), |(i, _)| match_end + i);
            format!(
                "{}[{}]{}",
                &body[start..pos],
                &body[pos..match_end],
                &body[match_end..end]
            )
        }
        None => body.chars().take(WINDOW * 2).collect(),
    }
}

// ---------------------------------------------------------------------
// MemoryIndex — test-only in-memory implementation of VaultIndex
// ---------------------------------------------------------------------

/// In-memory [`VaultIndex`] used for fast, deterministic domain tests.
///
/// Backed by `Mutex<MemoryIndexState>` (four `HashMap`s) so it satisfies
/// the trait's `Send + Sync` bound and lets tests share one index by
/// reference without platform-dependent IO. Production code always
/// uses [`SqliteIndex`]; having two impls proves the trait abstraction
/// is real — if domain code silently hard-coded `SqliteIndex`, this
/// suite would catch it at compile time.
///
/// Cross-reference queries (`find_backlinks`, `find_by_tag`,
/// `deadlines_between`) are O(n) linear scans. That's fine at test-fake
/// scale; never call it from production.
#[derive(Debug, Default)]
pub struct MemoryIndex {
    state: Mutex<MemoryIndexState>,
}

/// Internal state: one map per table family, keyed by the source
/// `VaultPath`. Cascading deletes are implemented by removing the path
/// from every map in `remove_note`.
#[derive(Debug, Default)]
struct MemoryIndexState {
    notes: HashMap<VaultPath, NoteEntry>,
    deadlines: HashMap<VaultPath, Vec<DeadlineEntry>>,
    links: HashMap<VaultPath, Vec<LinkEntry>>,
    tags: HashMap<VaultPath, Vec<String>>,
    milestones: HashMap<VaultPath, Vec<MilestoneEntry>>,
    archival_snapshots: HashMap<VaultPath, ArchivalSnapshot>,
    /// Searchable (title, body) text per note, mirroring `notes_fts`.
    fts: HashMap<VaultPath, (Option<String>, String)>,
}

impl MemoryIndex {
    pub fn new() -> Self {
        Self::default()
    }
}

impl VaultIndex for MemoryIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state.notes.insert(entry.path.clone(), entry.clone());
        Ok(())
    }

    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        // No FK engine in memory; mirror `ON DELETE CASCADE` manually
        // by dropping the path from every facet map.
        let mut state = self.state.lock().expect("poisoned mutex");
        state.notes.remove(path);
        state.deadlines.remove(path);
        state.links.remove(path);
        state.tags.remove(path);
        state.milestones.remove(path);
        state.archival_snapshots.remove(path);
        state.fts.remove(path);
        Ok(())
    }

    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        Ok(state.notes.get(path).cloned())
    }

    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<NoteEntry> = state
            .notes
            .values()
            .filter(|n| n.note_type == note_type)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.path.as_path().cmp(b.path.as_path()));
        Ok(out)
    }

    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<VaultPath> = state.notes.keys().cloned().collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state
            .fts
            .insert(path.clone(), (title.map(str::to_owned), body.to_owned()));
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        // Deliberately simple: a case-insensitive substring match over
        // the stored title + body, ranked title-first. This double exists
        // for fast domain tests, not to reproduce FTS5 — it does not
        // honour MATCH operators (`AND`/`OR`/`"phrase"`/`prefix*`) or
        // porter stemming; assert those against `SqliteIndex`.
        let needle = query.to_lowercase();
        let state = self.state.lock().expect("poisoned mutex");
        let mut hits: Vec<SearchHit> = state
            .fts
            .iter()
            .filter_map(|(path, (title, body))| {
                let title_hit = title
                    .as_deref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle));
                let body_hit = body.to_lowercase().contains(&needle);
                if !title_hit && !body_hit {
                    return None;
                }
                let note_type = state
                    .notes
                    .get(path)
                    .map(|n| n.note_type.clone())
                    .unwrap_or_default();
                Some(SearchHit {
                    path: path.clone(),
                    note_type,
                    title: title.clone(),
                    snippet: memory_snippet(body, &needle),
                    // Lower is better; a title hit outranks a body-only
                    // hit, matching the weighted bm25 ordering.
                    score: if title_hit { 0.0 } else { 1.0 },
                })
            })
            .collect();
        // Stable order: score first, then path for determinism.
        hits.sort_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.as_path().cmp(b.path.as_path()))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<VaultPath> = state.fts.keys().cloned().collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state.deadlines.insert(path.clone(), deadlines.to_vec());
        Ok(())
    }

    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<(VaultPath, DeadlineEntry)> = state
            .deadlines
            .iter()
            .flat_map(|(path, entries)| {
                entries
                    .iter()
                    .filter(|d| d.due_date.as_str() >= from && d.due_date.as_str() <= to)
                    .map(move |d| (path.clone(), d.clone()))
            })
            .collect();
        // Match SqliteIndex's `ORDER BY due_date`.
        out.sort_by(|a, b| a.1.due_date.cmp(&b.1.due_date));
        Ok(out)
    }

    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state.links.insert(path.clone(), links.to_vec());
        Ok(())
    }

    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        // Distinct source paths where any link resolves to `path`.
        // A note with multiple links to the same target still appears
        // once, matching SqliteIndex's SELECT DISTINCT.
        let mut seen: Vec<VaultPath> = Vec::new();
        for (source_path, entries) in state.links.iter() {
            if entries
                .iter()
                .any(|l| l.resolved_path.as_ref() == Some(path))
                && !seen.contains(source_path)
            {
                seen.push(source_path.clone());
            }
        }
        seen.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(seen)
    }

    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        // Insertion order preserved, matching SqliteIndex's `ORDER BY id`
        // (rowids are monotonic with insertion).
        Ok(state.links.get(path).cloned().unwrap_or_default())
    }

    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        // Dedupe in-place to match SqliteIndex's "INSERT OR IGNORE"
        // against the (note_path, tag) primary key. A duplicate tag
        // in the caller's input silently becomes a single entry.
        let mut deduped: Vec<String> = Vec::with_capacity(tags.len());
        for t in tags {
            if !deduped.contains(t) {
                deduped.push(t.clone());
            }
        }
        state.tags.insert(path.clone(), deduped);
        Ok(())
    }

    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<VaultPath> = state
            .tags
            .iter()
            .filter_map(|(path, tags)| {
                if tags.iter().any(|t| t == tag) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state.milestones.insert(path.clone(), milestones.to_vec());
        Ok(())
    }

    fn milestones_for_project(&self, slug: &str) -> Result<Vec<MilestoneEntry>, IndexError> {
        let candidates = project_path_candidates(slug);
        let state = self.state.lock().expect("poisoned mutex");
        // Source order within a project is the stored Vec order, which
        // mirrors SqliteIndex's `ORDER BY id`. Active wins over parked
        // if (pathologically) both exist; only one ever should.
        for candidate in &candidates {
            if let Ok(vp) = VaultPath::new(candidate)
                && let Some(entries) = state.milestones.get(&vp)
            {
                return Ok(entries.clone());
            }
        }
        Ok(Vec::new())
    }

    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        let mut out: Vec<(VaultPath, MilestoneEntry)> = state
            .milestones
            .iter()
            .flat_map(|(path, entries)| {
                entries
                    .iter()
                    .filter(|m| m.date.as_deref().is_some_and(|d| d >= from && d <= to))
                    .map(move |m| (path.clone(), m.clone()))
            })
            .collect();
        // Match SqliteIndex's `ORDER BY date`.
        out.sort_by_key(|(_, m)| m.date.clone());
        Ok(out)
    }

    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        let mut state = self.state.lock().expect("poisoned mutex");
        state
            .archival_snapshots
            .insert(path.clone(), snapshot.clone());
        Ok(())
    }

    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError> {
        let state = self.state.lock().expect("poisoned mutex");
        Ok(state.archival_snapshots.get(path).cloned())
    }
}
