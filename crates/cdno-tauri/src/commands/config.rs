//! Config inspector (#365, PR1) + live reload (#365, PR2) + raw editor
//! save (#365, PR3).
//!
//! `read_config`/`validate_config` are pure reads — no journal, no
//! events — mirroring the Templates read posture. `read_config` hands the
//! UI the verbatim file plus a content hash that PR3's `save_config`
//! echoes back for its compare-and-swap. `validate_config` runs the exact
//! composition
//! `Vault::new` performs (`toml::from_str` → `ignore_set` →
//! `TypeRegistry::validate`) against a candidate string, reporting
//! `Ok(())` or a structured `{ message, line?, col? }` — the same error
//! type the PR3 save gate reuses, so the dry-run and the real gate cannot
//! drift.
//!
//! `reload_config` is PR2's reload PLUMBING: it re-reads config.toml from
//! disk, rebuilds the `Vault` on the SAME store/index, and atomically
//! swaps it into `AppState`. No config WRITE happens here (that is PR3);
//! this command exists so a later save can apply a config edit live, and
//! so the swap is manually testable now.

use std::sync::Arc;

use cdno_core::config::{
    CustomNoteType, FieldSpec, IgnoreSet, SchemaExtension, VaultConfig, VaultMeta,
};
use cdno_core::config_edit;
use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;
use cdno_core::paths::CONFIG_FILE;
use cdno_core::store::VaultStore;
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::{
    ConfigDocument, ConfigSaveError, ConfigValidationError, validate_config_str,
};
use tauri::{Emitter, Manager};

use crate::commands::actions::record_and_emit;
use crate::error::CmdError;
use crate::events::{Origin, VAULT_CHANGED, VaultArea, VaultChanged};
use crate::state::AppState;
use crate::watcher::all_areas;
use crate::with_vault::with_vault;

/// Read the raw config document (content + hash). Public and
/// synchronous — the test seam, exercised directly over the Memory
/// doubles.
pub fn read_config_impl(vault: &Vault) -> Result<ConfigDocument, CmdError> {
    Ok(vault.read_config_raw()?)
}

/// The raw `.cuaderno/config.toml` text plus its content hash — the
/// inspector's read. A pure read: no journal, no emit.
#[tauri::command]
pub async fn read_config(state: tauri::State<'_, AppState>) -> Result<ConfigDocument, CmdError> {
    with_vault(&state.vault(), read_config_impl).await?
}

/// A serialisable projection of the parsed config for the structured
/// Config view (#365, PR5a) — the vault meta, the `[note_types.*]`
/// table, and the `[schemas.*]` table, each named and sorted. Distinct
/// from [`ConfigDocument`], which carries the raw file text for the raw
/// editor: this is the typed, form-facing shape the read-only structured
/// panel renders.
///
/// A dedicated view-model rather than serialising `VaultConfig` directly:
/// `VaultConfig` isn't `Serialize`/`TS` (its `[variables]` block uses
/// `#[serde(flatten)]`, which ts-rs can't follow), and PR5a renders only
/// the meta/note-types/schemas slice — variables and `ignore` stay out of
/// the form for now.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigModel {
    /// The `[vault]` section — name and the active-project cap.
    pub vault: VaultMeta,
    /// Every `[note_types.<name>]`, sorted by `name` — a `HashMap`
    /// iterates in an unspecified order, so the projection sorts for a
    /// stable render (and stable tests).
    pub note_types: Vec<NamedNoteType>,
    /// Every `[schemas.<name>]`, sorted by `name` — same stable-order
    /// rationale as `note_types`.
    pub schemas: Vec<NamedSchema>,
}

/// One `[note_types.<name>]` entry: the map key lifted alongside its
/// definition. Not `#[serde(flatten)]`-ed (ts-rs friction) — the name
/// rides as its own field.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct NamedNoteType {
    pub name: String,
    pub note_type: CustomNoteType,
}

/// One `[schemas.<name>]` entry: the map key lifted alongside its
/// extension. Same no-flatten shape as [`NamedNoteType`].
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct NamedSchema {
    pub name: String,
    pub schema: SchemaExtension,
}

/// Project the live vault config into a [`ConfigModel`]. Public and
/// synchronous — the test seam, exercised directly over the Memory
/// doubles (mirrors [`read_config_impl`]).
///
/// Reads `vault.config()` — the parsed config currently *in effect* —
/// rather than re-parsing the file from disk. This is deliberate: the
/// live reload (#365, PR2/PR4) keeps `vault.config()` in lockstep with an
/// external `config.toml` edit, so the structured view (invalidated on
/// the `Config` area) always reflects the applied config, and the read
/// stays pure — no filesystem re-read, testable with an in-memory config
/// and no tempfile root.
pub fn read_config_model_impl(vault: &Vault) -> Result<ConfigModel, CmdError> {
    Ok(project_config_model(vault.config()))
}

/// Project a parsed [`VaultConfig`] into the form-facing [`ConfigModel`] —
/// the vault meta plus the note-type and schema tables lifted into sorted,
/// named lists. Shared by [`read_config_model_impl`] (the applied config)
/// and [`parse_config_model_impl`] (a candidate draft string), so the
/// read-only baseline and the editable form derive their model identically.
fn project_config_model(config: &VaultConfig) -> ConfigModel {
    let mut note_types: Vec<NamedNoteType> = config
        .note_types
        .iter()
        .map(|(name, note_type)| NamedNoteType {
            name: name.clone(),
            note_type: note_type.clone(),
        })
        .collect();
    note_types.sort_by(|a, b| a.name.cmp(&b.name));

    let mut schemas: Vec<NamedSchema> = config
        .schemas
        .iter()
        .map(|(name, schema)| NamedSchema {
            name: name.clone(),
            schema: schema.clone(),
        })
        .collect();
    schemas.sort_by(|a, b| a.name.cmp(&b.name));

    ConfigModel {
        vault: config.vault.clone(),
        note_types,
        schemas,
    }
}

/// The structured projection of the parsed config for the read-only form
/// (#365, PR5a). A pure read: no journal, no emit — same posture as
/// [`read_config`].
#[tauri::command]
pub async fn read_config_model(state: tauri::State<'_, AppState>) -> Result<ConfigModel, CmdError> {
    with_vault(&state.vault(), read_config_model_impl).await?
}

/// Parse a candidate config string into the form [`ConfigModel`] — the
/// display seam behind the EDITABLE form (#365, PR5b). Public and
/// synchronous test seam.
///
/// The form's source of truth for persistence is the shared draft STRING
/// plus the surgical `config_*` edits; this parses that draft into the
/// typed model the form renders, so the view always reflects the live
/// (possibly multi-edit, not-yet-saved) draft rather than the applied
/// config [`read_config_model`] returns. A candidate that is not valid
/// TOML surfaces as [`CmdError::Invalid`] (verbatim message), which the
/// form shows as a calm "fix it in Raw" state rather than crashing — no
/// domain validation runs here (that stays the save gate's job), only the
/// TOML parse the projection needs.
pub fn parse_config_model_impl(content: &str) -> Result<ConfigModel, CmdError> {
    let config: VaultConfig =
        toml::from_str(content).map_err(|err| CmdError::Invalid(err.to_string()))?;
    Ok(project_config_model(&config))
}

/// Parse a candidate draft string into the form model (#365, PR5b). Pure —
/// depends only on its input, so it runs inline rather than through
/// `with_vault`, mirroring `validate_config`.
#[tauri::command]
pub async fn parse_config_model(content: String) -> Result<ConfigModel, CmdError> {
    parse_config_model_impl(&content)
}

// --- Config form surgical edits (#365, PR5b) ---
//
// Four PURE string-transform commands the editable Config form drives.
// Each takes the current draft `content` plus the one piece the user
// touched, applies a comment-preserving `toml_edit` edit to *only* that
// table via [`cdno_core::config_edit`], and returns the new candidate
// string. They write NOTHING to the vault, journal NOTHING, and emit
// NOTHING: the form feeds the returned string back into the shared draft
// and the subsequent `save_config` runs the whole validate ->
// compare-and-swap -> write -> live-reload gate. This keeps the form on
// the exact same never-brick seam as the raw editor — it can only ever
// hand the gate a candidate string, never bypass it. The work is a small
// in-memory TOML parse + edit (no store, no index), so it runs inline
// rather than through `with_vault`, mirroring `validate_config`.

/// Insert or replace `[note_types.<name>]` in the draft, returning the
/// new config string for the form to `setDraft`. Pure — no write; the
/// later `save_config` persists.
#[tauri::command]
pub async fn config_set_note_type(
    content: String,
    name: String,
    note_type: CustomNoteType,
) -> Result<String, CmdError> {
    Ok(config_edit::set_note_type(&content, &name, &note_type)?)
}

/// Remove `[note_types.<name>]` from the draft, returning the new config
/// string. Idempotent (removing an absent type is a no-op success). Pure —
/// no write.
#[tauri::command]
pub async fn config_remove_note_type(content: String, name: String) -> Result<String, CmdError> {
    Ok(config_edit::remove_note_type(&content, &name)?)
}

/// Insert or replace `[schemas.<note_type>.fields.<field>]` in the draft,
/// returning the new config string. Pure — no write.
#[tauri::command]
pub async fn config_set_schema_field(
    content: String,
    note_type: String,
    field: String,
    spec: FieldSpec,
) -> Result<String, CmdError> {
    Ok(config_edit::set_schema_field(
        &content, &note_type, &field, &spec,
    )?)
}

/// Remove `[schemas.<note_type>.fields.<field>]` from the draft, returning
/// the new config string. Idempotent. Pure — no write.
#[tauri::command]
pub async fn config_remove_schema_field(
    content: String,
    note_type: String,
    field: String,
) -> Result<String, CmdError> {
    Ok(config_edit::remove_schema_field(
        &content, &note_type, &field,
    )?)
}

/// The validate → compare-and-swap → write core of the config save
/// (#365, PR3), public and synchronous — the test seam, exercised
/// directly over the Memory doubles. This is where the never-brick
/// invariant is proven: it validates `content` before any write and,
/// on a validation or conflict rejection, leaves the file untouched.
///
/// The `#[tauri::command]` [`save_config`] wraps this with the two
/// steps that need the app handle — the self-write journal + emit, and
/// the live reload — which a synchronous, disk-free test cannot reach.
pub fn save_config_impl(
    vault: &Vault,
    content: &str,
    expected_hash: &str,
) -> Result<ConfigDocument, ConfigSaveError> {
    vault.save_config_raw(content, expected_hash)
}

/// Dry-run the config validation `Vault::new` runs against `content`,
/// without touching the vault. `Ok(())` means the config would open;
/// `Err` carries a human-readable message (and, for a TOML syntax
/// error, the line/column). A pure read — it depends only on its input,
/// not on the open vault, so it never blocks the vault lock.
#[tauri::command]
pub async fn validate_config(content: String) -> Result<(), ConfigValidationError> {
    // The check is a small in-memory parse + validation pass; it doesn't
    // touch the store or index, so it runs inline rather than through
    // `with_vault` (there is no vault to borrow).
    validate_config_str(&content)
}

/// Save an edited `.cuaderno/config.toml` (#365, PR3) — the raw editor's
/// write, and the ONLY path that persists a config edit from the app.
/// Structured so it is impossible to commit a config the vault could not
/// reopen: the very first thing it does (inside [`save_config_impl`] →
/// `Vault::save_config_raw`) is run the exact validation `Vault::new`
/// runs, and any failure returns before a single byte is written.
///
/// The full order, as one indivisible command so no client can write
/// without validating:
///
/// 1. **VALIDATE** the candidate `content` server-side. On any error
///    return [`ConfigSaveError::Validation`] and write nothing.
/// 2. **COMPARE-AND-SWAP** on `expected_hash` against the current
///    on-disk config. On a mismatch (a concurrent hand-edit) return
///    [`ConfigSaveError::Conflict`] and write nothing.
/// 3. **WRITE** the buffer verbatim to `.cuaderno/config.toml`.
///    (Steps 1-3 are `save_config_impl`, run on the blocking pool.)
/// 4. **JOURNAL + EMIT** the write as a `Config` self-write so the
///    watcher suppresses its own echo while the frontend still refetches.
/// 5. **RELOAD** the vault live via [`reload_vault_config`] so the new
///    config applies without a restart. Step 1 already validated, so
///    this reload's `Vault::new` re-validation is belt-and-braces — it
///    passes by construction.
///
/// Returns the persisted [`ConfigDocument`] (re-read content + fresh
/// hash) so the UI can update its compare-and-swap baseline for the next
/// save without a separate read.
#[tauri::command]
pub async fn save_config<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    content: String,
    expected_hash: String,
) -> Result<ConfigDocument, ConfigSaveError> {
    // Steps 1-3 (validate → CAS → write verbatim) run in the domain on the
    // blocking pool. `with_vault`'s own `Err` is a blocking-pool panic
    // (JoinError) — mapped to `Internal` here so the command's error type
    // stays `ConfigSaveError` end to end.
    let vault = state.vault();
    let saved = {
        let content = content.clone();
        let expected_hash = expected_hash.clone();
        match with_vault(&vault, move |vault| {
            save_config_impl(vault, &content, &expected_hash)
        })
        .await
        {
            Ok(result) => result?,
            Err(cmd_err) => return Err(ConfigSaveError::Internal(cmd_err.to_string())),
        }
    };

    // Step 4 — journal the self-write + emit a `Config` change. The path is
    // always CONFIG_FILE (the domain confines the write to it), so it is
    // reconstructed here rather than threaded back from the domain.
    let path =
        VaultPath::new(CONFIG_FILE).map_err(|err| ConfigSaveError::Internal(err.to_string()))?;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Config]);

    // Step 5 — reload so the edit applies live. A reload failure after a
    // validated write is genuinely unexpected (the file we just wrote
    // passed step 1's gate), so surface it as Internal; the belt-and-braces
    // reload keeps the OLD vault live on any error, never leaving the
    // session vault-less.
    reload_vault_config(&app)
        .await
        .map_err(|err| ConfigSaveError::Internal(err.to_string()))?;

    Ok(saved)
}

/// Load `.cuaderno/config.toml` from `root` and build a fresh vault plus
/// its ignore set on the given store/index — the disk-read + rebuild core
/// [`rebuild_and_swap`] performs, factored out so it is testable over the
/// Memory doubles without a Tauri `AppHandle` (GH #365 PR4).
///
/// Both the ignore set and the vault are built before returning, so any
/// error (bad globs, a `TypeRegistry::validate` rejection, a TOML parse
/// failure) surfaces here with nothing yet swapped — the seam that lets
/// [`rebuild_and_swap`] keep its never-brick guarantee.
///
/// `pub` (not `pub(crate)`) so the integration test suite can prove
/// never-brick over the Memory doubles without a Tauri `AppHandle` — the
/// same test-seam posture as `read_config_impl` / `save_config_impl`.
pub fn load_vault_and_ignore(
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    root: &std::path::Path,
) -> Result<(Vault, IgnoreSet), DomainError> {
    // A missing file falls back to the default config, matching
    // `open_vault`'s first-launch behaviour.
    let config = VaultConfig::load(root)?;
    // Compile the fresh ignore set before moving `config` into `Vault::new`
    // (which recompiles its own copy for reconcile) — the watcher's swapped
    // matcher must be the same set the rebuilt index was reconciled against.
    let ignore = config.ignore_set()?;
    // Rebuild on the retained store/index — no SQLite reopen.
    let (vault, _report) = Vault::new(store, index, config)?;
    Ok((vault, ignore))
}

/// Rebuild the vault (and its ignore set) from the on-disk config and swap
/// both into managed state — the synchronous core shared by the async
/// [`reload_vault_config`] command and the watcher's external-config-edit
/// path (GH #365 PR4). Does NOT emit; the caller emits with the origin
/// that fits it (`SelfWrite` for a command-driven reload, `External` for a
/// watcher-driven one).
///
/// Never-brick: the fresh `IgnoreSet` and `Vault` are both built (via
/// [`load_vault_and_ignore`]) BEFORE either handle is swapped, so any error
/// returns with the OLD vault and OLD ignore set still live — the session
/// is never left vault-less. This is the last safety net beneath PR3's
/// pre-write validate gate.
///
/// Synchronous by design: the watcher thread is a plain `std::thread` and
/// calls this directly; the async command hops it onto the blocking pool.
pub fn rebuild_and_swap<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), DomainError> {
    let state = app.state::<AppState>();
    let (vault, ignore) =
        load_vault_and_ignore(state.store.clone(), state.index.clone(), &state.root)?;
    // Both built successfully — only now swap, so a failure above leaves the
    // old handles live. The vault swaps first, then its matching ignore set;
    // an in-flight command holding an owned `Arc` snapshot of the old vault
    // finishes cleanly (the swap never pulls a vault out from under it).
    state.vault.store(Arc::new(vault));
    state.ignore.store(Arc::new(ignore));
    Ok(())
}

/// Reload `.cuaderno/config.toml` from disk and swap the live vault
/// (#365, PR2; ignore-set swap added in PR4). The reload plumbing behind a
/// later config save:
///
/// 1. Re-read the config from `state.root` and rebuild the `Vault` on the
///    SAME store/index — no SQLite reopen. `Vault::new` re-runs the full
///    open-time safety net (`ignore_set` + `TypeRegistry::validate` +
///    reconcile), so a config that would not open is caught here.
/// 2. On success, `ArcSwap::store` the new vault AND the fresh ignore set
///    (both handled by [`rebuild_and_swap`]), so the watcher's next
///    reconcile honours any changed `ignore` globs. Commands already
///    running against the old vault finish cleanly — each holds an owned
///    `Arc` snapshot from `state.vault()`, so the swap never pulls a vault
///    out from under an in-flight call.
/// 3. Emit an all-areas `vault:changed` so the frontend refetches
///    everything the new config might have changed (note types, schemas,
///    folders).
///
/// Belt-and-braces (non-negotiable, design §safety-invariants): on ANY
/// rebuild error the swap is SKIPPED and the error returned — the OLD
/// vault stays live, so a bad on-disk config can never leave the session
/// vault-less. This is the last safety net beneath PR3's pre-write
/// validate gate.
///
/// The blocking rebuild (`VaultConfig::load` + `Vault::new`, both
/// synchronous disk/SQLite work) runs on the blocking pool so it never
/// stalls the async runtime — same posture as `with_vault`.
pub async fn reload_vault_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), CmdError> {
    // The rebuild is synchronous disk/SQLite work; run it on the blocking
    // pool so it never stalls the async runtime. `rebuild_and_swap` both
    // rebuilds and swaps under the never-brick guarantee.
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || rebuild_and_swap(&app2))
        .await
        .map_err(|e| {
            // A JoinError almost always means the rebuild closure panicked;
            // contain it, never leak the panic payload across the bridge.
            tracing::error!(error = %e, "vault reload panicked on the blocking pool");
            CmdError::Internal("internal error while reloading the config".to_owned())
        })??;

    // A config change can touch any view, so invalidate every area.
    if let Err(err) = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            // This process performed the reload; there is no external
            // writer to distinguish, and no self-write path to journal
            // (the reload wrote no files — it only re-read config).
            origin: Origin::SelfWrite,
            areas: all_areas(),
            paths: Vec::new(),
        },
    ) {
        tracing::warn!(error = %err, "failed to emit vault:changed after a config reload");
    }
    Ok(())
}

/// Reload the vault's config live (#365, PR2). Thin `#[tauri::command]`
/// over [`reload_vault_config`]; returns unit on success, or a
/// `CmdError` (with the old vault left live) if the on-disk config will
/// not open.
#[tauri::command]
pub async fn reload_config<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), CmdError> {
    reload_vault_config(&app).await
}
