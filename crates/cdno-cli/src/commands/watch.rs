//! `cdno watch` — keep the index honest while the vault changes under it.
//!
//! The index is a cache of the markdown on disk, rebuilt by reconciliation.
//! Every other CLI verb reconciles once, at vault open, and exits. That is
//! enough when the process is short-lived, but it means an edit made in
//! another editor is invisible to `cdno search` until the next command
//! happens to run. This watches for those edits and reconciles.
//!
//! Saves `cdno-core/src/watcher.rs`, whose only consumer was the desktop's
//! watcher thread (#597, #600).
//!
//! ## A feature build, not a port
//!
//! The desktop's watcher was coupled to app state — a write journal that
//! stops the app hearing its own writes, and react-query invalidation.
//! None of that transfers, so there is no reference implementation to diff
//! against and correctness cannot be established by comparison. What the
//! CLI needs is narrower: on a debounced external change, reconcile.
//!
//! ## The self-echo is via DIRECTORIES, not via the index
//!
//! This was measured, not reasoned about, and the first guess was wrong.
//! The obvious echo is the index: reconciliation writes
//! `.cuaderno/index.db`, so reacting to that would re-trigger itself.
//! Filtering `.cuaderno/` alone does NOT stop the loop, and a build that
//! did only that spun 18 reconcile passes in 6 seconds on a freshly
//! initialised vault with nobody touching it.
//!
//! The real source is the walk. `reconcile` calls `store.walk_dir` over
//! the whole vault, reading every directory; inotify reports those reads,
//! so each pass emits a `Changed` event for `projects`, `journal/2026`,
//! `stewardships` and every other folder — which triggers the next pass.
//! The index was never the problem, because a pass that finds nothing new
//! still stirs the directories it looked in.
//!
//! So relevance is decided on the FILES that can actually change the
//! index: a path under `.cuaderno/` is ignored (our own writes, and
//! template edits that the index does not care about), and everything
//! else must be a `.md` file. A directory event carries no `.md`
//! extension, so the walk's own footprints are filtered by the same rule
//! rather than by a special case.
//!
//! This is also why the desktop's write journal does not transfer: it
//! exists because the app writes NOTES and must not hear itself. This
//! process writes nothing but the index, so it needs no journal — but it
//! does need to not hear its own reads, which the journal never covered.
//!
//! ## What the `.md`-only rule gives up
//!
//! Only markdown is indexed, so a PDF appearing changes no row by itself.
//! Filing an attachment writes or updates its `.md` stub as well, and
//! that is caught. A directory removed takes its notes with it, and each
//! of those arrives as a `Removed` `.md` path in the same batch. The case
//! genuinely missed is a bare directory rename with no note touched,
//! which no reconcile would change either. `Rescan` remains the backstop
//! whenever the backend admits it dropped events.
//!
//! A `config.toml` edit is ignored too. See `Vault::reconcile` —
//! honouring new `ignore` globs means rebuilding the vault, not
//! reconciling it, so `cdno watch` names the boundary and asks to be
//! restarted rather than quietly using stale globs.
//!
//! ## Bursts are coalesced
//!
//! The watcher already debounces (400ms) to absorb an editor's atomic-save
//! storm. A `git checkout` across hundreds of notes still arrives as
//! several batches, so every batch waiting in the channel is drained
//! before reconciling — one pass for the burst rather than one per batch.
//! Events that land DURING a pass queue up and are drained by the next
//! iteration, which is also what keeps this process from ever running two
//! passes at once (#459 is about passes interleaving; this loop is
//! single-threaded and synchronous, so it cannot race itself).

use std::path::Path;
use std::sync::mpsc::channel;

use anyhow::{Context, Result};

use cdno_core::watcher::{FileEvent, FileWatcher, FsFileWatcher};
use cdno_core::{paths, reconcile::ReconciliationReport};

use crate::bootstrap;

/// Whether a watched path should trigger a reconcile.
///
/// Its own function, and `pub`, because this is the predicate the
/// self-echo argument above rests on — a test can hold it to that
/// directly, rather than trying to observe a spin loop.
pub fn is_relevant(event: &FileEvent) -> bool {
    match event {
        // The backend says it may have dropped events, so the batch
        // cannot be trusted to be complete: reconcile regardless of what
        // else is in it.
        FileEvent::Rescan => true,
        FileEvent::Changed(path) | FileEvent::Removed(path) => {
            let path = path.as_path();
            // `.cuaderno/` holds our own index writes, plus templates
            // whose contents the index does not track. `Path::starts_with`
            // compares whole components, which is why this goes through
            // `as_path()` rather than a string prefix: a sibling directory
            // named `.cuadernoX/` shares the textual prefix but is an
            // ordinary part of the vault, and ignoring it would be a
            // missed reconcile.
            if path.starts_with(paths::CUADERNO_DIR) {
                return false;
            }
            // Markdown only. This is what stops the walk's own directory
            // reads from re-triggering the pass that made them — see the
            // module docs; a directory event has no `.md` extension, so it
            // falls out here rather than needing a rule of its own.
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        }
    }
}

/// Format a completed pass for the human watching the terminal.
///
/// Reports the counts that changed rather than a bare "reconciled": the
/// point of leaving this running is to see that an edit landed, and
/// `0 added, 0 updated` is exactly the case where something did not.
pub fn describe_pass(report: &ReconciliationReport) -> String {
    let mut parts = Vec::new();
    if report.added > 0 {
        parts.push(format!("{} added", report.added));
    }
    if report.updated > 0 {
        parts.push(format!("{} updated", report.updated));
    }
    if report.removed > 0 {
        parts.push(format!("{} removed", report.removed));
    }
    if parts.is_empty() {
        parts.push("no index change".to_owned());
    }
    let mut line = format!("reconciled: {}", parts.join(", "));
    if !report.errors.is_empty() {
        // Per-file failures do not abort the pass, so they would be
        // invisible without this — a note that stopped being indexed
        // because of a typo in its frontmatter is precisely what someone
        // watching wants to be told.
        line.push_str(&format!(
            " ({} file(s) could not be indexed)",
            report.errors.len()
        ));
    }
    line
}

pub fn run(root: &Path) -> Result<()> {
    // Opening the vault reconciles once, so the index is correct before
    // the first event rather than only after it.
    let (vault, report) = bootstrap::open_vault(root)?;
    println!(
        "Watching {} — {}. Press Ctrl-C to stop.",
        root.display(),
        describe_pass(&report)
    );
    println!("Note: a change to .cuaderno/config.toml needs a restart to take effect.");

    let (sender, receiver) = channel();
    let mut watcher = FsFileWatcher::new(root);
    watcher
        .watch(sender)
        .context("starting the filesystem watcher")?;

    // Ctrl-C terminates the process by default, which is the whole of the
    // stop story: nothing here needs unwinding. A pass interrupted partway
    // leaves the index incomplete, and that is safe — it is a cache, the
    // markdown is untouched, and the next open reconciles it back.
    while let Ok(batch) = receiver.recv() {
        let mut relevant = batch.iter().any(is_relevant);
        // Drain whatever else is already queued so a burst becomes one
        // pass. `try_recv` is non-blocking, so this stops as soon as the
        // channel is empty rather than waiting for a quiet period the
        // debouncer has already waited for.
        while let Ok(extra) = receiver.try_recv() {
            relevant |= extra.iter().any(is_relevant);
        }
        if !relevant {
            continue;
        }
        match vault.reconcile() {
            Ok(report) => println!("{}", describe_pass(&report)),
            // A failed pass must not end the watch: the usual cause is
            // transient (a file half-written by another process), and the
            // next event reconciles again. Exiting here would leave the
            // index stale with nothing watching it.
            Err(err) => eprintln!("reconcile failed, still watching: {err}"),
        }
    }

    Ok(())
}
