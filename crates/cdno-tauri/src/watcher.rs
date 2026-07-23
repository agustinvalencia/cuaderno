//! The watcher thread: debounced filesystem batches in, reconcile +
//! `vault:changed` emissions out (plan §3.1).
//!
//! Runs on a dedicated `std::thread` — never the blocking pool — and
//! owns the receiving end of the `FsFileWatcher` channel for the
//! process lifetime. Per batch: filter to note-relevant paths, check
//! the `WriteJournal` for self-echoes, run a full `reconcile()`
//! (path-agnostic repair — correctness never depends on event
//! fidelity; a successful `config.toml` rebuild reconciles inside
//! `Vault::new` instead, so the standalone pass is skipped there — #371),
//! classify the surviving paths into areas, and emit.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use arc_swap::ArcSwap;

use cdno_core::config::IgnoreSet;
use cdno_core::error::ConfigError;
use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::VaultStore;
use cdno_core::watcher::FileEvent;
use cdno_domain::error::DomainError;

use crate::commands::config::rebuild_and_swap;
use crate::events::{
    self, CONFIG_STATUS, ConfigHealth, ConfigStatus, Origin, VAULT_CHANGED, VaultArea,
    VaultChanged, WATCHER_STATUS, WatcherStatus,
};
use crate::state::{AppState, WriteJournal};

/// Handles the watcher thread needs, cloned out of the bootstrap
/// before `Vault` swallowed the store/index Arcs.
pub struct WatcherDeps {
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    /// The active ignore matcher, shared by reference with [`AppState`] so a
    /// config reload can swap a fresh set in and the next reconcile honours
    /// the new globs without a restart (GH #365 PR4). Loaded per reconcile
    /// call rather than cached, so a mid-session swap takes effect on the
    /// very next batch.
    pub ignore: Arc<ArcSwap<IgnoreSet>>,
    /// The exclusion counts the #440 notice reads, shared by reference with
    /// [`AppState`]. Every reconcile updates them: the watcher's passes are
    /// reconciliations too, and a bulk move of notes into a folder an
    /// existing glob already matches changes what is in the index without
    /// any config edit to trigger a rebuild.
    pub exclusions: Arc<ArcSwap<crate::events::IndexExclusions>>,
}

/// Consume debounced batches until the sender (the `FsFileWatcher`,
/// owned by the app's setup state) goes away. Spawn via
/// `std::thread::spawn`.
pub fn run(app: AppHandle, deps: WatcherDeps, rx: Receiver<Vec<FileEvent>>) {
    while let Ok(batch) = rx.recv() {
        handle_batch(&app, &deps, batch);
    }
    tracing::debug!("watcher channel closed; watcher thread exiting");
}

/// What a debounced batch means for the frontend. Computed by the
/// pure [`plan_batch`] so the decision table is unit-testable without
/// an `AppHandle`.
#[derive(Debug, PartialEq)]
pub enum BatchPlan {
    /// Only our own echoes (or noise) — reconcile as insurance, emit
    /// no change event (the command already emitted a precise
    /// `origin: self` one).
    Quiet,
    /// Events were dropped somewhere — invalidate everything.
    Rescan,
    /// External edits in these areas.
    External {
        areas: Vec<VaultArea>,
        paths: Vec<String>,
        /// Whether the batch touched `.cuaderno/config.toml` itself — the
        /// trigger for a live vault rebuild (GH #365 PR4). Template `.md`
        /// files under `.cuaderno/templates/` also classify as
        /// `VaultArea::Config` but leave this `false`: they don't change the
        /// note-type registry, so they need only the ordinary refetch.
        config_changed: bool,
    },
}

/// Classify a batch against the self-write journal.
pub fn plan_batch(journal: &WriteJournal, batch: Vec<FileEvent>) -> BatchPlan {
    let mut rescan = false;
    let mut external: Vec<VaultPath> = Vec::new();
    for event in batch {
        match event {
            FileEvent::Rescan => rescan = true,
            FileEvent::Changed(path) | FileEvent::Removed(path) => {
                if !is_note_path(&path) {
                    continue;
                }
                // Self-echo: our own command already emitted a
                // precise origin:self event for this path.
                if journal.is_recent_self_write(&path) {
                    continue;
                }
                external.push(path);
            }
        }
    }

    if rescan {
        return BatchPlan::Rescan;
    }
    let mut areas: Vec<VaultArea> = external.iter().filter_map(events::classify).collect();
    areas.sort();
    areas.dedup();
    if areas.is_empty() {
        return BatchPlan::Quiet;
    }
    // A surviving (non-echo) edit to the config file itself means the
    // note-type registry may have changed — the watcher must rebuild the
    // live vault, not just refetch. Template edits under `.cuaderno/templates/`
    // are excluded (see `is_config_file`).
    let config_changed = external.iter().any(is_config_file);
    BatchPlan::External {
        areas,
        paths: external.iter().map(|p| p.to_string()).collect(),
        config_changed,
    }
}

/// The vault-relative config file the reload watches, distinct from the
/// template files that also classify as `VaultArea::Config` but don't
/// change the note-type registry. Matches the exact `.cuaderno/config.toml`
/// path, not any `config.toml` under `.cuaderno/`, so a stray
/// `.cuaderno/templates/config.toml` could never trigger a spurious rebuild.
fn is_config_file(path: &VaultPath) -> bool {
    path.as_path() == std::path::Path::new(cdno_core::paths::CONFIG_FILE)
}

fn handle_batch(app: &AppHandle, deps: &WatcherDeps, batch: Vec<FileEvent>) {
    let state = app.state::<AppState>();
    let plan = plan_batch(&state.journal, batch);

    // Index health is reported on EVERY batch — the degraded pill is the
    // frontend's poll-fallback trigger, so a reconcile failure must never stay
    // invisible (once write commands land, self-echo is the common batch
    // shape). WHERE that reconcile runs depends on the plan:
    //
    // - A config rebuild reconciles inside `rebuild_and_swap` (against the NEW
    //   ignore set) on its way to success, and that pass is path-agnostic: it
    //   already folds any note edits riding in the same batch. Running the
    //   standalone reconcile here as well would be a wasted second pass (#371),
    //   so that branch owns its own reconcile + health emit — and only needs
    //   the standalone reconcile where the rebuild FAILS and the old vault
    //   stays live (see `handle_external_config_edit`).
    // - Every other plan (including a pure self-echo) runs the standalone
    //   `run_reconcile` — path-agnostic repair, correctness never depends on
    //   event fidelity — and reports its health here.
    match plan {
        BatchPlan::External {
            areas,
            paths,
            config_changed: true,
        } => handle_external_config_edit(app, deps, areas, paths),
        other => {
            let outcome = run_reconcile(deps);
            emit_watcher_status(app, outcome.ok);
            match other {
                // A quiet batch normally tells the frontend nothing. But if
                // this pass changed what is excluded from the index, the
                // exclusion notice has to hear about it — and the batch that
                // causes it (notes moved into an ignored folder) is often
                // exactly the one that classifies to no area at all.
                BatchPlan::Quiet => {
                    if outcome.exclusions_changed {
                        let _ = app.emit(
                            VAULT_CHANGED,
                            VaultChanged {
                                origin: Origin::External,
                                areas: Vec::new(),
                                paths: Vec::new(),
                            },
                        );
                    }
                }
                BatchPlan::Rescan => {
                    let _ = app.emit(
                        VAULT_CHANGED,
                        VaultChanged {
                            origin: Origin::External,
                            areas: all_areas(),
                            paths: Vec::new(),
                        },
                    );
                }
                // Only `config_changed: false` reaches here; the `true` case is
                // handled above.
                BatchPlan::External { areas, paths, .. } => {
                    let _ = app.emit(
                        VAULT_CHANGED,
                        VaultChanged {
                            origin: Origin::External,
                            areas,
                            paths,
                        },
                    );
                }
            }
        }
    }
}

/// Emit the index-health pill (`ok` / `degraded`) — the frontend's
/// poll-fallback trigger, reported on every batch (§3.1).
fn emit_watcher_status(app: &AppHandle, ok: bool) {
    let _ = app.emit(
        WATCHER_STATUS,
        WatcherStatus {
            state: if ok { "ok" } else { "degraded" },
        },
    );
}

/// A courtesy yield before the single retry of a transient config reload
/// (#372). This is not the whole grace period: the retry itself calls
/// `Vault::new` → `reconcile`, which re-acquires the write lock with a
/// fresh 5s budget, so the effective second chance is this delay plus that
/// full lock wait. The yield just avoids hammering the lock the instant the
/// first attempt gave up; the lock-holder has almost always finished within
/// the retry's own wait.
const RELOAD_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// Does a failed config rebuild mean the on-disk config is genuinely
/// *invalid* — bad TOML, a bad ignore glob, a rejected note-type or
/// schema — as opposed to a transient/operational failure (the vault write
/// lock was momentarily held during reconcile, or an IO/index hiccup)?
///
/// Only the former should tell the user their `config.toml` is broken; a
/// transient failure keeps the last good config and must never be
/// mislabelled as invalid (#372).
///
/// Classified as a **transient allowlist** rather than an invalid denylist,
/// and deliberately so: misclassifying a transient error as invalid at
/// worst flashes a visible, self-correcting banner, whereas misclassifying
/// an *invalid* config as transient silently swallows a real error with no
/// feedback and no self-heal. So an error we don't positively recognise as
/// operational defaults to *invalid*. `Vault::new` reaches here through
/// three failure sources — `ignore_set()` (a bad glob → `Config`),
/// `TypeRegistry::validate` (a shadowed built-in → `ReservedTypeName`, a
/// reserved schema key → `ReservedSchemaField`, other type/schema faults →
/// `Config`), and `reconcile` (a write-lock timeout, wrapped as `Index`) —
/// so the only genuinely transient outcomes are index/store/transaction
/// failures and a raw config read that caught the file mid-rename.
#[doc(hidden)]
pub fn is_invalid_config_error(err: &DomainError) -> bool {
    !matches!(
        err,
        DomainError::Index(_)
            | DomainError::Store(_)
            | DomainError::Transaction(_)
            | DomainError::Config(ConfigError::Read { .. })
    )
}

/// Apply an external `.cuaderno/config.toml` edit: rebuild the live vault
/// (and its ignore set) from the new config, then tell the frontend what
/// happened (GH #365 PR4, revised #372).
///
/// On a successful rebuild, one all-areas `vault:changed` refetches
/// everything the new config might have reshaped (note types, schemas,
/// folders), and a `config:status` valid clears any prior error banner.
///
/// A rebuild failure splits three ways, because [`rebuild_and_swap`] has
/// already guaranteed the OLD vault stays live (never-brick):
///
/// - **Genuinely invalid config** (bad TOML/glob/schema): surface the error
///   as a non-red `config:status` banner, and still emit `vault:changed`
///   for the batch's non-config edits — a broken config edit bundled with a
///   real note edit must not swallow the note edit.
/// - **Transient contention** (the write lock was held, or an IO/index
///   hiccup): the config itself may be fine. Retry once after a short
///   backoff — the lock-holder has almost always finished by then.
/// - **Still blocked after the retry**: keep the last good config *without*
///   a false "invalid" banner. Emit a distinct `Deferred` `config:status`
///   so the UI shows a calm "vault was busy — the change will apply on the
///   next edit" note (#384), refresh the batch's non-config edits, and let
///   the change apply on the next config edit's reconcile.
///
/// This path also owns the batch's index-health emit (#371): the reconcile-vs-not
/// decision is classified by [`classify_rebuild`] and settled by
/// [`emit_config_reload_outcome`] — an applied rebuild reports `ok` directly
/// (its own reconcile covered the batch); every failure runs the standalone
/// reconcile the success path skips and reports *that* health. Either way
/// `watcher:status` fires exactly once per batch.
fn handle_external_config_edit(
    app: &AppHandle,
    deps: &WatcherDeps,
    areas: Vec<VaultArea>,
    paths: Vec<String>,
) {
    let first = rebuild_and_swap(app);
    if classify_rebuild(&first) != RebuildAttempt::Transient {
        // Applied or genuinely invalid — settled on the first attempt.
        emit_config_reload_outcome(app, deps, first, areas, paths);
        return;
    }
    // Transient: the config may be perfectly valid — we just could not apply
    // it this instant. A single quick retry after a short backoff catches the
    // common brief-contention case; the retry's outcome (applied, invalid, or
    // still-blocked → deferred) is authoritative.
    tracing::debug!(error = ?first.as_ref().err(), "config reload deferred (vault busy); retrying once");
    std::thread::sleep(RELOAD_RETRY_BACKOFF);
    emit_config_reload_outcome(app, deps, rebuild_and_swap(app), areas, paths);
}

/// How a single [`rebuild_and_swap`] attempt landed, classified from its
/// result — the pure core of the config-reload decision, split out so the
/// reconcile-vs-not choice (#371) is unit-testable without an `AppHandle`,
/// exactly as [`plan_batch`] and [`is_invalid_config_error`] are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum RebuildAttempt {
    /// The rebuild applied. Its own reconcile (against the NEW ignore set) is
    /// the batch's single reconcile pass and has already folded any note edits
    /// riding in the same batch.
    Applied,
    /// The config is genuinely invalid (bad TOML/glob/schema). The rebuild
    /// bailed *before* any reconcile ran — a parse fault in `VaultConfig::load`,
    /// or `ignore_set`/`validate` in `Vault::new` — so the old vault stays live
    /// and no reconcile touched the index.
    Invalid,
    /// A transient failure — the vault write lock was momentarily held, or an
    /// IO/index hiccup. The config may be fine; the old vault stays live.
    Transient,
}

impl RebuildAttempt {
    /// Whether the watcher must run its standalone reconcile for this outcome
    /// (#371). Only an applied rebuild reconciled the batch itself (via
    /// `Vault::new` against the new ignore set); every failure kept the OLD
    /// vault live with its reconcile un-run or incomplete, so the batch's
    /// non-config note edits still need folding into the index against the
    /// still-live old ignore set.
    #[doc(hidden)]
    pub fn needs_standalone_reconcile(self) -> bool {
        !matches!(self, RebuildAttempt::Applied)
    }
}

/// Classify a [`rebuild_and_swap`] result: `Ok` applied; an `Err` splits by
/// [`is_invalid_config_error`] into a genuinely invalid config vs a transient
/// failure worth one retry (#372).
#[doc(hidden)]
pub fn classify_rebuild(result: &Result<(), DomainError>) -> RebuildAttempt {
    match result {
        Ok(()) => RebuildAttempt::Applied,
        Err(e) if is_invalid_config_error(e) => RebuildAttempt::Invalid,
        Err(_) => RebuildAttempt::Transient,
    }
}

/// Emit the terminal events for a settled config-reload attempt: the index
/// health pill (§3.1), then the matching config banner. Owns the batch's
/// reconcile-vs-not decision (#371) via
/// [`RebuildAttempt::needs_standalone_reconcile`] — an applied rebuild
/// reconciled the batch itself, so it reports `ok` with no standalone pass;
/// every failure keeps the old vault live and runs the standalone reconcile to
/// fold the batch's note edits, reporting THAT health. So `watcher:status`
/// fires exactly once here. `result` must be a *terminal* attempt: a transient
/// one only ever reaches here post-retry (the "still blocked" → deferred case).
fn emit_config_reload_outcome(
    app: &AppHandle,
    deps: &WatcherDeps,
    result: Result<(), DomainError>,
    areas: Vec<VaultArea>,
    paths: Vec<String>,
) {
    let attempt = classify_rebuild(&result);
    // A failed rebuild leaves the old vault live, so the standalone pass is
    // what repairs the index — and its exclusion counts are the ones that
    // now describe it. A successful rebuild already stored its own inside
    // `rebuild_and_swap`.
    let ok = if attempt.needs_standalone_reconcile() {
        run_reconcile(deps).ok
    } else {
        true
    };
    emit_watcher_status(app, ok);
    match (attempt, result) {
        (RebuildAttempt::Applied, _) => emit_config_reloaded(app, paths),
        (RebuildAttempt::Invalid, Err(e)) => emit_config_invalid(app, e, areas, paths),
        (RebuildAttempt::Transient, _) => {
            // Reachable only post-retry: still blocked. Keep the last good
            // config without a false "invalid" banner — the deferred banner
            // says "vault was busy; it'll apply on the next edit" (#384).
            tracing::warn!(
                "config reload still blocked; keeping the last good config, will apply on the next config edit"
            );
            emit_config_deferred(app, areas, paths);
        }
        // Unreachable: `Invalid` classifies only an `Err`. Fall back to the
        // calm deferred banner rather than panic if that invariant ever breaks.
        (RebuildAttempt::Invalid, Ok(())) => emit_config_deferred(app, areas, paths),
    }
}

/// Signal an applied config reload: refetch every area the new config might
/// reshape, and clear any prior notice.
fn emit_config_reloaded(app: &AppHandle, paths: Vec<String>) {
    let _ = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            origin: Origin::External,
            areas: all_areas(),
            paths,
        },
    );
    let _ = app.emit(
        CONFIG_STATUS,
        ConfigStatus {
            health: ConfigHealth::Valid,
            message: None,
        },
    );
}

/// Signal a genuinely invalid config: the non-red banner carrying the open
/// error, plus a `vault:changed` for the batch's non-config edits.
fn emit_config_invalid(
    app: &AppHandle,
    err: DomainError,
    areas: Vec<VaultArea>,
    paths: Vec<String>,
) {
    tracing::warn!(error = %err, "external config edit is invalid; keeping the last good config");
    // The open error is actionable ("expected `=`", a rejected type/schema),
    // so surface it as the banner's detail line.
    emit_config_status_with_edits(
        app,
        ConfigHealth::Invalid,
        Some(err.to_string()),
        areas,
        paths,
    );
}

/// Signal a transiently-deferred reload (#384): the config may be fine but a
/// busy vault kept it from applying. A calm, distinct banner rather than the
/// "invalid config" one; the batch's non-config edits still refresh.
fn emit_config_deferred(app: &AppHandle, areas: Vec<VaultArea>, paths: Vec<String>) {
    // No detail line: the transient error is a raw, non-actionable string
    // (a lock timeout / index message) that would undercut the calm "vault
    // was busy" framing. The lead sentence says all the user needs (#384).
    emit_config_status_with_edits(app, ConfigHealth::Deferred, None, areas, paths);
}

/// Emit a non-`Valid` `config:status` (with an optional detail message), then
/// a `vault:changed` for the batch's non-config edits — the shared shape of
/// the invalid and deferred paths.
fn emit_config_status_with_edits(
    app: &AppHandle,
    health: ConfigHealth,
    message: Option<String>,
    areas: Vec<VaultArea>,
    paths: Vec<String>,
) {
    let _ = app.emit(CONFIG_STATUS, ConfigStatus { health, message });
    let _ = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            origin: Origin::External,
            areas,
            paths,
        },
    );
}

/// What a reconcile pass did, beyond repairing the index.
#[derive(Debug, Clone, Copy)]
pub struct ReconcileOutcome {
    /// Index health for the degraded pill.
    pub ok: bool,
    /// Whether what is excluded from the index moved. Worth an event even
    /// when the batch itself was quiet: a note moved into a folder an
    /// `ignore` glob matches leaves the index without touching any area the
    /// frontend subscribes to (#440).
    pub exclusions_changed: bool,
}

/// Repair the index. `ok` is `false` (→ degraded pill + poll fallback in
/// the frontend) only on catastrophic failure, not per-file errors;
/// `exclusions_changed` says whether this pass altered what is excluded
/// from the index, which the caller needs because that change is worth
/// telling the frontend about even when nothing else in the batch was.
///
/// Public so a test can drive the exact pass the watcher thread runs,
/// without an `AppHandle` or a real filesystem watcher.
pub fn run_reconcile(deps: &WatcherDeps) -> ReconcileOutcome {
    // Load the current ignore set per call so a config reload's swap
    // (GH #365 PR4) is honoured on the very next reconcile.
    let ignore = deps.ignore.load_full();
    match reconcile(&deps.store, &deps.index, &ignore) {
        Ok(report) => {
            if !report.errors.is_empty() {
                tracing::warn!(
                    errors = report.errors.len(),
                    "reconcile completed with per-file errors"
                );
            }
            // This pass is what the index now reflects, so the notice must
            // read from it (#440). Without this the counts would only ever
            // follow a config rebuild, and moving 200 notes under a folder
            // an existing glob matches would evict them from search, lint
            // and backlinks with nothing said.
            //
            // Unless the globs moved underneath us. A config rebuild can swap
            // a fresh `IgnoreSet` in while this pass is still walking, and
            // these counts were computed against the set loaded at entry — so
            // publishing them would overwrite the rebuild's correct numbers
            // with ones describing globs that are no longer in force, and the
            // emit below would then push that stale value at the user. The
            // rebuild's own counts are authoritative; drop ours. (The index
            // itself can still be left mixed by the interleaving — that is
            // #459, a pre-existing race this only declines to make visible.)
            if !Arc::ptr_eq(&ignore, &deps.ignore.load_full()) {
                tracing::debug!("ignore set swapped mid-reconcile; keeping the rebuild's counts");
                return ReconcileOutcome {
                    ok: true,
                    exclusions_changed: false,
                };
            }
            let fresh = crate::events::IndexExclusions::from_report(&report);
            let previous = **deps.exclusions.load();
            deps.exclusions.store(Arc::new(fresh));
            ReconcileOutcome {
                ok: true,
                exclusions_changed: fresh != previous,
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "watcher reconcile failed");
            ReconcileOutcome {
                ok: false,
                exclusions_changed: false,
            }
        }
    }
}

/// Paths the UI could care about: markdown notes outside `.cuaderno/`
/// (plus the config file and the template files, which drive the log
/// form's dynamic fields). The `.cdno-wip-*` atomic-write temp files
/// carry no `.md` extension, so this also drops our own write staging.
fn is_note_path(path: &VaultPath) -> bool {
    let p = path.as_path();
    if p.starts_with(cdno_core::paths::CUADERNO_DIR) {
        // Template edits change which fields the log form gathers, so a
        // `.cuaderno/templates/*.md` touch must survive filtering; the
        // config file matters too. Everything else under `.cuaderno`
        // (index db, lock file) is churn no view renders.
        if p.starts_with(cdno_core::paths::TEMPLATES_DIR) {
            return p.extension().and_then(|e| e.to_str()) == Some("md");
        }
        return p.file_name().and_then(|f| f.to_str()) == Some("config.toml");
    }
    p.extension().and_then(|e| e.to_str()) == Some("md")
}

/// Every area the frontend renders — the invalidate-everything set,
/// shared by the watcher's `Rescan` plan and the config-reload command
/// (GH #365), which both mean "refetch the whole UI".
pub(crate) fn all_areas() -> Vec<VaultArea> {
    vec![
        VaultArea::Projects,
        VaultArea::Actions,
        VaultArea::Daily,
        VaultArea::Weekly,
        VaultArea::Monthly,
        VaultArea::Commitments,
        VaultArea::Portfolios,
        VaultArea::Stewardships,
        VaultArea::Questions,
        VaultArea::Inbox,
        VaultArea::Config,
    ]
}
