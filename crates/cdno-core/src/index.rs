//! SQLite-backed vault index.
//!
//! The index is a cache — filesystem state is authoritative, reconciliation
//! rebuilds the index on any inconsistency. See `SCHEMA.md` for the table
//! shapes and rationale.
//!
//! This module owns connection setup, migrations, the [`VaultIndex`] trait,
//! and a concrete [`SqliteIndex`] implementation.

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
const MIGRATIONS: &[(u32, &str, &str)] = &[(
    1,
    "Initial schema: notes, deadlines, links, note_tags, schema_migrations",
    include_str!("../migrations/001_initial.sql"),
)];

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
        let conn = self.conn.lock().expect("poisoned mutex");
        conn.execute(
            "DELETE FROM notes WHERE path = ?1",
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
