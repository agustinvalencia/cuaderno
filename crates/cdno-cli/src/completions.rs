//! Shell completion plumbing for the `cdno` binary.
//!
//! Two layers, both fed by clap_complete:
//!
//! 1. **Static scripts** via `cdno completions <shell>`. The user sources
//!    the printed script in their shell's rc file; it wires the
//!    standard "what flags exist, what subcommands exist" tab table.
//!
//! 2. **Dynamic per-flag completion** via `CompleteEnv` (the runtime
//!    completion protocol the script invokes back into the binary).
//!    For slug-valued flags we open the vault on the fly and offer
//!    real project / portfolio / stewardship / question slugs as
//!    candidates. `--query` on `cdno action {promote,complete}` reads
//!    the action bullets on the chosen project.
//!
//! ## Failure mode
//!
//! Completers never panic and never error visibly. If the vault root
//! can't be discovered, the vault fails to open, or any query
//! returns an error, the completer returns an empty `Vec` — TAB
//! quietly does nothing rather than dumping a stack trace into the
//! user's prompt.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use chrono::Local;
use clap_complete::Shell;
use clap_complete::engine::CompletionCandidate;
use clap_complete::env::Shells;

use cdno_domain::Vault;

use crate::bootstrap;

/// Bin name we register the completion shim under. Centralised so the
/// completer self-call from the shell stays in sync with the actual
/// binary name we ship.
const BIN_NAME: &str = "cdno";

/// Env-var name that the `CompleteEnv::complete()` intercept at
/// `main()` boot looks for. Must match `CompleteEnv::with_factory`'s
/// default — switching this without updating both sides breaks
/// dynamic completion silently.
const COMPLETE_VAR: &str = "COMPLETE";

/// Print the runtime-completion registration shim for `shell` to
/// stdout.
///
/// The emitted script is the dynamic-engine shim (not the legacy
/// `_arguments`-style static script): it hooks into the shell and
/// re-invokes `cdno` with completion env vars set whenever the user
/// presses TAB. That re-invocation lands in the `CompleteEnv` intercept
/// at the top of `main`, which in turn calls our `ArgValueCompleter`
/// closures to surface vault-aware slugs.
///
/// Returns the writer error verbatim; bubbles back through main as a
/// proper exit code. In practice this only fails if stdout is closed.
pub fn print_script(shell: Shell) -> io::Result<()> {
    let shell_name = shell.to_string();
    let shells = Shells::builtins();
    let Some(emitter) = shells.completer(&shell_name) else {
        // `Shell` is a closed clap_complete enum and `Shells::builtins()`
        // covers every variant — this is dead code defensively kept so
        // a future clap_complete release adding a new `Shell` variant
        // surfaces here loudly rather than silently producing an
        // empty script.
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("shell {shell_name} is not registered with clap_complete"),
        ));
    };
    let mut out = io::stdout().lock();
    emitter.write_registration(COMPLETE_VAR, BIN_NAME, BIN_NAME, BIN_NAME, &mut out)
}

// ---------------------------------------------------------------------
// Vault opening — silent best-effort.
// ---------------------------------------------------------------------

/// Discover the vault from CWD and open it. Returns `None` on any
/// failure (no vault discovered, config invalid, index corrupt). The
/// caller is a tab-completion path; surfacing the error would smear
/// it across the user's prompt.
fn try_open_vault() -> Option<Vault> {
    let cwd = std::env::current_dir().ok()?;
    let root = bootstrap::discover_vault_root(&cwd)?;
    open_vault_at(&root)
}

/// Open the vault at `root`. Pulled out so unit tests can target a
/// known fixture directory rather than relying on CWD.
fn open_vault_at(root: &Path) -> Option<Vault> {
    bootstrap::open_vault(root).ok().map(|(v, _report)| v)
}

// ---------------------------------------------------------------------
// Per-flag completers.
//
// Each is a free `fn(&OsStr) -> Vec<CompletionCandidate>` so it slots
// straight into `clap_complete::engine::ArgValueCompleter::new(...)`.
// The `current` argument is what the user has typed so far for this
// flag; we hand back the full candidate list and let the shell do
// substring filtering. Filtering here would over-fit to one shell's
// matching style (zsh fuzzy, fish prefix-only, bash prefix-with-case).
// ---------------------------------------------------------------------

/// Active project slugs. Used by `--project` on action verbs and the
/// project-update `--slug` verbs (state, milestone, waiting).
pub fn complete_active_project(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let Ok(active) = vault.active_projects() else {
        return Vec::new();
    };
    active
        .into_iter()
        .filter_map(|(path, _fm)| slug_from_path(&path).map(CompletionCandidate::new))
        .collect()
}

/// Parked project slugs. Used by `cdno project activate --slug`.
pub fn complete_parked_project(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let Ok(parked) = vault.parked_projects() else {
        return Vec::new();
    };
    parked
        .into_iter()
        .filter_map(|(path, _fm)| slug_from_path(&path).map(CompletionCandidate::new))
        .collect()
}

/// Active + parked project slugs (the union). Used by `cdno project
/// show` and `cdno project park` so the picker isn't startled when
/// the user reaches for a non-active project by accident — the
/// domain still enforces eligibility at execute time.
pub fn complete_any_project(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(active) = vault.active_projects() {
        for (path, _fm) in active {
            if let Some(slug) = slug_from_path(&path) {
                out.push(CompletionCandidate::new(slug));
            }
        }
    }
    if let Ok(parked) = vault.parked_projects() {
        for (path, _fm) in parked {
            if let Some(slug) = slug_from_path(&path) {
                out.push(CompletionCandidate::new(slug));
            }
        }
    }
    out
}

/// Note-type names — the 11 built-ins plus any config-defined custom types.
/// Used by `--type` on `cdno search` and `<type>` on `cdno note`/`cdno
/// templates`. Opens the vault to include custom types; falls back to the
/// built-in set when no vault is discoverable (so completion still works
/// outside a vault, and never errors).
pub fn complete_note_type(_current: &OsStr) -> Vec<CompletionCandidate> {
    match try_open_vault() {
        Some(vault) => vault
            .type_registry()
            .all_names()
            .into_iter()
            .map(CompletionCandidate::new)
            .collect(),
        None => cdno_domain::note_type::NoteType::ALL
            .iter()
            .map(|nt| CompletionCandidate::new(nt.as_str()))
            .collect(),
    }
}

/// Portfolio slugs. Used by `--portfolio` on `cdno file` and `cdno
/// portfolio show`.
pub fn complete_portfolio(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let today = Local::now().date_naive();
    let Ok(summaries) = vault.list_portfolios(today) else {
        return Vec::new();
    };
    summaries
        .into_iter()
        .map(|s| CompletionCandidate::new(s.slug))
        .collect()
}

/// Stewardship slugs. Used by `--stewardship` on `cdno track` and
/// `cdno stewardship add-periodic`, plus `--slug` on `stewardship show`.
pub fn complete_stewardship(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let today = Local::now().date_naive();
    let Ok(summaries) = vault.list_stewardships(today) else {
        return Vec::new();
    };
    summaries
        .into_iter()
        .map(|s| CompletionCandidate::new(s.slug))
        .collect()
}

/// Every question slug (across statuses). Used by `--slug` on the
/// question lifecycle verbs (park, answer, retire, activate). The
/// status filter lives in the domain handler; completing too
/// permissively here is the right side to err on — the wrong-status
/// pick errors at execute time with a clear message.
pub fn complete_question(_current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(vault) = try_open_vault() else {
        return Vec::new();
    };
    let Ok(qs) = vault.list_questions() else {
        return Vec::new();
    };
    qs.into_iter()
        .map(|q| CompletionCandidate::new(q.slug))
        .collect()
}

// ---------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------

/// Derive a project slug from its vault path. `projects/foo.md` →
/// `foo`. Returns `None` for paths without a usable stem.
fn slug_from_path(path: &cdno_core::path::VaultPath) -> Option<String> {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
}
