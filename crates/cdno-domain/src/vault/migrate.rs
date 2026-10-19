//! The method-migration runner (RFC 0006 §8.4, plan T8): move a vault to the
//! method version this binary implements, running any file migrations and
//! stamping `[vault] method_version` last.
//!
//! # The list
//!
//! [`FILE_MIGRATIONS`] mirrors the index's `MIGRATIONS` in
//! `cdno-core/src/index.rs`: an ordered list, append-only, and a shipped
//! entry is never edited. Each entry is keyed by the **major** version it
//! migrates to and nothing finer: under the method's own test (§7.4) a minor
//! version leaves every existing page readable as it is, so a minor has no
//! file step, and a non-empty step means a major. The list is empty for RLM
//! 1.1, whose only file change is the stamp.
//!
//! # The rules every step obeys
//!
//! - **Idempotent.** A step is a pure function of the files as they are now:
//!   it reads through [`MigrationPlan::read`] and records whole-file writes
//!   with [`MigrationPlan::write`], and a file it has already migrated plans
//!   nothing. The runner drops a planned write identical to the file on disk
//!   as well, so a rerun after a partial migration completes it rather than
//!   repeating it.
//! - **Never the journal, never an append-only note.** A planned write under
//!   `journal/` (monthly notes included, which are mutable but journal), to a
//!   note of an append-only type (daily, weekly, evidence, tracking, and a
//!   custom type declared `append_only`), or to the vault's own `.cuaderno/`
//!   files is refused before anything is staged. The type is read from the
//!   frontmatter and the index, so the fence by folder covers only
//!   `journal/` and `.cuaderno/`: a new file with no `type:` placed in an
//!   append-only type's folder (a portfolio's evidence, a stewardship's
//!   `tracking/`) is not caught. The journal is the record of
//!   what happened, in the shapes it was written in, so the readers keep
//!   accepting every historical line shape for ever instead.
//!
//! # The run
//!
//! One transaction, taken with `Vault::transaction_ungated`, under the
//! write lock and outside the method-version gate (a vault on an older major
//! is exactly what the gate refuses and the runner exists for). The version
//! is read under that lock, as the gate reads it. Staged in this order: each
//! step's file writes and index upserts; then the `vault migrated` log line
//! through `stage_daily_log`; then, **last**, the stamp.
//!
//! **Why the stamp is last.** A failed commit rolls back what it applied, so
//! the order matters little when nothing crashes; but the transaction is not
//! crash-safe by design. With the stamp last, a crash or a failed rollback
//! leaves the stamp old, and a rerun re-plans (a migrated file plans nothing)
//! and completes. With the stamp first, the vault would claim a version its
//! files never reached, and the rerun would find nothing to do: silent and
//! permanent. One transaction for the whole run, rather than one per step,
//! so the files and the version commit or roll back together. The cost is
//! accepted: a crash after the log line lands but before the stamp leaves a
//! `vault migrated` entry with no stamp, and the rerun writes a second one.
//! The log stays true about what was attempted; the stamp is what the gate
//! reads.
//!
//! The stamp is written through the transaction with the comment-preserving
//! `toml_edit` setter [`set_method_version`] and checked by
//! [`validate_config_str`], never through `save_config_raw` or
//! `save_config_to`. Those (the `cdno config` verbs) write without the lock,
//! so just before staging the stamp the runner re-reads the config and
//! refuses if it changed since the run read it. A window remains between
//! that check and the commit's write, in which an unlocked config write would
//! be overwritten; it is as narrow as the commit's own file writes, and the
//! same accepted single-user window `save_config_to`'s compare-and-swap has.
//!
//! CLI-only (`cdno migrate`, T9): never run at open, never over MCP.

use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::config::{MethodVersion, method_version_from_str};
use cdno_core::config_edit::set_method_version;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;
use cdno_core::paths::{CONFIG_FILE, CUADERNO_DIR, JOURNAL};
use cdno_core::transaction::VaultTransaction;

use super::Vault;
use super::commitments::CommitmentEntry;
use super::config::{ConfigDocument, read_config_from, validate_config_str};
use super::index_entry::build_index_entry_for;
use super::parked_dates::READ_THROUGH_BELOW;
use crate::error::DomainError;
use crate::method_version::{MethodSkew, UNSTAMPED_READS_AS};
use crate::type_registry::NoteTypeDescriptor;

/// The head of the daily-log entry a migration writes, followed by indented
/// `was:` / `now:` lines: `was: unstamped (read as 1.0)` for a vault that
/// recorded no version, `was: <version>` otherwise. In the `was:`/`now:`
/// family with `state on`, not a focus marker. No wikilink: there is no note
/// for the vault to link to (decision D1).
pub const VAULT_MIGRATED_PREFIX: &str = "vault migrated";

/// One file migration: what it migrates to, and how it plans its writes.
#[derive(Debug, Clone, Copy)]
pub struct FileMigration {
    /// The major method version this migration moves a vault to. It runs for
    /// a vault below this major when the binary is at or above it.
    pub to_major: u16,
    /// One line saying what the migration changes, for `cdno migrate
    /// --dry-run`.
    pub description: &'static str,
    /// Record the migration's writes on the plan, reading the vault as it is
    /// now. Must be idempotent and must not plan a journal, append-only or
    /// `.cuaderno/` write (the runner refuses those). Runs under the write
    /// lock in a real run, and without it in a dry run.
    pub plan: fn(&Vault, &mut MigrationPlan) -> Result<(), DomainError>,
}

/// Every file migration, in order. Append-only: a shipped entry is never
/// edited, as with the index's `MIGRATIONS`. Empty for RLM 1.1.
pub const FILE_MIGRATIONS: &[FileMigration] = &[];

/// The writes the file migrations of one run plan, kept apart from the
/// transaction because a `VaultTransaction` exposes nothing it has buffered:
/// the runner fences, filters and reports them before anything is staged.
#[derive(Debug, Default)]
pub struct MigrationPlan {
    writes: Vec<PlannedWrite>,
    /// The index, in the run's list of steps, of the step now planning.
    step: usize,
}

#[derive(Debug)]
struct PlannedWrite {
    path: VaultPath,
    content: String,
    /// The step that wrote `content` last.
    step: usize,
}

impl MigrationPlan {
    /// Plan a whole-file write of `content` to `path`. A later write to the
    /// same path replaces an earlier one, whichever step planned it.
    pub fn write(&mut self, path: VaultPath, content: impl Into<String>) {
        let content = content.into();
        let step = self.step;
        match self.writes.iter_mut().find(|w| w.path == path) {
            Some(planned) => {
                planned.content = content;
                planned.step = step;
            }
            None => self.writes.push(PlannedWrite {
                path,
                content,
                step,
            }),
        }
    }

    /// The content `path` will have if the plan so far is applied: the
    /// planned write when an earlier step (or this one) planned one, else the
    /// file on disk; `None` when there is neither. A step reads through this,
    /// so it sees the steps before it.
    pub fn read(&self, vault: &Vault, path: &VaultPath) -> Result<Option<String>, DomainError> {
        if let Some(planned) = self.writes.iter().find(|w| w.path == *path) {
            return Ok(Some(planned.content.clone()));
        }
        vault.read_if_exists(path)
    }
}

/// One file migration in a report: which, and the files it changes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MigrationStep {
    pub to_major: u16,
    pub description: &'static str,
    /// The files this step changes, in the order it planned them; empty when
    /// every file it covers is already migrated.
    pub writes: Vec<VaultPath>,
}

/// What a migration does, or did: returned by [`Vault::plan_migration`] (the
/// dry run) and [`Vault::migrate`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MigrationReport {
    /// The vault's recorded version when the run read it; `None` when
    /// unstamped (read as 1.0).
    pub from: Option<MethodVersion>,
    /// The version the run stamps: the binary's.
    pub to: MethodVersion,
    /// The file migrations that apply, in order; empty for a vault already
    /// on the binary's major.
    pub steps: Vec<MigrationStep>,
    /// The dates a minor version asks to be read through once: every hard
    /// date on a parked map, overdue first ([`Vault::parked_live_dates`],
    /// RFC 0006 §8.1, plan T12). Filled for a run that stamps a vault whose
    /// effective version is below [`READ_THROUGH_BELOW`]; empty otherwise,
    /// since a vault at RLM 1.1 or later has had its read-through.
    pub read_through: Vec<CommitmentEntry>,
}

impl MigrationReport {
    /// Whether the run stamps the vault: whenever it is not stamped with
    /// exactly the binary's version. An unstamped vault and one stamped 1.0
    /// are different states, so an unstamped vault is stamped even when the
    /// binary is at 1.0. `false` is a no-op: no write, no stamp, no line.
    pub fn stamps(&self) -> bool {
        self.from != Some(self.to)
    }

    /// The `[vault]` line the stamp writes, when the run stamps.
    pub fn stamp_line(&self) -> Option<String> {
        self.stamps()
            .then(|| format!("method_version = \"{}\"", self.to))
    }

    /// The daily-log entry the run writes, when it stamps:
    /// `vault migrated` with indented `was:` / `now:` lines.
    pub fn log_entry(&self) -> Option<String> {
        self.stamps().then(|| {
            let was = match self.from {
                Some(version) => version.to_string(),
                None => format!("unstamped (read as {UNSTAMPED_READS_AS})"),
            };
            format!("{VAULT_MIGRATED_PREFIX}\n  was: {was}\n  now: {}", self.to)
        })
    }
}

/// A run's plan before anything is staged.
struct Prepared {
    report: MigrationReport,
    /// The config as the run read it: the base the stamp is built from, and
    /// the hash the re-read before the stamp is compared with.
    config: ConfigDocument,
    /// Every planned write that changes a file, fenced, in planned order.
    writes: Vec<(VaultPath, String)>,
}

impl Vault {
    /// The dry run: what [`Vault::migrate`] would do with `steps` and a binary
    /// at `binary`, the file migrations, their writes and the stamp, without
    /// writing anything or taking the lock. Refuses as the real run would.
    /// `today` dates the read-through's overdue flags.
    ///
    /// `binary` and `steps` are parameters so tests inject a synthetic step
    /// and version; the CLI passes `METHOD_VERSION` and [`FILE_MIGRATIONS`].
    pub fn plan_migration(
        &self,
        today: NaiveDate,
        binary: MethodVersion,
        steps: &[FileMigration],
    ) -> Result<MigrationReport, DomainError> {
        Ok(self.prepare_migration(today, binary, steps)?.report)
    }

    /// Move the vault to `binary`: run the file migrations in `steps` that
    /// apply, log `vault migrated` at `at`, and stamp
    /// `[vault] method_version` last, all in one transaction under the write
    /// lock (see the module documentation for the order and why).
    ///
    /// A vault stamped with exactly `binary` is a no-op: nothing is written.
    /// A vault on a newer version, minor included, is
    /// [`DomainError::MethodVersionAhead`]. A step that plans a protected
    /// write is [`DomainError::MigrationTouchesProtectedPath`], a config that
    /// changed during the run [`DomainError::ConfigChangedDuringMigration`],
    /// and a stamped config that would not open
    /// [`DomainError::MigrationStampInvalid`]; each refuses before anything
    /// is written.
    pub fn migrate(
        &self,
        at: NaiveDateTime,
        binary: MethodVersion,
        steps: &[FileMigration],
    ) -> Result<MigrationReport, DomainError> {
        // The lock first, then every read: the version, the config the stamp
        // is built from, and the files the steps plan against.
        let mut tx = self.transaction_ungated()?;
        let prepared = self.prepare_migration(at.date(), binary, steps)?;
        let Some(entry) = prepared.report.log_entry() else {
            // Already at the binary's version: nothing to do, and the
            // transaction drops with nothing buffered.
            return Ok(prepared.report);
        };

        // 1. Each step's writes, with their index rows.
        for (path, content) in &prepared.writes {
            tx.write_file(path.clone(), content.clone());
            if let Some(note_type) = markdown_note_type(path, content) {
                tx.upsert_note(build_index_entry_for(path, content, &note_type)?);
            }
        }

        // 2. The log line. The fence keeps every step out of the journal, so
        //    the daily note this reads back is the one on disk.
        self.stage_daily_log(at, &entry, &mut tx)?;

        // 3. Last, the stamp, built from the config the run read, provided it
        //    is still the config on disk: the `cdno config` verbs write
        //    without the lock, and stamping the older text would overwrite
        //    their edit. The window left between this re-read and the
        //    commit's write is named in the module documentation.
        let current = read_config_from(&*self.store)?;
        if current.hash != prepared.config.hash {
            return Err(DomainError::ConfigChangedDuringMigration);
        }
        let stamped = set_method_version(&prepared.config.content, binary).map_err(|err| {
            DomainError::MigrationStampInvalid {
                reason: err.to_string(),
            }
        })?;
        validate_config_str(&stamped).map_err(|err| DomainError::MigrationStampInvalid {
            reason: err.message,
        })?;
        tx.write_file(VaultPath::new(CONFIG_FILE)?, stamped);

        tx.commit()?;
        Ok(prepared.report)
    }

    /// A transaction that takes the vault write lock, exactly as
    /// `Vault::transaction` does, but skips the method-version gate.
    ///
    /// Private to this module, so the runner is its only caller by
    /// construction. The runner has to write to the vaults the gate refuses
    /// (one on an older major, which `cdno migrate` is the one way across)
    /// and checks the version itself, re-read under this same lock; every
    /// other write goes through the gated `Vault::transaction`.
    fn transaction_ungated(&self) -> Result<VaultTransaction, DomainError> {
        Ok(VaultTransaction::new(
            Arc::clone(&self.store),
            Arc::clone(&self.index),
        )?)
    }

    /// Read the version and the config, decide whether the run stamps, and
    /// plan, fence and filter the applicable steps' writes. Writes nothing.
    fn prepare_migration(
        &self,
        today: NaiveDate,
        binary: MethodVersion,
        steps: &[FileMigration],
    ) -> Result<Prepared, DomainError> {
        let config = read_config_from(&*self.store)?;
        let from = method_version_from_str(&config.content)?;
        if matches!(
            MethodSkew::between(binary, from),
            MethodSkew::NewerMajor | MethodSkew::NewerMinor
        ) {
            return Err(DomainError::MethodVersionAhead {
                vault: from.unwrap_or(UNSTAMPED_READS_AS),
                binary,
            });
        }

        let mut report = MigrationReport {
            from,
            to: binary,
            steps: Vec::new(),
            read_through: Vec::new(),
        };
        if !report.stamps() {
            return Ok(Prepared {
                report,
                config,
                writes: Vec::new(),
            });
        }
        if from.unwrap_or(UNSTAMPED_READS_AS) < READ_THROUGH_BELOW {
            report.read_through = self.parked_live_dates(today)?;
        }

        // The steps above the vault's major, up to the binary's, in order.
        let from_major = from.unwrap_or(UNSTAMPED_READS_AS).major;
        let applicable: Vec<&FileMigration> = steps
            .iter()
            .filter(|step| step.to_major > from_major && step.to_major <= binary.major)
            .collect();

        let mut plan = MigrationPlan::default();
        for (index, step) in applicable.iter().enumerate() {
            plan.step = index;
            (step.plan)(self, &mut plan)?;
        }

        // Fence every planned write, changed or not, before anything is
        // staged: a step that plans a journal write is wrong even when the
        // write would change nothing.
        for planned in &plan.writes {
            if let Some(reason) = self.protected_reason(&planned.path, &planned.content)? {
                return Err(DomainError::MigrationTouchesProtectedPath {
                    step: applicable[planned.step].description.to_owned(),
                    path: planned.path.to_string(),
                    reason,
                });
            }
        }

        report.steps = applicable
            .iter()
            .map(|step| MigrationStep {
                to_major: step.to_major,
                description: step.description,
                writes: Vec::new(),
            })
            .collect();
        let mut writes = Vec::new();
        for planned in plan.writes {
            // A file already in its migrated form plans nothing.
            if self.read_if_exists(&planned.path)?.as_deref() == Some(planned.content.as_str()) {
                continue;
            }
            report.steps[planned.step].writes.push(planned.path.clone());
            writes.push((planned.path, planned.content));
        }

        Ok(Prepared {
            report,
            config,
            writes,
        })
    }

    /// Why a migration may not write `content` to `path`, or `None` when it
    /// may. The note's type is taken from every source that names one (the
    /// file on disk, its index row, and the planned content), so a step can
    /// neither rewrite an append-only note nor turn a note into one.
    fn protected_reason(
        &self,
        path: &VaultPath,
        content: &str,
    ) -> Result<Option<String>, DomainError> {
        if path.as_path().starts_with(JOURNAL) {
            return Ok(Some("the journal is append-only".to_owned()));
        }
        if path.as_path().starts_with(CUADERNO_DIR) {
            return Ok(Some(
                "the vault's own files; the runner writes the stamp itself".to_owned(),
            ));
        }
        let mut types: Vec<String> = Vec::new();
        if let Some(on_disk) = self.read_if_exists(path)? {
            types.extend(frontmatter_type(&on_disk));
        }
        if let Some(row) = self.index.find_by_path(path)? {
            types.push(row.note_type);
        }
        types.extend(frontmatter_type(content));
        let registry = self.type_registry();
        for note_type in types {
            let append_only = match registry.resolve(&note_type) {
                Some(NoteTypeDescriptor::Builtin(builtin)) => builtin.is_append_only(),
                Some(NoteTypeDescriptor::Custom { def, .. }) => def.append_only,
                None => false,
            };
            if append_only {
                return Ok(Some(format!("a {note_type} note is append-only")));
            }
        }
        Ok(None)
    }

    /// The file at `path`, or `None` when there is none.
    fn read_if_exists(&self, path: &VaultPath) -> Result<Option<String>, DomainError> {
        match self.store.read_file(path) {
            Ok(content) => Ok(Some(content)),
            Err(StoreError::NotFound(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

/// The `type:` a note's frontmatter declares, if it parses and has one.
fn frontmatter_type(content: &str) -> Option<String> {
    let (frontmatter, _body) = Frontmatter::parse(content).ok()?;
    frontmatter.optional_field::<String>("type").ok().flatten()
}

/// The note type to index a planned write under: a markdown file whose
/// frontmatter declares one. Anything else gets no index row here, and the
/// next reconciliation indexes it as it would any file.
fn markdown_note_type(path: &VaultPath, content: &str) -> Option<String> {
    let is_markdown = path.as_path().extension() == Some(std::ffi::OsStr::new("md"));
    if is_markdown {
        frontmatter_type(content)
    } else {
        None
    }
}
